// Copyright (c) 2026 Robert Grosse. All rights reserved.
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::expr::CoerceSubHoleSrc;
use crate::coercion::*;
use crate::core::*;
use crate::introspect_types::*;
use crate::kinds::kinds_are_equal;
use crate::parse_types::ParsedTypeSubstitutions;
use crate::restamp::*;
use crate::spans::*;
use crate::spines::*;
use crate::type_errors::*;
use crate::types::*;

#[derive(Debug, Clone, Copy)]
pub enum SubsumptionCategory {
    Func,
    Record,
}

// Only needs to be Clone for REPL rewind, clones will not normally actually be used.
#[derive(Debug, Clone)]
pub struct SubsumptionCheck {
    scopelvl: ScopeLvl,
    target_type_span: Span,
    sub_hole_src: CoerceSubHoleSrc,
    rhs: Use,
    pub rhs_cat: SubsumptionCategory,
    rhs_spine: Option<Rc<SpineConstructor>>,
    rhs_params: ConstructorAppParams<Use>,
    subs: ParsedTypeSubstitutions,
}
impl SubsumptionCheck {
    pub fn new(
        core: &mut TypeCheckerCore,
        scopelvl: ScopeLvl,
        target_type_span: Span,
        sub_hole_src: CoerceSubHoleSrc,
        mut rhs: Use,
        subs: ParsedTypeSubstitutions,
    ) -> Result<Self, SpannedError> {
        let (mut rhs_dt, rhs_params) = core.u_dissect(rhs)?;
        let mut rhs_params = rhs_params.cloned().unwrap_or_default();
        if let DissectedType::Constructor(data) = rhs_dt {
            rhs = apply_wrap_coercion(core, data, &rhs_params, rhs)?;
            let (dt, params) = core.u_dissect(rhs)?;
            rhs_dt = dt;
            rhs_params = params.cloned().unwrap_or_default();
        }

        let rhs_spine = match &rhs_dt {
            DissectedType::Constructor(data) => data.spine.clone(),
            _ => None,
        };

        let rhs_cat = match CoercionTargetCategory::new(rhs_dt, !rhs_params.0.is_empty()) {
            CoercionTargetCategory::Func => SubsumptionCategory::Func,
            CoercionTargetCategory::Record => SubsumptionCategory::Record,
            _ => {
                return Err(SpannedError::new1(
                    "TypeError: Output type of :> expression must be a function or record type, or coercible from one.",
                    target_type_span,
                ));
            }
        };

        Ok(Self {
            scopelvl,
            target_type_span,
            sub_hole_src,
            rhs,
            rhs_cat,
            rhs_spine,
            rhs_params,
            subs,
        })
    }

    pub fn run(self, core: &mut TypeCheckerCore, lhs: Value) -> Result<(Value, Use), SpannedError> {
        let comparison_key = self.subs.placeholder_kinds.as_ref().map(|s| s.key);
        let mut lhs_replacements = Replacements::new(comparison_key);
        let mut rhs_replacements = Replacements::new(comparison_key);
        let lhs_lst = core.v_as_spine_or_type(lhs)?;
        let lhs_spine = match &lhs_lst {
            LoadedSpineOrType::Spine(spine, _params) => Some(spine.clone()),
            LoadedSpineOrType::Type(_) => None,
        };

        use SubsumptionCategory::*;
        match self.rhs_cat {
            Func => {
                func_subsumption_sub(
                    core,
                    self.scopelvl,
                    self.target_type_span,
                    self.sub_hole_src,
                    self.subs,
                    &lhs_spine,
                    &self.rhs_spine,
                    &mut lhs_replacements,
                )?;
            }
            Record => {
                // For functions, the LHS is instantiated while the RHS is abstract.
                // For records, it's the other way around.
                // Therefore, we can just reuse the func code by calling it with rhs and lhs swapped.
                func_subsumption_sub(
                    core,
                    self.scopelvl,
                    self.target_type_span,
                    self.sub_hole_src,
                    self.subs,
                    &self.rhs_spine,
                    &lhs_spine,
                    &mut rhs_replacements,
                )?;
            }
        };

        let lhs = match lhs_lst {
            LoadedSpineOrType::Spine(spine, params) => spine.template.instantiate(core, lhs_replacements, &params)?,
            LoadedSpineOrType::Type(t) => t,
        };

        let rhs = if let Some(rhs_spine) = &self.rhs_spine {
            rhs_spine.template.instantiate(core, rhs_replacements, &self.rhs_params)?
        } else {
            self.rhs
        };

        Ok((lhs, rhs))
    }
}

fn func_subsumption_sub(
    core: &mut TypeCheckerCore,
    scopelvl: ScopeLvl,
    _target_type_span: Span,
    sub_hole_src: CoerceSubHoleSrc,
    mut subs: ParsedTypeSubstitutions,
    lhs_spine: &Option<Rc<SpineConstructor>>,
    rhs_spine: &Option<Rc<SpineConstructor>>,
    lhs_replacements: &mut Replacements,
) -> Result<(), SpannedError> {
    let mut rhs_kinds = HashMap::new();
    if let Some(rhs_spine) = rhs_spine {
        for (kind, name) in rhs_spine.poly_params.iter() {
            rhs_kinds.insert(*name, kind);
        }
    }

    // Make sure that the actual kinds match the expected kinds of any placeholders.
    if let Some(phs) = &mut subs.placeholder_kinds {
        for (name, expected) in phs.m.iter_mut() {
            if let Some(kind) = rhs_kinds.get(name) {
                expected.check(kind)?;
            }
        }
    }

    if let Some(lhs_spine) = lhs_spine {
        for (kind, name) in lhs_spine.poly_params.iter() {
            if let Some(mut found) = subs.subs.remove(name) {
                found.1.check(kind)?;
                lhs_replacements.add(*name, found.0);
            } else {
                if let Some(other) = rhs_kinds.get(name)
                    && kinds_are_equal(&kind.0, &other.0)
                {
                    // Leave it out from the replacement dict and it will be replaced
                    // with an ephemeral poly comparison type by default.
                } else {
                    // Create a new type variable for the missing substitution
                    let hole_src = sub_hole_src.get(*name);
                    let p = core.var(hole_src, scopelvl);
                    lhs_replacements.add(*name, p);
                };
            }
        }
    }

    Ok(())
}

//////////////////////////////////////////////////////////////////////////////////////
#[derive(Debug, Clone)]
pub struct FuncInstantiationCheck {
    pub scopelvl: ScopeLvl,
    pub lhs_expr_span: Span,
    pub rhs: Use,
}
impl FuncInstantiationCheck {
    pub fn run(self, core: &mut TypeCheckerCore, lhs: Value) -> Result<(Value, Use), SpannedError> {
        let lhs = match core.v_as_spine_or_type(lhs)? {
            LoadedSpineOrType::Spine(spine, params) => {
                instantiate_spine_func(core, self.scopelvl, self.lhs_expr_span, spine, params)?
            }
            LoadedSpineOrType::Type(t) => t,
        };

        let lhs = restamp_func_or_val(core, lhs, self.lhs_expr_span)?;
        Ok((lhs, self.rhs))
    }
}

fn instantiate_spine_func(
    core: &mut TypeCheckerCore,
    scopelvl: ScopeLvl,
    lhs_expr_span: Span,
    spine: Rc<SpineConstructor>,
    params: ConstructorAppParams<Value>,
) -> Result<Value, ICE> {
    let mut lhs_flat = Vec::new();
    for (_kind, name) in spine.poly_params.iter() {
        lhs_flat.push(*name);
    }

    let mut lhs_replacements = Replacements::new(None);
    for name in lhs_flat {
        let hole_src = HoleSrc::AddFuncCoerce(lhs_expr_span, name);
        let p = core.var(hole_src, scopelvl);
        lhs_replacements.add(name, p);
    }

    spine.template.instantiate(core, lhs_replacements, &params)
}
