// Copyright (c) 2026 Robert Grosse. All rights reserved.
use std::collections::HashMap;

use crate::ast;
use crate::ast::StringId;
use crate::coercion::*;
use crate::core::*;
use crate::introspect_types::*;
use crate::kinds::*;
use crate::parse_patterns::*;
use crate::parse_types::*;
use crate::restamp::*;
use crate::spans::*;
use crate::spines::*;
use crate::subsumption::*;
use crate::templates::*;
use crate::type_errors::*;
use crate::types::*;
use crate::unification::*;
use crate::unwindmap::*;

use UTypeHead::*;
use VTypeHead::*;

type Result<T> = std::result::Result<T, SpannedError>;

#[derive(Debug, Clone)]
pub enum TypeBinding {
    Con(TyConDefInd),
    Types((Value, Use), KindVar),
}
impl TypeBinding {
    pub fn inspect(core: &mut TypeCheckerCore, p: (Value, Use), kind: &KindVar) -> std::result::Result<Self, ICE> {
        if let ValOrHole::Val(VTypeConstructor(data), _) = core.get_val_or_hole(p.0)?
            && data.spine.is_none()
        {
            return Ok(TypeBinding::Con(data.category));
        }
        Ok(TypeBinding::Types(p, kind.clone()))
    }

    pub fn get(&self, core: &mut TypeCheckerCore, span: Span) -> Result<(ParsedTypeHead, KindVar)> {
        Ok(match self {
            TypeBinding::Con(i) => {
                let tycon_def = core.tycons.get(*i);
                let kind = tycon_def.kind().clone();
                let kind_span = tycon_def.span().unwrap_or(span);
                let head = ParsedTypeHead::Type(core.simple(*i, span));
                (head, KindVar::Known((kind, kind_span)))
            }
            TypeBinding::Types(p, kind) => {
                let head = ParsedTypeHead::Type(*p);
                (head, kind.clone())
            }
        })
    }
}

type BindingsUnwindPoint = (UnwindPoint, UnwindPoint, ScopeLvl);
#[derive(Debug, Clone)]
pub struct ValueBinding {
    val: Value,
    span: Span,
    is_mod: bool,
}
impl ValueBinding {
    pub fn new(val: Value, span: Span) -> Self {
        Self {
            val,
            span,
            is_mod: false,
        }
    }

    pub fn get_type_member(
        &self,
        core: &mut TypeCheckerCore,
        name: Spanned<StringId>,
        mod_span: Span,
        path_span: Span,
    ) -> Result<(ParsedTypeHead, KindVar)> {
        if !self.is_mod {
            return Err(SpannedError::new2(
                "SyntaxError: Expected module binding.",
                mod_span,
                "Note: Variable was defined as a non-module binding here:",
                self.span,
            ));
        }

        // If the module type is never, just return (never, any) pairs for any possible type name.
        if self.val == BOT {
            let p = (BOT, TOP);
            return Ok((ParsedTypeHead::Type(p), KindVar::new_var(name.1)));
        }

        let aliases = core.get_aliases(self.val)?;
        let binding = aliases
            .get(&name.0)
            .ok_or_else(|| {
                SpannedError::new2(
                    "TypeError: Undefined module type member:",
                    name.1,
                    "Note: Module was defined here:",
                    self.span,
                )
            })?
            .clone();

        binding.get(core, path_span)
    }
}

enum MaybeBinding<T> {
    Def(T),
    Hidden(Span),
    RecHint(Span),
}

pub struct BindingMap<T>(UnwindMap<StringId, MaybeBinding<T>>);
impl<T> BindingMap<T> {
    fn new() -> Self {
        Self(UnwindMap::new())
    }

    pub fn insert(&mut self, name: StringId, binding: T) {
        self.0.insert(name, MaybeBinding::Def(binding));
    }

    fn iter(&self) -> impl Iterator<Item = (&StringId, &T)> {
        self.0.m.iter().filter_map(|(k, v)| match v {
            MaybeBinding::Def(b) => Some((k, b)),
            _ => None,
        })
    }

    fn get(&self, name: StringId, msg: &str, span: Span) -> Result<&T> {
        use MaybeBinding::*;
        match self.0.get(&name) {
            Some(Def(b)) => Ok(b),
            Some(Hidden(hidden_by)) => Err(SpannedError::new2(
                format!("SyntaxError: Undefined {}", msg),
                span,
                "Note: A binding with this name was defined previously but was hidden here:",
                *hidden_by,
            )),
            Some(RecHint(def_span)) => {
                let mut e = SpannedError::new1(format!("SyntaxError: Undefined {}", msg), span);
                e.push_str("Hint: Consider making the definition recursive like this:");
                e.push_insert("rec ", *def_span, "");
                Err(e)
            }
            None => Err(SpannedError::new1(format!("SyntaxError: Undefined {}", msg), span)),
        }
    }

    pub fn hide(&mut self, name: StringId, hidden_by: Span) {
        if self.0.m.contains_key(&name) {
            self.0.insert(name, MaybeBinding::Hidden(hidden_by));
        }
    }

    fn rec_hint(&mut self, name: StringId, def_span: Span) {
        // Only insert rec hint if there is no real binding already defined with that name.
        if !matches!(self.0.m.get(&name), Some(MaybeBinding::Def(..))) {
            self.0.insert(name, MaybeBinding::RecHint(def_span));
        }
    }
}

pub struct Bindings {
    pub vars: BindingMap<ValueBinding>,
    pub types: BindingMap<TypeBinding>,
    pub scopelvl: ScopeLvl,
}
impl Bindings {
    fn new() -> Self {
        Self {
            vars: BindingMap::new(),
            types: BindingMap::new(),
            scopelvl: ScopeLvl::MIN,
        }
    }

    fn new_with_builtins(types: &[(StringId, TyConDefInd)]) -> Self {
        let mut this = Self::new();
        for (name, ind) in types {
            this.types.insert(*name, TypeBinding::Con(*ind));
        }
        this.make_permanent();
        this
    }

    fn unwind_point(&mut self) -> BindingsUnwindPoint {
        (self.vars.0.unwind_point(), self.types.0.unwind_point(), self.scopelvl)
    }

    fn unwind(&mut self, n: BindingsUnwindPoint) {
        self.vars.0.unwind(n.0);
        self.types.0.unwind(n.1);
        self.scopelvl = n.2;
    }

    fn make_permanent(&mut self) {
        self.vars.0.make_permanent();
        self.types.0.make_permanent();
    }

    fn add_bindings(&mut self, other: ParsedBindings) {
        for (name, vb) in other.vars {
            self.vars.insert(name, vb);
        }
        for (name, tycon) in other.types {
            self.types.insert(name, TypeBinding::Con(tycon));
        }
    }

    fn add_var_bindings(&mut self, other: &[(StringId, (Span, Value))]) {
        for (name, (span, v)) in other {
            self.vars.insert(*name, ValueBinding::new(*v, *span));
        }
    }

    pub fn lookup_var(&self, name: StringId, span: Span) -> Result<&ValueBinding> {
        self.vars.get(name, "variable", span)
    }

    pub fn lookup_type(&self, name: StringId, span: Span) -> Result<&TypeBinding> {
        self.types.get(name, "type", span)
    }
}

#[allow(non_snake_case)]
pub struct TypeckState {
    core: TypeCheckerCore,
    bindings: Bindings,
    imports: HashMap<ast::ImportId, Value>,

    TY_BOOL: TyConDefInd,
    TY_FLOAT: TyConDefInd,
    TY_INT: TyConDefInd,
    TY_STR: TyConDefInd,
    builtin_types: Vec<(StringId, TyConDefInd)>,
}
impl TypeckState {
    #[allow(non_snake_case)]
    pub fn new(strings: &mut lasso::Rodeo) -> Self {
        let mut core = TypeCheckerCore::new();
        let mut builtin_types = Vec::new();

        macro_rules! add_builtin_type {
            ($name:expr) => {{
                let name = strings.get_or_intern_static($name);
                let i = core.add_builtin_type(name);
                builtin_types.push((name, i));
                i
            }};
        }

        let TY_BOOL = add_builtin_type!("bool");
        let TY_FLOAT = add_builtin_type!("float");
        let TY_INT = add_builtin_type!("int");
        let TY_STR = add_builtin_type!("str");

        Self {
            core,
            bindings: Bindings::new_with_builtins(&builtin_types),
            imports: HashMap::new(),

            TY_BOOL,
            TY_FLOAT,
            TY_INT,
            TY_STR,
            builtin_types,
        }
    }

    pub fn with<'a>(&'a mut self, strings: &'a mut lasso::Rodeo) -> Typeck<'a> {
        Typeck {
            core: &mut self.core,
            bindings: &mut self.bindings,
            imports: &mut self.imports,
            strings,
            TY_BOOL: self.TY_BOOL,
            TY_FLOAT: self.TY_FLOAT,
            TY_INT: self.TY_INT,
            TY_STR: self.TY_STR,
            builtin_types: &self.builtin_types,
        }
    }
}

#[allow(non_snake_case)]
pub struct Typeck<'a> {
    core: &'a mut TypeCheckerCore,
    bindings: &'a mut Bindings,
    imports: &'a mut HashMap<ast::ImportId, Value>,
    strings: &'a mut lasso::Rodeo,
    TY_BOOL: TyConDefInd,
    TY_FLOAT: TyConDefInd,
    TY_INT: TyConDefInd,
    TY_STR: TyConDefInd,

    builtin_types: &'a [(StringId, TyConDefInd)],
}
impl<'a> Typeck<'a> {
    fn type_parser(&mut self) -> TypeParser<'_> {
        TypeParser::new(self.core, self.strings, self.bindings, self.bindings.scopelvl)
    }

    fn type_parser_at(&mut self, scopelvl: ScopeLvl) -> TypeParser<'_> {
        TypeParser::new(self.core, self.strings, self.bindings, scopelvl)
    }

    fn parse_type_signature(&mut self, tyexpr: &ast::STypeExpr) -> Result<(Value, Use)> {
        self.type_parser().parse_type(tyexpr)
    }

    fn literal_tycon(&self, lit: &ast::expr::Literal) -> TyConDefInd {
        use ast::expr::Literal::*;
        match lit {
            Bool => self.TY_BOOL,
            Float => self.TY_FLOAT,
            Int => self.TY_INT,
            Str => self.TY_STR,
        }
    }

    fn flow_top(&mut self, bound: Use, span: Span) {
        let t = self.core.new_val(VTypeHead::VTop, span);
        self.core.flow(t, bound, span);
    }

    fn check_expr(&mut self, expr: &ast::SExpr, bound: Use) -> Result<()> {
        use ast::Expr::*;
        match &expr.0 {
            // Invariant: Non-inferred types are always valid for the current scope. To ensure
            // this, block exprs are only checkable, not inferrable. This means they'll be wrapped
            // in a fresh inference var (in the parent scope) if necessary.
            Block(e) => {
                let mark = self.bindings.unwind_point();

                for stmt in e.statements.iter() {
                    self.check_statement(stmt, false)?;
                }

                if let Some(e) = &e.expr {
                    self.check_expr(e, bound)?;
                } else {
                    self.flow_top(bound, expr.1);
                };

                self.bindings.unwind(mark);
            }
            Call(e) => {
                let arg_type = self.infer_expr(&e.arg)?;
                let expected_func_type = self.core.new_use_with_src(
                    UFunc {
                        arg: arg_type,
                        ret: bound,
                        prop: FuncProperties::default(),
                    },
                    expr.1,
                    UseSrc::CallExpr,
                );

                let lhs_type = self.infer_expr(&e.func)?;
                let cb = FuncInstantiationCheck {
                    scopelvl: self.bindings.scopelvl,
                    lhs_expr_span: e.func.1,
                    rhs: expected_func_type,
                };
                let cb = InnerCallback::Func(cb);
                let cb = UnwrapCoercionCallback { cb };
                let target_span = expr.1;
                create_unification_nodes(self.core, self.bindings.scopelvl, target_span, cb, lhs_type)?;
            }
            FieldAccess(e) => {
                let bound = self.core.obj_use(vec![(e.field.0, (bound, None, e.field.1))], e.field.1);

                let lhs_type = self.infer_expr(&e.expr)?;
                let cb = UnwrapCoercionCallback {
                    cb: InnerCallback::Record(bound),
                };
                let target_span = e.field.1;
                create_unification_nodes(self.core, self.bindings.scopelvl, target_span, cb, lhs_type)?;
            }
            FieldSet(e) => {
                let rhs_type = self.infer_expr(&e.value)?;
                let bound = self
                    .core
                    .obj_use(vec![(e.field.0, (bound, Some(rhs_type), e.field.1))], e.field.1);

                let lhs_type = self.infer_expr(&e.expr)?;
                let cb = UnwrapCoercionCallback {
                    cb: InnerCallback::Record(bound),
                };
                let target_span = e.field.1;
                create_unification_nodes(self.core, self.bindings.scopelvl, target_span, cb, lhs_type)?;
            }
            If(e) => {
                let bool_use = self.core.simple_use(self.TY_BOOL, e.cond.1, UseSrc::None);
                self.check_expr(&e.cond.0, bool_use)?;
                self.check_expr(&e.then_expr, bound)?;

                if let Some(e) = &e.else_expr {
                    self.check_expr(e, bound)?;
                } else {
                    self.flow_top(bound, expr.1);
                };
            }
            Loop(e) => {
                let ucd = UCaseData::new(vec![
                    (self.strings.get_or_intern_static("Break"), TOP),
                    (self.strings.get_or_intern_static("Continue"), TOP),
                ]);

                let bound = self.core.new_use(UTypeHead::UCase(ucd), expr.1);
                self.check_expr(&e.body, bound)?;
            }
            Match(e) => {
                let mut tp = self.type_parser();
                let (input_bound, arms) = parse_match_cases(e, &mut tp)?;

                self.check_expr(&e.expr.0, input_bound)?;

                for arm in arms {
                    for (bindings, guard) in arm.case_results {
                        let mark = self.bindings.unwind_point();
                        self.bindings.add_var_bindings(&bindings);

                        let bool_use = self.core.simple_use(self.TY_BOOL, guard.1, UseSrc::None);
                        self.check_expr(guard, bool_use)?;
                        self.bindings.unwind(mark);
                    }

                    // Now check the body using the merged bindings
                    let mark = self.bindings.unwind_point();
                    arm.merged_bindings
                        .add_bindings(&mut self.bindings.vars, self.core, arm.body.1, self.bindings.scopelvl);

                    self.check_expr(arm.body, bound)?;
                    self.bindings.unwind(mark);
                }
            }

            // Cases that should be inferred instead
            BinOp(_) | Case(_) | Coerce(_) | FuncDef(_) | Identity(_) | Literal(_) | Record(_) | Typed(_) | Variable(_)
            | Unsafe(_) => {
                // Span is just an arbitrary span (usually that of the current expression) used
                // to help users diagnose cause of a type error that doesn't go through any holes.
                let t = self.infer_expr(expr)?;
                self.core.flow(t, bound, expr.1);
            }
        };
        self.core.run_pending_checks(self.strings)?;
        Ok(())
    }

    fn infer_expr(&mut self, expr: &ast::SExpr) -> Result<Value> {
        use ast::Expr::*;

        match &expr.0 {
            BinOp(e) => {
                let (arg_class, ret_class) = &e.op_type;
                let (lhs_bound, rhs_bound) = match arg_class {
                    Some(arg_class) => {
                        let cls = self.literal_tycon(arg_class);
                        let src = UseSrc::BinOpExpr(e.str);
                        (
                            self.core.simple_use(cls, e.lhs.1, src),
                            self.core.simple_use(cls, e.rhs.1, src),
                        )
                    }
                    None => (TOP, TOP),
                };
                self.check_expr(&e.lhs, lhs_bound)?;
                self.check_expr(&e.rhs, rhs_bound)?;

                let cls = self.literal_tycon(ret_class);
                Ok(self.core.simple_val(cls, expr.1))
            }
            Case(e) => {
                let val_type = self.infer_expr(&e.expr)?;
                Ok(self.core.new_val(
                    VCase {
                        case: (e.tag.0, val_type),
                    },
                    e.tag.1,
                ))
            }
            Coerce(e) => {
                let comparison_key = ComparisonKey(expr.1);

                // Parse substitutions
                let (target_type, subs) = self.type_parser().parse_coerce_target(&e.target, Some(comparison_key))?;

                let cb = SubsumptionCheck::new(
                    self.core,
                    self.bindings.scopelvl,
                    e.target.ty.1,
                    e.target.hole_src_for_implicit_substitution(),
                    target_type.1,
                    subs,
                )?;
                let cb = InnerCallback::Sub(cb);
                let cb = UnwrapCoercionCallback { cb };

                let lhs_type = if let Some(input_type) = &e.input_type {
                    let input_type = self.parse_type_signature(input_type)?;
                    self.check_expr(&e.expr, input_type.1)?;
                    input_type.0
                } else {
                    let v = self.infer_expr(&e.expr)?;
                    let v = self.core.add_hole_if_higher_priority(
                        v,
                        HoleSrc::OptAscribe(e.expr.1),
                        self.bindings.scopelvl,
                        e.expr.1,
                    );
                    v
                };

                create_unification_nodes(self.core, self.bindings.scopelvl, expr.1, cb, lhs_type)?;
                Ok(target_type.0)
            }
            FuncDef(e) => {
                let parsed = self.type_parser().parse_func_sig(e, expr.1)?;

                let func_type = parsed.func_type;
                let ret_bound = parsed.ret_bound;

                let mark = self.bindings.unwind_point();
                self.bindings.scopelvl.inc();
                self.bindings.add_bindings(parsed.bindings);

                self.check_expr(&e.body, ret_bound)?;

                self.bindings.unwind(mark);
                Ok(func_type)
            }
            Identity(type_annot) => {
                let span = expr.1;
                let (v, u) =
                    self.type_parser()
                        .parse_type_or_hole(type_annot.as_ref(), span, HoleSrc::IdentityAscribe(span))?;
                let prop = FuncProperties { is_identity: true };
                Ok(self.core.new_val(VTypeHead::VFunc { arg: u, ret: v, prop }, span))
            }
            Literal(e) => {
                use ast::expr::Literal::*;
                let span = e.value.1;

                let ty = match e.lit_type {
                    Bool => self.TY_BOOL,
                    Float => self.TY_FLOAT,
                    Int => self.TY_INT,
                    Str => self.TY_STR,
                };
                Ok(self.core.simple_val(ty, span))
            }
            Record(e) => {
                let mut field_checker = DupNameChecker::new("field");
                let mut fields = HashMap::with_capacity(e.fields.len());
                let mut alias_checker = DupNameChecker::new("type alias");
                let mut aliases = HashMap::new();

                for ((name, name_span), member) in &e.fields {
                    use ast::expr::RecordExprMember::*;
                    match member {
                        Field(mutable, expr, type_annot) => {
                            field_checker.add(*name, *name_span)?;

                            if *mutable {
                                let (v, u) = self.type_parser().parse_type_or_hole(
                                    type_annot.as_ref(),
                                    *name_span,
                                    HoleSrc::OptAscribe(*name_span),
                                )?;

                                self.check_expr(expr, u)?;
                                fields.insert(*name, (v, Some(u), *name_span));
                            } else {
                                // For immutable fields, use the type annotation if one was supplied
                                // but do not create a hole (inference variable) if there wasn't,
                                let t = if let Some(ty) = type_annot {
                                    let (v, u) = self.parse_type_signature(ty)?;
                                    self.check_expr(expr, u)?;
                                    v
                                } else {
                                    self.infer_expr(expr)?
                                };

                                fields.insert(*name, (t, None, *name_span));
                            }
                        }
                        Alias(ty) => {
                            alias_checker.add(*name, *name_span)?;

                            let binding = self.type_parser().parse_type_alias(ty)?;
                            aliases.insert(*name, binding);
                        }
                    }
                }
                Ok(self.core.new_val(VTypeHead::VObj { fields, aliases }, expr.1))
            }
            Typed(e) => {
                let sig_type = self.parse_type_signature(&e.type_expr)?;
                self.check_expr(&e.expr, sig_type.1)?;
                Ok(sig_type.0)
            }
            Variable(e) => {
                let v = self.bindings.lookup_var(e.name, expr.1).map(|vb| vb.val)?;
                // Need to restamp in case it is a 0-arg ADT value.
                let v = restamp_func_or_val(self.core, v, expr.1)?;
                Ok(v)
            }
            Unsafe(_) => Ok(BOT),

            // Cases that have to be checked instead
            Block(_) | Call(_) | FieldAccess(_) | FieldSet(_) | If(_) | Loop(_) | Match(_) => {
                let (v, u) = self.core.var(HoleSrc::CheckedExpr(expr.1), self.bindings.scopelvl);
                self.check_expr(expr, u)?;
                Ok(v)
            }
        }
    }

    fn check_let_def(&mut self, lhs: &ast::LetPattern, expr: &ast::SExpr) -> Result<()> {
        // Check if left hand side is a simple assignment to add rec hint and possibly skip inference
        if let &ast::LetPattern::Var(ast::VarPattern((Some(name), name_span), ref annot)) = lhs {
            // Add hint to switch to "let rec: if recursive.
            self.bindings.vars.rec_hint(name, name_span);

            if let Some(tyexpr) = annot {
                // Explicit type annotation
                let (v, u) = self.parse_type_signature(tyexpr)?;
                self.check_expr(expr, u)?;
                self.bindings.vars.insert(name, ValueBinding::new(v, name_span));
            } else {
                // No type annotation: only add an inference var if the inferred type is already an inference var with lower priority.
                let ty = self.infer_expr(expr)?;
                let ty = self.core.add_hole_if_higher_priority(
                    ty,
                    HoleSrc::OptAscribe(name_span),
                    self.bindings.scopelvl,
                    name_span,
                );

                self.bindings.vars.insert(name, ValueBinding::new(ty, name_span));
            }
        } else {
            let mut tp = self.type_parser();
            let (bound, bindings) = parse_unconditional_match(lhs, &mut tp)?;

            // Important: The RHS of a let needs to be evaluated *before* we add the bindings from the LHS
            self.check_expr(expr, bound)?;

            // Now add the pattern bindings
            self.bindings.add_var_bindings(&bindings);
        }
        Ok(())
    }

    fn check_let_rec_defs(&mut self, defs: &Vec<ast::LetRecDefinition>) -> Result<()> {
        let mut temp = Vec::new();
        // Parse the function signatures
        // Materialize the outer function types and assign to bindings
        for &((name, name_span), ref annot, (ref expr, rhs_span)) in defs.iter() {
            match expr {
                ast::Expr::FuncDef(e) => {
                    let parsed = self.type_parser().parse_func_sig(e, rhs_span)?;

                    let ty = if let Some(tyexpr) = annot {
                        let (v, u) = self.parse_type_signature(tyexpr)?;
                        self.core.flow(parsed.func_type, u, tyexpr.1);
                        v
                    } else {
                        parsed.func_type
                    };

                    self.bindings.vars.insert(name, ValueBinding::new(ty, name_span));
                    temp.push((parsed, &e.body));
                }
                _ => {
                    return Err(SpannedError::new1(
                        "SyntaxError: Let rec can only assign function definitions.",
                        rhs_span,
                    ));
                }
            }
        }

        // Now process the body of each function definition one by one
        for (parsed, body) in temp {
            let mark = self.bindings.unwind_point();
            self.bindings.scopelvl.inc();
            self.bindings.add_bindings(parsed.bindings);

            self.check_expr(body, parsed.ret_bound)?;

            self.bindings.unwind(mark);
        }

        Ok(())
    }

    fn check_module_binding(
        &mut self,
        name: Spanned<StringId>,
        v: Value,
        extra_hole_src: Option<HoleSrc>,
        coercions: &ast::ImplicitCoercions,
    ) -> Result<()> {
        let (dt, params) = self.core.v_dissect(v)?;
        let params = params.cloned().unwrap_or_default();
        let v = match dt {
            DissectedType::Constructor(data) => {
                apply_unwrap_coercion(self.core, Some(data), &params, v, CoercionTargetCategory::Record)?
            }
            _ => v,
        };
        let lst = self.core.v_as_spine_or_type(v)?;
        let v = self.pin_existential_type(name, lst, extra_hole_src, coercions)?;

        let mut binding = ValueBinding::new(v, name.1);
        binding.is_mod = true;
        self.bindings.vars.insert(name.0, binding);

        Ok(())
    }

    fn pin_existential_type(
        &mut self,
        name: Spanned<StringId>,
        lst: LoadedSpineOrType<Value>,
        extra_hole_src: Option<HoleSrc>,
        coercions: &ast::ImplicitCoercions,
    ) -> Result<Value> {
        let name_span = name.1;
        Ok(match lst {
            LoadedSpineOrType::Type(v) => {
                use ValOrHole::*;
                match self.core.get_val_or_hole(v)? {
                    Never => {}
                    Val(head, span) => {
                        if let VTypeHead::VObj { .. } = head {
                            // Ok
                        } else {
                            return Err(SpannedError::new2(
                                "TypeError: Type of module binding here must be a record type.",
                                name_span,
                                "but it may be a non-record type here:",
                                span,
                            ));
                        }
                    }
                    Hole(src) => {
                        let mut src = src.src;
                        if let Some(extra) = extra_hole_src
                            && extra.priority() > src.priority()
                        {
                            src = extra;
                        }

                        let mut e = SpannedError::new1("TypeError: Module binding must have a known type.", name_span);
                        e.push_str("Hint: Consider adding an explicit type annotation here:");
                        src.add_to_error(&mut e, self.strings);
                        return Err(e);
                    }
                }
                v
            }
            LoadedSpineOrType::Spine(spine, params) => {
                if let SpineContents::Func(ty) = &spine.template {
                    return Err(SpannedError::new2(
                        "TypeError: Type of module binding here must be a record type.",
                        name_span,
                        "but it may be a function type here:",
                        ty.0,
                    ));
                }

                // Increment scope level since we're adding new types.
                self.bindings.scopelvl.inc();

                let mut new_types = Vec::new();
                for (kind, param_name) in spine.poly_params.iter() {
                    let tycon_ind = self.core.tycons.add_custom(
                        Some(name.0),
                        *param_name,
                        name_span,
                        self.bindings.scopelvl,
                        kind.0.clone(),
                    );
                    new_types.push((*param_name, tycon_ind));
                }

                let temp_type_map = new_types.iter().copied().collect();
                let v = spine.template.instantiate_val_adding_aliases(self.core, &params, new_types)?;

                if let VTypeHead::VObj { fields, aliases: _ } = self.core.get_vhead(v)?.0 {
                    let temp_var_map = fields
                        .iter()
                        .map(|(field_name, (field_type, _, _field_span))| (*field_name, *field_type))
                        .collect();

                    check_coercions(self.strings, self.core, coercions, temp_type_map, temp_var_map)?;
                } else {
                    return Err(ice().into());
                }
                v
            }
        })
    }

    fn check_statement(&mut self, def: &ast::Statement, allow_useless_exprs: bool) -> Result<()> {
        use ast::Statement::*;
        match def {
            Empty => {}
            Expr(expr) => {
                if !allow_useless_exprs {
                    use ast::Expr::*;
                    match &expr.0 {
                        Block(_) | Call(_) | FieldSet(_) | If(_) | Loop(_) | Match(_) => {}

                        _ => {
                            let mut e =
                                SpannedError::new1("SyntaxError: The value of this expression will be ignored.", expr.1);
                            e.push_str("Hint: If intentional, use let _ = ... to explicitly ignore it.");
                            return Err(e);
                        }
                    };
                }

                self.check_expr(expr, TOP)?;
            }
            Import(lhs, rhs) => {
                let v = self.imports.get(&lhs.0).ok_or_else(ice)?;
                use ast::ImportStyle::*;
                match rhs {
                    Full(name) => {
                        let mut binding = ValueBinding::new(*v, name.1);
                        binding.is_mod = true;
                        self.bindings.vars.insert(name.0, binding);
                    }
                    Fields(import_fields) => {
                        let (vhead, _) = self.core.get_vhead(*v)?;
                        if let VTypeHead::VObj { fields, aliases } = vhead {
                            for na in import_fields.iter() {
                                let name = na.name.0;
                                let mut has_import = false;

                                if let Some((v, _, _)) = fields.get(&name) {
                                    self.bindings.vars.insert(na.alias.0, ValueBinding::new(*v, na.alias.1));
                                    has_import = true;
                                } else {
                                    self.bindings.vars.hide(na.alias.0, na.alias.1);
                                }

                                if let Some(ty) = aliases.get(&name) {
                                    self.bindings.types.insert(na.alias.0, ty.clone());
                                    has_import = true;
                                } else {
                                    self.bindings.types.hide(na.alias.0, na.alias.1);
                                }

                                if !has_import {
                                    return Err(SpannedError::new1(
                                        "ImportError: Module has no value or type member with this name:",
                                        na.name.1,
                                    ));
                                }
                            }
                        } else {
                            return Err(ice().into());
                        }
                    }
                };
            }
            LetDef((pattern, var_expr)) => {
                self.check_let_def(pattern, var_expr)?;
            }
            LetRecDef(defs) => {
                self.check_let_rec_defs(defs)?;
            }
            ModuleDef(name, tyex, coercions, rhs_expr) => {
                let rhs_ty = if let Some(tyex) = tyex {
                    let (v, u) = self.parse_type_signature(tyex)?;
                    self.check_expr(rhs_expr, u)?;
                    v
                } else {
                    self.infer_expr(rhs_expr)?
                };

                // If there's already an explicit annotation, we can't suggest adding one.
                let extra_hole_src = if tyex.is_none() {
                    Some(HoleSrc::OptAscribe(name.1))
                } else {
                    None
                };

                self.check_module_binding(*name, rhs_ty, extra_hole_src, coercions)?;
            }
            NewtypeDef(def) => {
                self.bindings.types.rec_hint(def.name, def.name_span);
                let kind = def.kind();

                // Parse type RHS with old scopelvl since def is not recursive
                let old_scopelvl = self.bindings.scopelvl;
                self.bindings.scopelvl.inc();
                let tycon_ind =
                    self.core
                        .tycons
                        .add_custom(None, def.name, def.name_span, self.bindings.scopelvl, kind.0.clone());

                let mut vals = Vec::new();
                self.type_parser_at(old_scopelvl)
                    .parse_newtype_def(tycon_ind, &kind, def, &mut vals)?;

                self.bindings.types.insert(def.name, TypeBinding::Con(tycon_ind));
                for (name, span, v) in vals {
                    self.bindings.vars.insert(name, ValueBinding::new(v, span));
                }
            }
            NewtypeRecDef(defs) => {
                // Increase scope level *before* parsing RHSes since defs are mutually recursive
                self.bindings.scopelvl.inc();

                // First add all the new tycons to bindings
                let temp: Vec<_> = defs
                    .iter()
                    .map(|def| {
                        let kind = def.kind();

                        let tycon_ind = self.core.tycons.add_custom(
                            None,
                            def.name,
                            def.name_span,
                            self.bindings.scopelvl,
                            kind.0.clone(),
                        );

                        self.bindings.types.insert(def.name, TypeBinding::Con(tycon_ind));
                        (def, kind, tycon_ind)
                    })
                    .collect();

                let mut vals = Vec::new();

                // Then parse the RHSes and add the constructor functions to bindings
                for (def, kind, tycon_ind) in temp {
                    self.type_parser().parse_newtype_def(tycon_ind, &kind, def, &mut vals)?;
                }
                // Finally, add the value bindings
                for (name, span, v) in vals {
                    self.bindings.vars.insert(name, ValueBinding::new(v, span));
                }
            }
            Println(exprs) => {
                for expr in exprs {
                    self.check_expr(expr, TOP)?;
                }
            }
            TypeAlias(name, rhs) => {
                let binding = self.type_parser().parse_type_alias(rhs)?;
                self.bindings.types.insert(name.0, binding);
            }
        };
        self.core.run_pending_checks(self.strings)?;
        Ok(())
    }

    pub fn check_module_file(&mut self, id: ast::ImportId, parsed: &ast::File) -> Result<()> {
        // To compile a module, we need to create new bindings to avoid using bindings that were
        // already defined in REPL mode, but we also need to re-add the builtin types.
        let mut temp_bindings = Bindings::new_with_builtins(self.builtin_types);
        temp_bindings.scopelvl = self.bindings.scopelvl;

        let mut temp = Typeck {
            core: self.core,
            bindings: &mut temp_bindings,
            imports: self.imports,
            strings: self.strings,
            TY_BOOL: self.TY_BOOL,
            TY_FLOAT: self.TY_FLOAT,
            TY_INT: self.TY_INT,
            TY_STR: self.TY_STR,
            builtin_types: self.builtin_types,
        };
        for item in parsed.statements.0.iter() {
            temp.check_statement(item, false)?;
        }

        let new = if let Some(exports) = &parsed.exports {
            let span = exports.target.ty.1;
            let (target_type, subs) = temp.type_parser().parse_coerce_target(&exports.target, None)?;

            let mut fields = HashMap::new();
            for (name, name_span) in exports.field_keys()? {
                let v = temp.bindings.lookup_var(name, name_span)?.val;
                fields.insert(name, (v, None, name_span));
            }
            // The actual type of the exported bindings
            let lhs = temp.core.new_val(
                VTypeHead::VObj {
                    fields,
                    aliases: HashMap::new(),
                },
                span,
            );
            // Check actual against expected type.
            let cb = SubsumptionCheck::new(
                temp.core,
                temp.bindings.scopelvl,
                span,
                exports.target.hole_src_for_implicit_substitution(),
                target_type.1,
                subs,
            )?;
            let (v, u) = cb.run(temp.core, lhs)?;
            temp.core.flow(v, u, exports.target.ty.1);

            // target_type.0 is the abstracted (existential) type. Now we need to pin it by creating new types.
            let lst = temp.core.v_as_spine_or_type(target_type.0)?;
            temp.pin_existential_type((id.0, span), lst, None, &exports.coercions)?
        } else {
            // No explicit exports case: Just export all available var and type bindings.
            let mut aliases = HashMap::new();
            let mut fields = HashMap::new();
            for (name, vb) in temp.bindings.vars.iter() {
                fields.insert(*name, (vb.val, None, vb.span));
            }
            for (name, ty) in temp.bindings.types.iter() {
                aliases.insert(*name, ty.clone());
            }

            temp.core.new_val(VTypeHead::VObj { fields, aliases }, parsed.statements.1)
        };

        // Important: We need to avoid resetting scopelvl because types defined in the imported
        // file can leak through the returned bindings. Essentially, type checking behaves as if
        // the imported files are concatenated at the start, except that the bindings aren't
        // visible unless explicitly imported.
        self.bindings.scopelvl = temp.bindings.scopelvl;
        self.imports.insert(id, new);
        Ok(())
    }

    pub fn check_script(&mut self, parsed: &ast::File) -> Result<()> {
        // Tell type checker to start keeping track of changes to the type state so we can roll
        // back all the changes if the script contains an error.
        self.core.save();
        let mark = self.bindings.unwind_point();

        let len = parsed.statements.0.len();
        for (i, item) in parsed.statements.0.iter().enumerate() {
            let is_last = i == len - 1;
            if let Err(e) = self.check_statement(item, is_last) {
                // println!("num type nodes {}", self.core.num_type_nodes());

                // Roll back changes to the type state and bindings
                self.core.revert();
                self.bindings.unwind(mark);
                return Err(e);
            }
        }

        // Now that script type-checked successfully, make the global definitions permanent
        // by removing them from the changes rollback list
        self.core.make_permanent();
        self.bindings.make_permanent();
        Ok(())
    }
}
