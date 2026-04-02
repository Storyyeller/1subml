// Copyright (c) 2026 Robert Grosse. All rights reserved.
use std::collections::HashMap;
use std::mem::swap;

use crate::ast;
use crate::ast::*;
use crate::js;
use crate::ordered_map::OrderedMap;
use crate::spans::*;
use crate::unwindmap::UnwindMap;

const TAG_FIELD: &str = "_";
const VAL_FIELD: &str = "$";

struct ModuleBindings {
    all: js::Expr,
    vars: HashMap<StringId, js::Expr>,
}

pub struct Compiler {
    scope_var_name: String, // name of JS var used to store variables in the current scope
    scope_counter: u64,
    param_counter: u64,
    // For choosing new var names
    var_counter: u64,
    // ML name -> JS expr for current scope
    bindings: UnwindMap<StringId, js::Expr>,
    // Imported modules that haven't been compiled yet
    pub pending_imports: Vec<(ImportId, ast::File)>,
    imported: HashMap<ImportId, ModuleBindings>,
}
impl Compiler {
    pub fn new() -> Self {
        Self {
            scope_var_name: "$".to_string(),
            scope_counter: 0,
            param_counter: 0,
            var_counter: 0,
            bindings: UnwindMap::new(),
            pending_imports: Vec::new(),
            imported: HashMap::new(),
        }
    }

    fn set_binding(&mut self, k: StringId, v: js::Expr) {
        self.bindings.insert(k, v);
    }

    fn new_var_name(&mut self) -> String {
        let js_name = format!("v{}", self.var_counter);
        self.var_counter += 1;
        js_name
    }

    fn new_temp_var_assign(&mut self, mut rhs: js::Expr, out: &mut Vec<js::Expr>) -> js::Expr {
        if rhs.try_inline(out) {
            return rhs;
        }

        let js_name = self.new_var_name();

        let expr = js::scope_field(&self.scope_var_name, &js_name);
        out.push(js::assign(expr.clone(), rhs, false));
        expr
    }

    fn new_var(&mut self, ml_name: StringId) -> js::Expr {
        let js_name = self.new_var_name();
        let expr = js::scope_field(&self.scope_var_name, &js_name);
        self.set_binding(ml_name, expr.clone());
        expr
    }

    fn new_var_assign(&mut self, ml_name: StringId, rhs: js::Expr, out: &mut Vec<js::Expr>) -> js::Expr {
        let expr = self.new_temp_var_assign(rhs, out);
        self.set_binding(ml_name, expr.clone());
        expr
    }

    fn new_scope_name(&mut self) -> String {
        let js_name = format!("s{}", self.scope_counter);
        self.scope_counter += 1;
        js_name
    }

    fn new_param_name(&mut self) -> String {
        let js_name = format!("p{}", self.param_counter);
        self.param_counter += 1;
        js_name
    }
}
pub struct Context<'a>(pub &'a mut Compiler, pub &'a lasso::Rodeo);
impl<'a> Context<'a> {
    fn ml_scope<T>(&mut self, cb: impl FnOnce(&mut Self) -> T) -> T {
        let n = self.bindings.unwind_point();
        let res = cb(self);
        self.bindings.unwind(n);
        res
    }

    fn fn_scope<T>(&mut self, cb: impl FnOnce(&mut Self) -> T) -> T {
        let old_var_counter = self.var_counter;
        let old_param_counter = self.param_counter;
        let old_scope_counter = self.scope_counter;
        self.var_counter = 0;

        let res = self.ml_scope(cb);

        self.var_counter = old_var_counter;
        self.param_counter = old_param_counter;
        self.scope_counter = old_scope_counter;
        res
    }

    fn get(&self, id: StringId) -> &'a str {
        self.1.resolve(&id)
    }

    fn get_new(&self, id: StringId) -> String {
        self.1.resolve(&id).to_owned()
    }
}
impl<'a> core::ops::Deref for Context<'a> {
    type Target = Compiler;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}
impl<'a> core::ops::DerefMut for Context<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
    }
}

fn case_obj(ctx: &mut Context<'_>, tag: StringId, expr: js::Expr) -> js::Expr {
    let tag = js::lit(format!("\"{}\"", ctx.get(tag)));
    js::obj(vec![(TAG_FIELD.to_string(), tag), (VAL_FIELD.to_string(), expr)])
}

fn compile(ctx: &mut Context<'_>, expr: &ast::SExpr) -> Result<js::Expr, ICE> {
    Ok(match &expr.0 {
        ast::Expr::BinOp(e) => {
            let lhs = compile(ctx, &e.lhs)?;
            let rhs = compile(ctx, &e.rhs)?;
            let jsop = match e.op {
                ast::expr::Op::Add => js::Op::Add,
                ast::expr::Op::Sub => js::Op::Sub,
                ast::expr::Op::Mult => js::Op::Mult,
                ast::expr::Op::Div => js::Op::Div,
                ast::expr::Op::Rem => js::Op::Rem,

                ast::expr::Op::Lt => js::Op::Lt,
                ast::expr::Op::Lte => js::Op::Lte,
                ast::expr::Op::Gt => js::Op::Gt,
                ast::expr::Op::Gte => js::Op::Gte,

                ast::expr::Op::Eq => js::Op::Eq,
                ast::expr::Op::Neq => js::Op::Neq,

                ast::expr::Op::BoolAnd => js::Op::BoolAnd,
                ast::expr::Op::BoolOr => js::Op::BoolOr,
            };
            js::binop(lhs, rhs, jsop)
        }
        ast::Expr::Block(e) => {
            ctx.ml_scope(|ctx| {
                let mut exprs = Vec::new(); // a list of assignments, followed by rest

                for stmt in &e.statements {
                    compile_statement(ctx, &mut exprs, stmt)?;
                }

                if let Some(e) = &e.expr {
                    exprs.push(compile(ctx, e)?);
                } else {
                    exprs.push(js::void());
                }
                Ok(js::comma_list(exprs))
            })?
        }
        ast::Expr::Call(e) => {
            if e.eval_arg_first {
                let mut exprs = Vec::new();
                let arg = compile(ctx, &e.arg)?;
                let arg = ctx.new_temp_var_assign(arg, &mut exprs);
                let func = compile(ctx, &e.func)?;
                exprs.push(js::call(func, arg));
                js::comma_list(exprs)
            } else {
                let func = compile(ctx, &e.func)?;
                let arg = compile(ctx, &e.arg)?;
                // Optimization: Skip calling identity functions.
                if func.is_var("id") { arg } else { js::call(func, arg) }
            }
        }
        ast::Expr::Case(e) => {
            let expr = compile(ctx, &e.expr)?;
            case_obj(ctx, e.tag.0, expr)
        }
        ast::Expr::Coerce(e) => compile(ctx, &e.expr)?,
        ast::Expr::FieldAccess(e) => {
            let lhs = compile(ctx, &e.expr)?;
            js::field(lhs, ctx.get_new(e.field.0))
        }
        ast::Expr::FieldSet(e) => {
            let mut exprs = Vec::new();

            let lhs_compiled = compile(ctx, &e.expr)?;
            let lhs_temp_var = ctx.new_temp_var_assign(lhs_compiled, &mut exprs);
            let lhs = js::field(lhs_temp_var, ctx.get_new(e.field.0));

            let res_temp_var = ctx.new_temp_var_assign(lhs.clone(), &mut exprs);
            exprs.push(js::assign(lhs.clone(), compile(ctx, &e.value)?, false));
            exprs.push(res_temp_var);

            js::comma_list(exprs)
        }
        ast::Expr::FuncDef(e) => {
            ctx.fn_scope(|ctx| {
                let mut new_scope_name = ctx.new_scope_name();
                swap(&mut new_scope_name, &mut ctx.scope_var_name);

                //////////////////////////////////////////////////////
                let js_arg = js::var(ctx.new_param_name());
                let mut exprs = Vec::new();
                compile_let_pattern_flat(ctx, &mut exprs, &e.param.0, js_arg.clone())?;

                exprs.push(compile(ctx, &e.body)?);
                let body = js::comma_list(exprs);
                //////////////////////////////////////////////////////

                swap(&mut new_scope_name, &mut ctx.scope_var_name);
                Ok(js::func(js_arg, new_scope_name, body))
            })?
        }
        ast::Expr::Identity(..) => js::var("id".to_string()),
        ast::Expr::If(e) => {
            let cond_expr = compile(ctx, &e.cond.0)?;
            let then_expr = compile(ctx, &e.then_expr)?;

            let else_expr = if let Some(else_expr) = &e.else_expr {
                compile(ctx, else_expr)?
            } else {
                js::void()
            };
            js::ternary(cond_expr, then_expr, else_expr)
        }
        ast::Expr::Literal(e) => {
            let mut code = e.value.0.clone();
            if let ast::expr::Literal::Int = e.lit_type {
                code.push_str("n");
            }
            if let Some(code) = code.strip_prefix('-') {
                js::unary_minus(js::lit(code.to_string()))
            } else {
                js::lit(code)
            }
        }
        ast::Expr::Loop(e) => {
            let lhs = js::var("loop".to_string());
            let rhs = compile(ctx, &e.body)?;
            let rhs = js::func(js::var("_".to_string()), "_2".to_string(), rhs);
            js::call(lhs, rhs)
        }
        ast::Expr::Match(e) => {
            let match_compiled = compile(ctx, &e.expr.0)?;
            // Generate merged pattern, and assign all matched values to temp vars
            let mut p = PatternTree::new(ctx, match_compiled);

            struct ProcessedCase<'a> {
                bindings: Vec<(StringId, js::Expr)>,
                conditions: Vec<(js::Expr, StringId)>,
                guard: &'a Option<Box<ast::SExpr>>,
            }

            let mut processed = Vec::new();
            for arm in &e.arms {
                let mut cases = Vec::new();
                for case in &arm.cases {
                    let mut bindings = Vec::new();
                    let mut conditions = Vec::new();
                    p.add_pattern(&case.pattern.0, ctx, &mut bindings, Some(&mut conditions));
                    cases.push(ProcessedCase {
                        bindings,
                        conditions,
                        guard: &case.guard,
                    });
                }
                processed.push((cases, &arm.expr));
            }

            let mut exprs = Vec::new();
            p.get_assigns(ctx.1, &mut exprs)?;

            // Now generate the actual match expression part (not counting the pre-assignments)
            let mut branches = Vec::new();
            for (cases, rhs_expr) in processed {
                // First check whether any bindings are different for different cases
                let mut binding_lists = HashMap::new();
                for case in cases.iter() {
                    for (name, expr) in case.bindings.iter() {
                        binding_lists.entry(*name).or_insert_with(Vec::new).push(expr);
                    }
                }
                // Remove bindings which aren't defined in all cases.
                binding_lists.retain(|_, lists| lists.len() == cases.len());
                let mut merged_bindings = Vec::new();
                let mut bindings_to_assign = HashMap::new();
                for (name, lists) in binding_lists.into_iter() {
                    let first = lists[0];
                    if lists.iter().all(|e| e == &first) {
                        merged_bindings.push((name, first.clone()));
                    } else {
                        // If the binding is different across cases, we need to assign it to a temp var before compiling the arm body.
                        let js_name = ctx.new_var_name();
                        let temp_var = js::scope_field(&ctx.scope_var_name, &js_name);

                        merged_bindings.push((name, temp_var.clone()));
                        bindings_to_assign.insert(name, temp_var);
                    }
                }

                // Now compile each case
                let mut compiled_cases = Vec::new();
                for case in cases {
                    let mut conditions = case
                        .conditions
                        .into_iter()
                        .map(|(cond_expr, tag)| js::eqop(cond_expr, js::lit(format!("\"{}\"", ctx.get(tag)))))
                        .collect::<Vec<_>>();

                    let mut extra_assigns = case
                        .bindings
                        .iter()
                        .filter_map(|(name, expr)| {
                            if let Some(temp_var) = bindings_to_assign.get(name) {
                                Some(js::assign(temp_var.clone(), expr.clone(), false))
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>();

                    // Compile the guard expression, and add it to conditions if it exists
                    // Note that the guard is compiled with the per-case bindings, not the merged bindings.
                    if let Some(guard_expr) = case.guard.as_ref() {
                        conditions.push(ctx.ml_scope(|ctx| {
                            for (ml_name, js_expr) in case.bindings {
                                ctx.set_binding(ml_name, js_expr);
                            }
                            compile(ctx, guard_expr)
                        })?);
                    }

                    // If the tag checks and guard expression pass, perform the assignments to merged vars, if necessary
                    if !extra_assigns.is_empty() {
                        extra_assigns.push(js::lit("true".to_string()));
                        conditions.push(js::comma_list(extra_assigns));
                    }

                    compiled_cases.push(js::logical_and(conditions));
                }

                // Finally, || together each compiled case, and compile the arm RHS
                ctx.ml_scope(|ctx| {
                    for (ml_name, js_expr) in merged_bindings {
                        ctx.set_binding(ml_name, js_expr);
                    }

                    let rhs = compile(ctx, rhs_expr)?;
                    branches.push((js::logical_or(compiled_cases), rhs));
                    Ok(())
                })?;
            }

            let last = branches.pop().ok_or_else(ice)?;
            let mut res = js::comma_list(vec![last.0, last.1]);
            while let Some((cond_expr, rhs_expr)) = branches.pop() {
                res = js::ternary(cond_expr, rhs_expr, res);
            }

            exprs.push(res);
            js::comma_list(exprs)
        }
        ast::Expr::Record(e) => {
            let mut fields = Vec::new();
            for ((name, _), member) in e.fields.iter() {
                match member {
                    ast::expr::RecordExprMember::Field(_, expr, _) => fields.push((ctx.get_new(*name), compile(ctx, expr)?)),
                    ast::expr::RecordExprMember::Alias(..) => {}
                }
            }
            js::obj(fields)
        }
        ast::Expr::Typed(e) => compile(ctx, &e.expr)?,
        ast::Expr::Variable(e) => ctx.bindings.get(&e.name).ok_or_else(ice)?.clone(),
        ast::Expr::Unsafe(s) => js::unsafe_(s.clone()),
    })
}

struct PatternTree {
    extra_assigns: Vec<js::Expr>,
    root: js::Expr,

    root_val: Option<js::Expr>, // cached temp var binding for root.$val

    // Use OrderedMap to ensure deterministic output
    fields: OrderedMap<StringId, Box<PatternTree>>,
    unconditional: Option<Box<PatternTree>>,
    conditional: OrderedMap<StringId, Box<PatternTree>>,
}
impl PatternTree {
    fn new(ctx: &mut Context<'_>, rhs: js::Expr) -> Self {
        let mut extra_assigns = Vec::new();
        let root = ctx.new_temp_var_assign(rhs, &mut extra_assigns);
        Self {
            extra_assigns,
            root,
            root_val: None,
            fields: OrderedMap::new(),
            unconditional: None,
            conditional: OrderedMap::new(),
        }
    }

    fn get_root_val_expr(&mut self, ctx: &mut Context<'_>) -> js::Expr {
        self.root_val
            .get_or_insert_with(|| {
                let val_expr = js::field(self.root.clone(), VAL_FIELD.to_string());
                ctx.new_temp_var_assign(val_expr, &mut self.extra_assigns)
            })
            .clone()
    }

    fn add_pattern(
        &mut self,
        pat: &ast::LetPattern,
        ctx: &mut Context<'_>,
        bindings_out: &mut Vec<(StringId, js::Expr)>,
        mut conditions_out: Option<&mut Vec<(js::Expr, StringId)>>,
    ) {
        use ast::LetPattern::*;
        match pat {
            Case(_, (tag, _), sub_pattern) => {
                if let Some(conditions_out) = conditions_out {
                    // Processing a conditional pattern

                    let tag_expr = js::field(self.root.clone(), TAG_FIELD.to_string());
                    conditions_out.push((tag_expr, *tag));

                    if let Some(sub_pattern) = sub_pattern.as_ref() {
                        let cond_expr = self.get_root_val_expr(ctx);
                        let sub_tree = self
                            .conditional
                            .entry_or_insert_with(*tag, || Box::new(PatternTree::new(ctx, cond_expr)));
                        sub_tree.add_pattern(sub_pattern, ctx, bindings_out, Some(conditions_out));
                    }
                } else {
                    // If conditions_out is None, then we are processing an unconditional pattern
                    if let Some(sub_pattern) = sub_pattern.as_ref() {
                        let cond_expr = self.get_root_val_expr(ctx);
                        let sub_tree = self
                            .unconditional
                            .get_or_insert_with(|| Box::new(PatternTree::new(ctx, cond_expr)));
                        sub_tree.add_pattern(sub_pattern, ctx, bindings_out, conditions_out);
                    }
                }
            }
            Record(_, (pairs, _), as_pat) => {
                for ((name, _), field) in pairs.iter() {
                    let ast::RecordPatternMember::Field(pat) = field;

                    let field_name = ctx.get_new(*name);
                    let sub_tree = self.fields.entry_or_insert_with(*name, || {
                        Box::new(PatternTree::new(ctx, js::field(self.root.clone(), field_name)))
                    });
                    sub_tree.add_pattern(pat, ctx, bindings_out, conditions_out.as_deref_mut());
                }

                if let Some(d) = as_pat.as_ref()
                    && let Some(ml_name) = d.0.0
                {
                    bindings_out.push((ml_name, self.root.clone()));
                }
            }

            Var(d) => {
                if let Some(ml_name) = d.0.0 {
                    bindings_out.push((ml_name, self.root.clone()));
                }
            }
        }
    }

    fn get_assigns(self, strings: &lasso::Rodeo, out: &mut Vec<js::Expr>) -> Result<(), ICE> {
        out.extend(self.extra_assigns);

        for (_field, sub_tree) in self.fields.into_iter() {
            sub_tree.get_assigns(strings, out)?;
        }
        if let Some(unconditional) = self.unconditional {
            unconditional.get_assigns(strings, out)?;
        }
        for (tag, sub_tree) in self.conditional.into_iter() {
            let tag_expr = js::field(self.root.clone(), TAG_FIELD.to_string());
            let tag_str = strings.try_resolve(&tag).ok_or_else(ice)?;
            let cond = js::eqop(tag_expr, js::lit(format!("\"{}\"", tag_str)));

            let mut temp = Vec::new();
            sub_tree.get_assigns(strings, &mut temp)?;
            if !temp.is_empty() {
                let body = js::comma_list(temp);
                out.push(js::logical_and(vec![cond, body]));
            }
        }
        Ok(())
    }
}

fn compile_let_pattern_flat(
    ctx: &mut Context<'_>,
    out: &mut Vec<js::Expr>,
    pat: &ast::LetPattern,
    rhs: js::Expr,
) -> Result<(), ICE> {
    let mut root = PatternTree::new(ctx, rhs);
    let mut bindings = Vec::new();
    root.add_pattern(pat, ctx, &mut bindings, None);
    root.get_assigns(ctx.1, out)?;

    for (ml_name, js_expr) in bindings {
        ctx.set_binding(ml_name, js_expr);
    }
    Ok(())
}

fn compile_newtype(ctx: &mut Context<'_>, exprs: &mut Vec<js::Expr>, def: &ast::NewtypeDef) {
    // Need to add the implicitly bound constructors and coercion functions to the ML scope
    if let NewtypeRHS::Enum(ctors) = &def.rhs {
        for (name, body) in ctors.iter() {
            let val = if let Some(_body) = body {
                let arg = js::var("x".to_string());
                let body = case_obj(ctx, name.0, arg.clone());
                js::func(arg, "_".to_string(), body)
            } else {
                case_obj(ctx, name.0, js::void())
            };
            ctx.new_var_assign(name.0, val, exprs);
        }
    }

    ctx.set_binding(def.name, js::var("id".to_string()));
    ctx.set_binding(def.name2, js::var("id".to_string()));
}

fn compile_statement(ctx: &mut Context<'_>, exprs: &mut Vec<js::Expr>, stmt: &ast::Statement) -> Result<(), ICE> {
    use ast::Statement::*;
    match stmt {
        Empty => {}
        Expr(expr) => exprs.push(compile(ctx, expr)?),
        Import(lhs, rhs) => {
            let bindings = ctx.0.imported.get(&lhs.0).ok_or_else(ice)?;
            use ast::ImportStyle::*;
            match rhs {
                Full(name) => {
                    // Have to bypass ctx.set_binding() to satisfy borrow checker here.
                    ctx.0.bindings.insert(name.0, bindings.all.clone());
                }
                Fields(import_fields) => {
                    for na in import_fields.iter() {
                        if let Some(expr) = bindings.vars.get(&na.name.0) {
                            ctx.0.bindings.insert(na.alias.0, expr.clone());
                        } else {
                            ctx.0.bindings.remove(&na.alias.0);
                        }
                    }
                }
            }
        }
        LetDef((pat, var_expr)) => {
            let rhs = compile(ctx, var_expr)?;
            compile_let_pattern_flat(ctx, exprs, pat, rhs)?;
        }
        LetRecDef(defs) => {
            let mut vars = Vec::new();
            let mut rhs_exprs = Vec::new();
            for ((name, _), _, _) in defs {
                vars.push(ctx.new_var(*name))
            }
            for (_, _, expr) in defs {
                rhs_exprs.push(compile(ctx, expr)?)
            }

            // Since dead code elimination is a single backwards pass, we need to skip it
            // in case of mutually recursive definitions to avoid false positives.
            let dont_optimize = vars.len() > 1;
            for (lhs, rhs) in vars.into_iter().zip(rhs_exprs) {
                exprs.push(js::assign(lhs, rhs, dont_optimize));
            }
        }
        ModuleDef(name, _type, _, expr) => {
            let rhs = compile(ctx, expr)?;
            ctx.new_var_assign(name.0, rhs, exprs);
        }
        NewtypeDef(def) => {
            compile_newtype(ctx, exprs, def);
        }
        NewtypeRecDef(defs) => {
            for def in defs {
                compile_newtype(ctx, exprs, def);
            }
        }
        Println(args) => {
            let mut compiled_args = Vec::new();
            for expr in args.iter() {
                compiled_args.push(compile(ctx, expr)?);
            }
            exprs.push(js::println(compiled_args));
        }
        TypeAlias(..) => {}
    }
    Ok(())
}

fn compile_imported_file(
    ctx: &mut Context<'_>,
    id: ast::ImportId,
    parsed: ast::File,
    exprs: &mut Vec<js::Expr>,
) -> Result<(), ICE> {
    let vars = ctx.ml_scope(|ctx| {
        for item in parsed.statements.0.iter() {
            compile_statement(ctx, exprs, item)?;
        }

        let mut bindings = HashMap::new();
        for (name, expr) in ctx.bindings.m.iter() {
            bindings.insert(*name, expr.clone());
        }

        Ok(bindings)
    })?;

    let mut fields = Vec::new();
    if let Some(exports) = parsed.exports.as_ref() {
        for (name, _) in exports.field_keys()? {
            let expr = vars.get(&name).ok_or_else(ice)?.clone();
            fields.push((ctx.get_new(name), expr));
        }
    } else {
        // No explicit exports, so just include all available bindings.
        for (name, expr) in vars.iter() {
            fields.push((ctx.get_new(*name), expr.clone()));
        }

        // Have to sort fields to ensure deterministic output
        fields.sort_unstable_by_key(|(name, _)| name.clone());
    }
    let all = ctx.new_temp_var_assign(js::obj(fields), exprs);

    ctx.imported.insert(id, ModuleBindings { all, vars });
    Ok(())
}

pub fn compile_script(ctx: &mut Context<'_>, parsed: &ast::File) -> Result<js::Expr, ICE> {
    let mut exprs = Vec::new();

    for (id, file) in std::mem::take(&mut ctx.pending_imports) {
        compile_imported_file(ctx, id, file, &mut exprs)?;
    }

    for item in parsed.statements.0.iter() {
        compile_statement(ctx, &mut exprs, item)?;
    }
    // If the last statement is not an expression, don't return a value
    if !matches!(parsed.statements.0.last(), Some(ast::Statement::Expr(_))) {
        exprs.push(js::void());
    }

    let mut res = js::comma_list(exprs);

    let other_vars_to_keep = ctx
        .bindings
        .m
        .values()
        .chain(ctx.imported.values().map(|b| &b.all))
        .cloned()
        .collect::<Vec<_>>();
    js::optimize(&mut res, ctx.scope_var_name.to_owned(), &other_vars_to_keep)?;
    Ok(res)
}
