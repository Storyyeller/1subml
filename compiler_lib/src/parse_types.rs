// Copyright (c) 2026 Robert Grosse. All rights reserved.
use crate::ast;
use crate::ast::Kind;
use crate::ast::NewtypeRHS;
use crate::ast::SKind;
use crate::ast::STypeExpr;
use crate::ast::StringId;
use crate::ast::Variance;
use crate::coercion::*;
use crate::core::*;
use crate::instantiate::ConOrTypes;
use crate::instantiate::TreeMaterializerState;
use crate::kinds::KindVar;
use crate::kinds::kinds_are_equal;
use crate::prune_unused_poly_vars::*;
use crate::spans::Span;
use crate::spans::Spanned;
use crate::spans::SpannedError as SyntaxError;
use crate::spans::ice;
use crate::spines::*;
use crate::templates::*;
use crate::type_errors::DupNameChecker;
use crate::type_errors::HoleSrc;
use crate::typeck::Bindings;
use crate::typeck::TypeBinding as GlobalBinding;
use crate::typeck::ValueBinding;
use crate::types::*;
use crate::unwindmap::UnwindMap;
use std::collections::HashMap;
use std::iter::zip;
use std::rc::Rc;

type Result<T> = std::result::Result<T, SyntaxError>;

#[derive(Debug, Clone)]
pub struct PlaceholderKinds {
    pub key: ComparisonKey,
    pub m: HashMap<StringId, KindVar>,
}
impl PlaceholderKinds {
    pub fn new(key: ComparisonKey) -> Self {
        Self { key, m: HashMap::new() }
    }

    fn make(&mut self, core: &mut TypeCheckerCore, name: StringId, span: Span) -> (ParsedTypeHead, KindVar) {
        let kind = self.m.entry(name).or_insert_with(|| KindVar::new_var(span)).clone();
        let temp = EphemeralPolyType { key: self.key, name };
        let head = ParsedTypeHead::Type(core.ephemeral(temp, span));
        (head, kind)
    }
}

struct PolyBinding {
    kind: SKind,
    loc: SourceLoc,
    names: NamePair,
    is_pair_allowed: bool,
}

struct RecBinding {
    kind: SKind,
    loc: SourceLoc,
}

enum LocalBinding {
    Poly(PolyBinding),
    Rec(RecBinding),
}

pub struct TypeParser<'a> {
    pub core: &'a mut TypeCheckerCore,
    pub strings: &'a mut lasso::Rodeo,
    bindings: &'a Bindings,
    local_types: UnwindMap<StringId, LocalBinding>,
    pub scopelvl: ScopeLvl,
    placeholder_kinds: Option<PlaceholderKinds>,

    source_loc_counter: u32,
}
impl<'a> TypeParser<'a> {
    pub fn new(
        core: &'a mut TypeCheckerCore,
        strings: &'a mut lasso::Rodeo,
        bindings: &'a Bindings,
        scopelvl: ScopeLvl,
    ) -> Self {
        Self {
            core,
            strings,
            bindings,
            local_types: UnwindMap::new(),
            scopelvl,
            placeholder_kinds: None,
            source_loc_counter: 1, // Start at 1 to reserve 0 for "no location"
        }
    }

    fn new_source_loc(&mut self) -> SourceLoc {
        let loc = SourceLoc(self.source_loc_counter);
        self.source_loc_counter += 1;
        loc
    }

    fn hole(&mut self, src: HoleSrc) -> ParsedTypeHead {
        ParsedTypeHead::Type(self.core.var(src, self.scopelvl))
    }

    fn sort_and_dedup<T>(&self, vec: &mut Vec<(StringId, (Span, T))>, msg: &'static str) -> Result<()> {
        let mut checker = DupNameChecker::new(msg);
        for (name, (span, _)) in vec.iter() {
            checker.add(*name, *span)?;
        }

        // Important - must sort by name in order to ensure deterministic ordering for spine generation.
        vec.sort_by_key(|t| self.strings.resolve(&t.0));
        Ok(())
    }

    fn parse_inv_pair2(
        &mut self,
        pair: &ast::InvTypeDecl,
        variance: Variance,
        expected_kind: &SKind,
    ) -> Result<VarianceInvPair> {
        use Variance::*;
        use VarianceInvPair::*;

        Ok(match pair {
            &ast::InvTypeDecl::Single(ref r) => {
                let r = self.parse_type_with_expected_kind(r, expected_kind)?;
                match variance {
                    Covariant => Co(r),
                    Contravariant => Contra(r),
                    Invariant => InvSingle(r),
                }
            }
            &ast::InvTypeDecl::Pair(ref r, ref w) => {
                let r = self.parse_type_with_expected_kind(r, expected_kind)?;
                let w = self.parse_type_with_expected_kind(w, expected_kind)?;
                match variance {
                    Invariant => InvPair(r, w),
                    _ => {
                        return Err(SyntaxError::new1(
                            "SyntaxError: ty1 <- ty2 syntax can only be used with invariant parameters.",
                            w.0,
                        ));
                    }
                }
            }
        })
    }

    fn parse_path_type(&mut self, path: &Spanned<ast::PathType>) -> Result<(ParsedTypeHead, KindVar)> {
        use ast::PathType::*;

        let span = path.1;
        let head = match &path.0 {
            Bot => ParsedTypeHead::Never,
            Top => ParsedTypeHead::Any,
            Hole => ParsedTypeHead::Type(self.core.var(HoleSrc::Explicit(span), self.scopelvl)),

            &Single(name) => return self.parse_path_type_path_single(name, span),
            &Placeholder(name) => {
                if let Some(ref mut m) = self.placeholder_kinds {
                    let (head, kind) = m.make(self.core, name, span);
                    return Ok((head, kind));
                } else {
                    return Err(SyntaxError::new1(
                        "SyntaxError: Substitution placeholders are only allowed in coercion expression type substitutions.",
                        span,
                    ));
                }
            }

            &ModMember(n1, n2) => {
                let mod_binding = self.bindings.lookup_var(n1.0, n1.1)?;
                return mod_binding.get_type_member(self.core, n2, n1.1, path.1);
            }

            &VarPair(p1, p2) => {
                return self
                    .parse_poly_var_pair(p1.0, p2.0)
                    .ok_or_else(|| SyntaxError::new1("SyntaxError: Illegal poly var pair.", span));
            }
        };

        let kind = KindVar::new_var(span);
        Ok((head, kind))
    }

    fn parse_poly_var_pair(&mut self, p1: StringId, p2: StringId) -> Option<(ParsedTypeHead, KindVar)> {
        let (b1, b2) = match (self.local_types.get(&p1), self.local_types.get(&p2)) {
            (Some(LocalBinding::Poly(b1)), Some(LocalBinding::Poly(b2))) => (b1, b2),
            _ => return None,
        };

        // Explicit var pairs are only allowed for function type declarations.
        if !b1.is_pair_allowed || !b2.is_pair_allowed {
            return None;
        }
        if b1.loc != b2.loc || !kinds_are_equal(&b1.kind.0, &b2.kind.0) {
            return None;
        }

        let names = match (b1.names, b2.names) {
            (NamePair::Single(n1), NamePair::Single(n2)) => NamePair::Pair(n1, n2),
            _ => return None,
        };

        Some((ParsedTypeHead::TempPolyVar(b1.loc, names), KindVar::Known(b1.kind.clone())))
    }

    fn parse_path_type_path_single(&mut self, con_name: StringId, con_span: Span) -> Result<(ParsedTypeHead, KindVar)> {
        use KindVar::Known;
        let (parsed_con, con_kind) = if let Some(binding) = self.local_types.get(&con_name) {
            match binding {
                LocalBinding::Poly(b) => (ParsedTypeHead::TempPolyVar(b.loc, b.names), Known(b.kind.clone())),
                LocalBinding::Rec(b) => (ParsedTypeHead::RecVar(b.loc), Known(b.kind.clone())),
            }
        } else {
            let binding = self.bindings.lookup_type(con_name, con_span)?;
            binding.get(self.core, con_span)?
        };

        Ok((parsed_con, con_kind))
    }

    fn parse_annotated_type_constructor(
        &mut self,
        path: &Spanned<ast::PathType>,
        annot: &Option<SKind>,
    ) -> Result<(ParsedTypeHead, KindVar)> {
        let (parsed_con, mut con_kind) = self.parse_path_type(path)?;
        if let Some(kind) = annot.as_ref() {
            con_kind.check(kind)?;
        }
        Ok((parsed_con, con_kind))
    }

    fn parse_type_with_expected_kind(&mut self, tyexpr: &ast::STypeExpr, expected_kind: &SKind) -> Result<RcParsedType> {
        let (ty, mut actual_kind) = self.parse_type_infer_kind(tyexpr)?;
        actual_kind.check(expected_kind)?;
        Ok(ty)
    }

    fn parse_type_infer_kind(&mut self, tyexpr: &ast::STypeExpr) -> Result<(RcParsedType, KindVar)> {
        let span = tyexpr.1;

        if let ast::TypeExpr::Named(pt) = &tyexpr.0 {
            if pt.args.0.is_empty() {
                let (parsed_con, kind) = self.parse_annotated_type_constructor(&pt.path, &pt.annot)?;
                return Ok((Rc::new((span, parsed_con)), kind));
            }
        } else if let ast::TypeExpr::ConstructorOf(rhs) = &tyexpr.0 {
            let inner = self.parse_type_sub(rhs)?;
            return match &inner.1 {
                ParsedTypeHead::Constructor(con, kind, ..) => Ok((con.clone(), KindVar::Known(kind.clone()))),
                // Also allow bare type nodes so that it can be used on spine constructors with no parameters.
                ParsedTypeHead::Type(..) => Ok((inner.clone(), KindVar::Known((Kind::Star, rhs.1)))),
                _ => Err(SyntaxError::new1(
                    "SyntaxError: constructor-of! can only be applied to constructor applications.",
                    rhs.1,
                )),
            };
        }

        let ty = self.parse_unnamed_type(tyexpr)?;
        Ok((ty, KindVar::Known((Kind::Star, span))))
    }

    /// Parse a path type, but require it to have kind Star even when there are no args.
    fn parse_named_type_as_star(&mut self, pt: &ast::PathTypeWithArgs) -> Result<ParsedTypeHead> {
        let (parsed_con, con_kind) = self.parse_annotated_type_constructor(&pt.path, &pt.annot)?;

        let con_span = pt.path.1;

        let con_kind = con_kind.force()?;
        let con_params = con_kind.0.params();
        if con_params.len() != pt.args.0.len() {
            return Err(SyntaxError::new2(
                format!(
                    "KindError: Expected {} type parameters, found {}.",
                    con_params.len(),
                    pt.args.0.len()
                ),
                if pt.args.0.is_empty() { con_span } else { pt.args.1 },
                format!("Note: Type constructor has {} parameters here:", con_params.len()),
                con_kind.1,
            ));
        }

        if con_params.is_empty() {
            return Ok(parsed_con);
        }

        let mut params = Vec::with_capacity(pt.args.0.len());
        for (arg, svkind) in zip(&pt.args.0, con_params.iter()) {
            params.push(self.parse_inv_pair2(arg, svkind.variance.0, &svkind.kind)?);
        }
        Ok(ParsedTypeHead::Constructor(Rc::new((con_span, parsed_con)), con_kind, params))
    }

    fn parse_unnamed_type(&mut self, tyexpr: &ast::STypeExpr) -> Result<RcParsedType> {
        use ast::TypeExpr::*;
        let span = tyexpr.1;

        let head = match &tyexpr.0 {
            Case(cases) => {
                let mut v = Vec::with_capacity(cases.len());
                for &((tag, tag_span), ref wrapped_expr) in cases {
                    let sub = if let Some(wrapped_expr) = wrapped_expr {
                        self.parse_type_sub(wrapped_expr)?
                    } else {
                        // If no type is specified for this case, treat it as "any"
                        Rc::new((tag_span, ParsedTypeHead::Any))
                    };

                    v.push((tag, (tag_span, sub)));
                }
                self.sort_and_dedup(&mut v, "variant tag")?;
                ParsedTypeHead::Case(v)
            }
            ConstructorOf(..) => return Err(ice().into()), // Already handled in parse_type_infer_kind
            Func(tparams_raw, lhs, rhs, is_id) => {
                let loc = self.new_source_loc();

                let mark = self.local_types.unwind_point();

                for tp in tparams_raw.iter() {
                    let binding = LocalBinding::Poly(PolyBinding {
                        kind: tp.kind.clone(),
                        loc,
                        names: NamePair::Single(tp.name.0),
                        is_pair_allowed: true,
                    });
                    self.local_types.insert(tp.alias.0, binding);
                }

                let lhs = self.parse_type_sub(lhs)?;
                let rhs = self.parse_type_sub(rhs)?;
                let prop = FuncProperties { is_identity: *is_id };

                self.local_types.unwind(mark);
                let head = ParsedTypeHead::Func(lhs, rhs, prop);

                let tparams = SortedTypeParams::from(tparams_raw, self.strings)?;
                let ty = finish_polymorphic_type(self.core, self.strings, loc, Rc::new((span, head)), tparams)?;
                let ty = Rc::try_unwrap(ty).unwrap();
                ty.1
            }
            Named(pt) => self.parse_named_type_as_star(pt)?,
            Record(fields) => {
                let loc = self.new_source_loc();
                let mark = self.local_types.unwind_point();

                let mut parsed_fields = Vec::new();
                let mut parsed_aliases = Vec::new();
                let mut tparams = TypeParamsBuilder::new();
                for &((name, name_span), ref type_decl) in fields {
                    use ast::RecordTypeMember::*;

                    match type_decl {
                        Field(is_mut, field_decl) => {
                            let variance = if *is_mut { Variance::Invariant } else { Variance::Covariant };
                            let pair = self.parse_inv_pair2(field_decl, variance, &(Kind::Star, name_span))?;
                            parsed_fields.push((name, (name_span, pair)));
                        }
                        Abstract(tp) => {
                            tparams.add(tp)?;

                            let binding = LocalBinding::Poly(PolyBinding {
                                kind: tp.kind.clone(),
                                loc,
                                names: NamePair::Single(tp.name.0),
                                is_pair_allowed: false,
                            });
                            self.local_types.insert(tp.alias.0, binding);
                        }
                        Alias(ty) => {
                            let (ty, kind) = self.parse_type_infer_kind(ty)?;
                            parsed_aliases.push((name, (name_span, (ty, kind))));
                        }
                    }
                }
                self.local_types.unwind(mark);
                self.sort_and_dedup(&mut parsed_fields, "record type field")?;
                self.sort_and_dedup(&mut parsed_aliases, "record alias member")?;
                let head = ParsedTypeHead::Record(parsed_fields, parsed_aliases, Vec::new());

                let tparams = tparams.finish(self.strings);
                let ty = finish_polymorphic_type(self.core, self.strings, loc, Rc::new((span, head)), tparams)?;
                let ty = Rc::try_unwrap(ty).unwrap();
                ty.1
            }
            &RecursiveDef(name, ref def) => {
                let loc = self.new_source_loc();
                let binding = LocalBinding::Rec(RecBinding {
                    kind: (Kind::Star, span),
                    loc,
                });

                let mark = self.local_types.unwind_point();
                self.local_types.insert(name, binding);
                let sub = self.parse_type_sub(def)?;
                self.local_types.unwind(mark);

                use RecUsageResult::*;
                let rec_contravariantly = match get_rec_head_usage(&sub, loc) {
                    // If recursive type does not actually reference itself, no need for RecHead
                    Unused => return Ok(sub),
                    Covariant => false,
                    Contravariant => true,
                };

                use ParsedTypeHead::*;
                match &sub.1 {
                    Case(..) | Func(..) | Constructor(..) | Record(..) => ParsedTypeHead::RecHead(RecHeadData {
                        loc,
                        rec_contravariantly,
                        body: sub,
                    }),
                    _ => {
                        // This should be unreachable due to the early return in the unused case above.
                        return Err(SyntaxError::new1(
                            "SyntaxError: Recursive types must be defined as a constructor application, function, record, or variant.",
                            sub.0,
                        ));
                    }
                }
            }
        };

        Ok(Rc::new((span, head)))
    }
    pub fn parse_type_sub(&mut self, tyexpr: &ast::STypeExpr) -> Result<RcParsedType> {
        self.parse_type_with_expected_kind(tyexpr, &(Kind::Star, tyexpr.1))
    }

    fn parse_type_or_hole_sub(&mut self, tyexpr: Option<&ast::STypeExpr>, span: Span, src: HoleSrc) -> Result<RcParsedType> {
        tyexpr
            .map(|tyexpr| self.parse_type_sub(tyexpr))
            .unwrap_or_else(|| Ok(Rc::new((span, self.hole(src)))))
    }

    ////////////////////////////////////////////////////////////////////////////////////////
    pub fn parse_type(&mut self, tyexpr: &ast::STypeExpr) -> Result<(Value, Use)> {
        let parsed = self.parse_type_sub(tyexpr)?;
        Ok(TreeMaterializerState::new().with(self.core).materialize_pair(&parsed)?)
    }

    pub fn parse_named_type(&mut self, pt: &ast::PathTypeWithArgs, span: Span) -> Result<(Value, Use)> {
        let parsed = self.parse_named_type_as_star(pt)?;
        Ok(TreeMaterializerState::new()
            .with(self.core)
            .materialize_pair(&(span, parsed))?)
    }

    pub fn parse_type_or_hole(&mut self, tyexpr: Option<&ast::STypeExpr>, span: Span, src: HoleSrc) -> Result<(Value, Use)> {
        let parsed = self.parse_type_or_hole_sub(tyexpr, span, src)?;
        Ok(TreeMaterializerState::new().with(self.core).materialize_pair(&parsed)?)
    }

    pub fn parse_type_alias(&mut self, tyexpr: &ast::STypeExpr) -> Result<GlobalBinding> {
        let (parsed, kind) = self.parse_type_infer_kind(tyexpr)?;
        let p = TreeMaterializerState::new().with(self.core).materialize_pair(&parsed)?;
        let binding = GlobalBinding::inspect(self.core, p, &kind)?;
        Ok(binding)
    }

    pub fn parse_coerce_target(
        mut self,
        target: &ast::expr::CoerceTarget,
        comparison_key: Option<ComparisonKey>,
    ) -> Result<((Value, Use), ParsedTypeSubstitutions)> {
        let target_ty = self.parse_type(&target.ty)?;

        // Now parse the substitutions
        self.placeholder_kinds = comparison_key.map(PlaceholderKinds::new);

        let mut dup = DupNameChecker::new("type substitution");
        let mut parsed_subs = HashMap::new();
        for &((name, span), ref sub) in target.substitutions.iter() {
            dup.add(name, span)?;
            let (parsed, kind) = self.parse_type_infer_kind(sub)?;
            let pair = TreeMaterializerState::new().with(self.core).materialize_pair(&parsed)?;
            parsed_subs.insert(name, (pair, kind));
        }

        let subs = ParsedTypeSubstitutions {
            subs: parsed_subs,
            placeholder_kinds: self.placeholder_kinds.take(),
        };
        Ok((target_ty, subs))
    }
}

#[derive(Debug, Clone)]
pub struct ParsedTypeSubstitutions {
    pub subs: HashMap<StringId, ((Value, Use), KindVar)>,
    pub placeholder_kinds: Option<PlaceholderKinds>,
}

#[derive(Default)]
struct PatternBindings {
    vars: HashMap<StringId, (Span, RcParsedType)>,
}
impl PatternBindings {
    fn insert_var(&mut self, name: StringId, span: Span, ty: RcParsedType) -> Result<()> {
        if let Some((old_span, _)) = self.vars.insert(name, (span, ty)) {
            Err(SyntaxError::new2(
                "SyntaxError: Repeated variable binding in pattern",
                span,
                "Note: Name was already bound here:",
                old_span,
            ))
        } else {
            Ok(())
        }
    }
}

impl<'a> TypeParser<'a> {
    fn parse_func_arg_pattern(&mut self, pat: &ast::LetPattern, out: &mut PatternBindings) -> Result<RcParsedType> {
        use ast::LetPattern::*;

        Ok(match pat {
            &Var(ast::VarPattern((name, span), ref tyexpr)) => {
                let ty = self.parse_type_or_hole_sub(tyexpr.as_ref(), span, HoleSrc::FuncArgAscribe(span))?;
                if let Some(name) = name {
                    out.insert_var(name, span, ty.clone())?;
                }
                ty
            }

            &Case(ref coercion_hint, (tag, span), ref val_pat) => {
                if let Some((_, span)) = coercion_hint {
                    return Err(SyntaxError::new1(
                        "SyntaxError: Coercions are not allowed in function argument patterns.",
                        *span,
                    ));
                }

                let sub = if let Some(val_pat) = val_pat {
                    self.parse_func_arg_pattern(val_pat, out)?
                } else {
                    // If no pattern is specified for this case, treat it as "any"
                    Rc::new((span, ParsedTypeHead::Any))
                };

                // No need for sorting or dup checking since only one case
                let v = vec![(tag, (span, sub))];

                Rc::new((span, ParsedTypeHead::Case(v)))
            }
            &Record(ref coercion_hint, (ref pairs, span), ref as_pat) => {
                if let Some((_, span)) = coercion_hint {
                    return Err(SyntaxError::new1(
                        "SyntaxError: Coercions are not allowed in function argument patterns.",
                        *span,
                    ));
                }
                if let Some(vp) = as_pat {
                    return Err(SyntaxError::new1(
                        "SyntaxError: 'as' patterns are not allowed in function argument patterns.",
                        vp.0.1,
                    ));
                }

                let mut fields = Vec::new();
                for &((name, name_span), ref field) in pairs {
                    use ast::RecordPatternMember::*;
                    match field {
                        Field(sub_pattern) => {
                            let sub = self.parse_func_arg_pattern(sub_pattern, out)?;
                            fields.push((name, (name_span, VarianceInvPair::Co(sub))));
                        }
                    }
                }
                self.sort_and_dedup(&mut fields, "record pattern field")?;
                Rc::new((span, ParsedTypeHead::Record(fields, Vec::new(), Vec::new())))
            }
        })
    }

    pub fn parse_func_sig(&mut self, e: &ast::expr::FuncDefExpr, span: Span) -> Result<ParsedFuncSig> {
        let ty_params = &e.type_params;
        let arg_pat = &e.param;
        let ret_type = e.return_type.as_ref();

        let (arg_pat, arg_pat_span) = (&arg_pat.0, arg_pat.1);

        // The abstract types generated for the function's type parameters
        // are only visible within the function body, so scopelvl needs to be 1 higher.
        let scopelvl_for_types = self.scopelvl.incremented();

        let loc = self.new_source_loc();

        let ty_params = ty_params.as_ref().map(|v| &v[..]).unwrap_or_default();

        let mut new_abstract_types = Vec::with_capacity(ty_params.len());
        let mut replacement_map = HashMap::with_capacity(ty_params.len());

        for tp in ty_params.iter() {
            let binding = LocalBinding::Poly(PolyBinding {
                kind: tp.kind.clone(),
                loc,
                names: NamePair::Single(tp.name.0),
                is_pair_allowed: false,
            });
            self.local_types.insert(tp.alias.0, binding);

            let tycon_ind = self
                .core
                .tycons
                .add_custom(None, tp.alias.0, tp.alias.1, scopelvl_for_types, tp.kind.0.clone());
            new_abstract_types.push((tp.alias.0, tycon_ind));
            replacement_map.insert((loc, tp.name.0), ConOrTypes::Con(tycon_ind));
        }

        // Parse argument pattern and return type
        let mut out = PatternBindings::default();
        let arg_bound = self.parse_func_arg_pattern(arg_pat, &mut out)?;
        let ret_type = self.parse_type_or_hole_sub(ret_type, arg_pat_span, HoleSrc::ReturnTypeAscribe(arg_pat_span))?;

        let head = ParsedTypeHead::Func(arg_bound, ret_type.clone(), FuncProperties::default());
        let tparams = SortedTypeParams::from(ty_params, self.strings)?;
        let ty = finish_polymorphic_type(self.core, self.strings, loc, Rc::new((span, head)), tparams)?;

        let mut state = TreeMaterializerState::new();
        let mut mat = state.with(self.core);

        let func_type = mat.materialize_val(&ty)?;

        // Set up replacement map for type parameters
        // These should never be referenced by func_type (as they'll be replaced by a
        // spine constructor in func_type), so add them after materializing func_type.
        mat.s.temp_poly_var_replacements = replacement_map;
        let ret_bound = mat.materialize_use(&ret_type)?;

        let mut vars = Vec::with_capacity(out.vars.len());
        let mut var_type_map = HashMap::with_capacity(out.vars.len());
        for (&name, &(span, ref ty)) in out.vars.iter() {
            let v = mat.materialize_val(ty)?;
            vars.push((name, ValueBinding::new(v, span)));
            var_type_map.insert(name, v);
        }

        check_coercions(
            self.strings,
            self.core,
            &e.coercions,
            new_abstract_types.iter().copied().collect(),
            var_type_map,
        )?;

        let bindings = ParsedBindings {
            vars,
            types: new_abstract_types,
        };

        Ok(ParsedFuncSig {
            bindings,
            func_type,
            ret_bound,
        })
    }

    pub fn parse_newtype_def(
        &mut self,
        tycon_ind: TyConDefInd,
        kind: &SKind,
        def: &ast::NewtypeDef,
        out: &mut Vec<(StringId, Span, Value)>,
    ) -> Result<()> {
        let loc = self.new_source_loc();
        let span = def.name_span;

        let lhs_construtor_node = Rc::new((span, ParsedTypeHead::Type(self.core.simple_restamp(tycon_ind, span))));
        let mut lhs_params_nodes = Vec::new();
        let mut tparams = TypeParamsBuilder::new();
        for p in def.params.iter() {
            let tp = &p.tp;
            tparams.add(tp)?;

            let names = if let Some(pair_name) = tp.pair_name {
                NamePair::Pair(tp.name.0, pair_name)
            } else {
                NamePair::Single(tp.name.0)
            };
            let binding = LocalBinding::Poly(PolyBinding {
                kind: tp.kind.clone(),
                loc,
                names,
                is_pair_allowed: false,
            });
            self.local_types.insert(tp.alias.0, binding);

            let param_node = Rc::new((p.tp.name.1, ParsedTypeHead::TempPolyVar(loc, names.flip())));
            lhs_params_nodes.push(match p.variance.0 {
                Variance::Covariant => VarianceInvPair::Co(param_node),
                Variance::Contravariant => VarianceInvPair::Contra(param_node),
                Variance::Invariant => VarianceInvPair::InvPair(param_node.clone(), param_node),
            });
        }
        let tparams = tparams.finish(self.strings);

        let lhs = if lhs_params_nodes.is_empty() {
            lhs_construtor_node.clone()
        } else {
            Rc::new((
                span,
                ParsedTypeHead::Constructor(lhs_construtor_node.clone(), kind.clone(), lhs_params_nodes),
            ))
        };

        let full_rhs = match &def.rhs {
            NewtypeRHS::Type(body) => self.parse_and_check_newtype_rhs(def, loc, body)?,
            NewtypeRHS::Enum(ctors) => {
                // Create version of lhs with any/never for params, in case there's a no-arg constructor
                let mut lhs_params_nodes_unused = Vec::new();
                for p in def.params.iter() {
                    let any_node = Rc::new((p.tp.name.1, ParsedTypeHead::Any));
                    let never_node = Rc::new((p.tp.name.1, ParsedTypeHead::Never));
                    lhs_params_nodes_unused.push(match p.variance.0 {
                        Variance::Covariant => VarianceInvPair::Co(never_node),
                        Variance::Contravariant => VarianceInvPair::Contra(any_node),
                        Variance::Invariant => VarianceInvPair::InvPair(never_node, any_node),
                    });
                }
                let lhs_unused = if lhs_params_nodes_unused.is_empty() {
                    lhs_construtor_node.clone()
                } else {
                    Rc::new((
                        span,
                        ParsedTypeHead::Constructor(lhs_construtor_node.clone(), kind.clone(), lhs_params_nodes_unused),
                    ))
                };
                let mut state = TreeMaterializerState::new();
                let lhs_unused = state.with(self.core).materialize_val(&lhs_unused)?;

                // Now process the actual constructors.
                let mut cases = Vec::with_capacity(ctors.len());
                for (name, body) in ctors.iter() {
                    if let Some(body) = body {
                        let rhs = self.parse_and_check_newtype_rhs(def, loc, body)?;
                        let tree = Rc::new((
                            name.1,
                            ParsedTypeHead::Func(rhs.clone(), lhs.clone(), FuncProperties::default()),
                        ));
                        let tree = finish_polymorphic_type(self.core, self.strings, loc, tree, tparams.clone())?;

                        let mut state = TreeMaterializerState::new();
                        let v = state.with(self.core).materialize_val(&tree)?;
                        out.push((name.0, name.1, v));

                        cases.push((name.0, (name.1, rhs)));
                    } else {
                        out.push((name.0, name.1, lhs_unused));
                        cases.push((name.0, (name.1, Rc::new((name.1, ParsedTypeHead::Any)))));
                    }
                }

                self.sort_and_dedup(&mut cases, "variant constructor")?;
                Rc::new((span, ParsedTypeHead::Case(cases)))
            }
        };

        // Now create the coercion types
        // Wrapper function: rhs -> lhs
        let prop = FuncProperties { is_identity: true };
        let tree1 = Rc::new((span, ParsedTypeHead::Func(full_rhs.clone(), lhs.clone(), prop)));
        // Unwrapper function: lhs -> rhs
        let tree2 = Rc::new((span, ParsedTypeHead::Func(lhs, full_rhs, prop)));

        let tree1 = finish_polymorphic_type(self.core, self.strings, loc, tree1, tparams.clone())?;
        let tree2 = finish_polymorphic_type(self.core, self.strings, loc, tree2, tparams)?;

        let mut state = TreeMaterializerState::new();
        let val1 = state.with(self.core).materialize_val(&tree1)?;
        let mut state = TreeMaterializerState::new();
        let val2 = state.with(self.core).materialize_val(&tree2)?;

        // Newtype def generated coercions should always be correct, so map errrors to ICE
        let to_ice = |e| if let InvalidCoercionError::ICE(e) = e { e } else { ice() };
        parse_and_register_coercion(self.core, tycon_ind, val1, span, false, true).map_err(to_ice)?;
        parse_and_register_coercion(self.core, tycon_ind, val2, span, true, true).map_err(to_ice)?;

        out.push((def.name, def.name_span, val1));
        out.push((def.name2, def.name_span, val2));

        Ok(())
    }

    /// Check that type parameters are used in positions consistent with their declared variance   
    fn parse_and_check_newtype_rhs(
        &mut self,
        def: &ast::NewtypeDef,
        loc: SourceLoc,
        rhs: &STypeExpr,
    ) -> Result<RcParsedType> {
        let rhs = self.parse_type_sub(rhs)?;
        let var_usage = PolyVarUses::new(Some(loc)).walk(&rhs);
        for param in def.params.iter() {
            use Variance::*;
            let (declared, dec_span) = param.variance;
            let name = param.tp.name.0;
            if let Some(use_span) = var_usage.get_contra(name)
                && declared == Covariant
            {
                return Err(SyntaxError::new2(
                    format!(
                        "SyntaxError: Type parameter '{}' is used contravariantly here:",
                        self.strings.resolve(&name),
                    ),
                    use_span,
                    "Note: But it is declared as covariant here:",
                    dec_span,
                ));
            }

            if let Some(use_span) = var_usage.get_co(name)
                && declared == Contravariant
            {
                return Err(SyntaxError::new2(
                    format!(
                        "SyntaxError: Type parameter '{}' is used covariantly here:",
                        self.strings.resolve(&name),
                    ),
                    use_span,
                    "Note: But it is declared as contravariant here:",
                    dec_span,
                ));
            }
        }
        Ok(rhs)
    }
}

#[derive(Debug, Clone)]
pub struct ParsedBindings {
    // Names already checked for duplicates
    pub vars: Vec<(StringId, ValueBinding)>,
    pub types: Vec<(StringId, TyConDefInd)>,
}

pub struct ParsedFuncSig {
    pub bindings: ParsedBindings,

    pub func_type: Value,
    pub ret_bound: Use,
}

pub fn check_coercions(
    strings: &lasso::Rodeo,
    core: &mut TypeCheckerCore,
    coercions: &ast::ImplicitCoercions,
    types: HashMap<StringId, TyConDefInd>,
    var_types: HashMap<StringId, Value>,
) -> Result<()> {
    let mut checker = DupNameChecker::new("implicit coercion");
    for (name, c1, c2) in coercions.0.iter() {
        checker.add(name.0, name.1)?;
        let tycon = types.get(&name.0).ok_or_else(|| {
            SyntaxError::new1(
                format!(
                    "SyntaxError: No newly defined type constructor '{}'",
                    strings.resolve(&name.0)
                ),
                name.1,
            )
        })?;

        if let Some(c1) = c1 {
            let v = var_types.get(&c1.0).ok_or_else(|| {
                SyntaxError::new1(
                    format!("SyntaxError: No newly defined variable named '{}'", strings.resolve(&c1.0)),
                    c1.1,
                )
            })?;
            parse_and_register_coercion(core, *tycon, *v, c1.1, true, false).map_err(|e| e.spanned(c1.1))?;
        }

        if let Some(c2) = c2 {
            let v = var_types.get(&c2.0).ok_or_else(|| {
                SyntaxError::new1(
                    format!("SyntaxError: No newly defined variable named '{}'", strings.resolve(&c2.0)),
                    c2.1,
                )
            })?;
            parse_and_register_coercion(core, *tycon, *v, c2.1, false, false).map_err(|e| e.spanned(c2.1))?;
        }
    }
    Ok(())
}
