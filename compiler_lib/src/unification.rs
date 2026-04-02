// Copyright (c) 2026 Robert Grosse. All rights reserved.
use crate::coercion::*;
use crate::core::*;
use crate::introspect_types::*;
use crate::spans::ice;
use crate::spans::*;
use crate::type_errors::*;
use crate::types::*;

#[derive(Debug, Clone, Copy)]
pub struct UnifiedSource(Value);
impl UnifiedSource {
    pub fn err(&self) -> PartialTypeError {
        PartialTypeError::UnificationConflict(self.0)
    }
}

#[derive(Debug, Clone)]
pub struct UnifyConstructors {
    cb: UnwrapCoercionCallback,
    params: ConstructorAppParams<Value>,
    lhs_with_params: Value,
    hole_src: HoleSrc,
    scopelvl: ScopeLvl,
}
impl UnifyConstructors {
    fn run(
        self,
        core: &mut TypeCheckerCore,
        lhs: Value,
        edge_context: TypeEdge,
    ) -> Result<(UTypeHead, UnifiedSource), SpannedError> {
        let (lhs_head, _) = core.get_vhead(lhs)?;
        let source = UnifiedSource(lhs);

        use UTypeHead::*;
        use VTypeHead::*;
        match lhs_head {
            VConstructorApplication(..) => Err(ice().into()),
            VTypeConstructor(data) => {
                let res = UTypeConstructor(data.clone());
                self.cb
                    .run(core, Some(data.clone()), &self.params, self.lhs_with_params, edge_context)?;
                Ok((res, source))
            }

            _ => {
                let (v, u) = core.var(self.hole_src, self.scopelvl);
                self.cb.run_with_structural_type(core, v, edge_context)?;
                Ok((UFilterOutConstructors(u), source))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnifyKinds {
    cb: UnwrapCoercionCallback,
    hole_src: HoleSrc,
    scopelvl: ScopeLvl,
    span: Span,
}
impl UnifyKinds {
    fn run(
        self,
        core: &mut TypeCheckerCore,
        lhs: Value,
        edge_context: TypeEdge,
    ) -> Result<(UTypeHead, UnifiedSource), SpannedError> {
        let (lhs_head, _) = core.get_vhead(lhs)?;
        let source = UnifiedSource(lhs);

        use UTypeHead::*;
        use VTypeHead::*;
        match lhs_head {
            VConstructorApplication(data) => {
                let data = data.clone();
                let mut new_bounds = Vec::new();
                let mut new_params = Vec::new();
                for (pv, pu) in data.params.0 {
                    let mut new_bound = (None, None);
                    let mut new_param = (None, None);
                    if let Some(_covariant_var) = pv {
                        let (v, u) = core.var(self.hole_src, self.scopelvl);
                        new_bound.0 = Some(u);
                        new_param.0 = Some(v);
                    }
                    if let Some(_contravariant_var) = pu {
                        let (v, u) = core.var(self.hole_src, self.scopelvl);
                        new_bound.1 = Some(v);
                        new_param.1 = Some(u);
                    }
                    new_bounds.push(new_bound);
                    new_params.push(new_param);
                }

                // Create a new unification node to serve as the "type constructor" subnode.
                let cb = UnifyConstructors {
                    cb: self.cb,
                    hole_src: self.hole_src,
                    scopelvl: self.scopelvl,
                    params: ConstructorAppParams::new(new_params),
                    lhs_with_params: lhs,
                };
                let cb_u = core.use_placeholder();
                core.set_unification_callback(cb_u, UnificationCallback::C(cb), self.span)?;

                Ok((
                    UConstructorApplication(ConstructorAppData {
                        tycon: cb_u,
                        kind: data.kind,
                        params: ConstructorAppParams::new(new_bounds),
                    }),
                    source,
                ))
            }

            _ => UnifyConstructors {
                cb: self.cb,
                hole_src: self.hole_src,
                scopelvl: self.scopelvl,
                params: ConstructorAppParams::empty(),
                lhs_with_params: lhs,
            }
            .run(core, lhs, edge_context),
        }
    }
}

#[derive(Debug, Clone)]
pub enum UnificationCallback {
    K(UnifyKinds),
    C(UnifyConstructors),
}
impl UnificationCallback {
    pub fn run(
        self,
        core: &mut TypeCheckerCore,
        lhs: Value,
        edge_context: TypeEdge,
    ) -> Result<(UTypeHead, UnifiedSource), SpannedError> {
        match self {
            UnificationCallback::K(k) => k.run(core, lhs, edge_context),
            UnificationCallback::C(c) => c.run(core, lhs, edge_context),
        }
    }
}

pub fn create_unification_nodes(
    core: &mut TypeCheckerCore,
    scopelvl: ScopeLvl,
    target_span: Span,
    cb: UnwrapCoercionCallback,

    lhs: Value,
) -> Result<(), SpannedError> {
    let (head, params) = core.v_dissect_sub(lhs)?;
    let params = params.cloned();
    use DissectedType::*;
    match head {
        // Note: never[params] is coercible to never, so it is ok here.
        Never => {}
        Hole(hole_data) => {
            // Invariant: Types are always valid in the current scope.
            if hole_data.scopelvl > scopelvl {
                return Err(ice().into());
            }

            if let Some(params) = params {
                // This branch is only taken in the unlikely event that we have a constructor
                // application on the LHS where the constructor node is an inference variable.
                // In this case, we skip creating UnifyKinds and create UnifyConstructors directly.
                // This means that we need to get the constructor variable node out of the lhs
                // so we can connect it to the new UnifyConstructors node.
                let head = core.get_vhead(lhs)?.0;
                let lhs_tycon_var = match head {
                    VTypeHead::VConstructorApplication(data) => data.tycon,
                    _ => return Err(ice().into()),
                };

                let cb = UnifyConstructors {
                    cb,
                    params,
                    lhs_with_params: lhs,
                    hole_src: hole_data.src,
                    scopelvl,
                };
                let cb_u = core.use_placeholder();
                core.set_unification_callback(cb_u, UnificationCallback::C(cb), target_span)?;
                core.flow(lhs_tycon_var, cb_u, target_span);
            } else {
                let cb = UnifyKinds {
                    cb,
                    hole_src: hole_data.src,
                    scopelvl,
                    span: target_span,
                };
                let cb_u = core.use_placeholder();
                core.set_unification_callback(cb_u, UnificationCallback::K(cb), target_span)?;
                core.flow(lhs, cb_u, target_span);
            }
        }

        Any | Func | Record | Variant => cb.run_with_structural_type(core, lhs, TypeEdge::root(target_span))?,
        Constructor(data) => {
            let params = params.unwrap_or_default();
            cb.run(core, Some(data), &params, lhs, TypeEdge::root(target_span))?
        }

        Unknown => return Err(ice().into()),
    }
    Ok(())
}

pub fn create_pattern_unification_var(
    core: &mut TypeCheckerCore,
    scopelvl: ScopeLvl,
    pattern_span: Span,
    cb: UnwrapCoercionCallback,
) -> Result<Use, ICE> {
    let cb_u = core.use_placeholder();
    let hole_src = HoleSrc::AddPatternCoerceHint(pattern_span, cb_u);
    // Create an inference var so that unification nodes are never exposed directly, and to ensure that the var may appear in error messages.
    let (v, u) = core.var(hole_src, scopelvl);

    let cb = UnifyKinds {
        cb,
        hole_src,
        scopelvl,
        span: pattern_span,
    };
    core.set_unification_callback(cb_u, UnificationCallback::K(cb), pattern_span)?;
    core.flow(v, cb_u, pattern_span);

    Ok(u)
}
