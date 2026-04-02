// Copyright (c) 2026 Robert Grosse. All rights reserved.
use std::iter::zip;

use crate::ast::StringId;
use crate::core::*;
use crate::instantiate::*;
use crate::introspect_types::*;
use crate::spans::ICE;
use crate::spans::Span;
use crate::spans::SpannedError;
use crate::spans::ice;
use crate::spines::Replacements;
use crate::spines::SpineContents;
use crate::subsumption::*;
use crate::templates::*;
use crate::types::*;

// Only needs to be Clone for REPL rewind, clones will not normally actually be used.
#[derive(Debug, Clone)]
pub enum InnerCallback {
    Sub(SubsumptionCheck),
    Func(FuncInstantiationCheck),
    Record(Use),
    Variant(Use),
}
impl InnerCallback {
    fn run(self, core: &mut TypeCheckerCore, lhs: Value, mut edge_context: TypeEdge) -> Result<(), SpannedError> {
        let (v, u) = match self {
            InnerCallback::Sub(sub) => {
                // Subsumption relies on scopelvl=NO_HOLES to correctly compare the ephemral poly
                // types used to check subsumption.
                // Soundness: This is basically the same as if we called core.flow() to add a
                // new root edge between the resulting types.
                edge_context.scopelvl = NO_HOLES;
                sub.run(core, lhs)?
            }
            InnerCallback::Func(sub) => sub.run(core, lhs)?,
            InnerCallback::Record(u) => (lhs, u),
            InnerCallback::Variant(u) => (lhs, u),
        };
        core.add_pending_edge(v, u, edge_context);
        Ok(())
    }

    fn get_category(&self) -> CoercionTargetCategory {
        use CoercionTargetCategory::*;
        match self {
            InnerCallback::Sub(s) => match s.rhs_cat {
                SubsumptionCategory::Func => Func,
                SubsumptionCategory::Record => Record,
            },
            InnerCallback::Func(_) => Func,
            InnerCallback::Record(_) => Record,
            InnerCallback::Variant(_) => Variant,
        }
    }
}

pub fn apply_unwrap_coercion(
    core: &mut TypeCheckerCore,
    lhs_con: Option<ConstructorData>,
    params: &ConstructorAppParams<Value>,
    uncoerced: Value,
    expected: CoercionTargetCategory,
) -> Result<Value, SpannedError> {
    if let Some(data) = lhs_con {
        if let TyConDef::Custom(def) = core.tycons.get(data.category)
            && let Some(coercion) = def.unwrap_coercion.as_ref()
        {
            let found = coercion.target_category;
            // Don't use coercion if it doesn't have the expected type.
            if found == expected || found == CoercionTargetCategory::Never {
                let coercion = coercion.clone();
                let coerced = coercion.instantiate(core, params)?;
                return Ok(coerced);
            }
        };
    }
    Ok(uncoerced)
}

// Only used for the target type of subsumption expressions.
pub fn apply_wrap_coercion(
    core: &mut TypeCheckerCore,
    data: ConstructorData,
    params: &ConstructorAppParams<Use>,
    uncoerced: Use,
) -> Result<Use, SpannedError> {
    if let TyConDef::Custom(def) = core.tycons.get(data.category)
        && let Some(coercion) = def.wrap_coercion.as_ref()
    {
        let coercion = coercion.clone();
        let coerced = coercion.instantiate(core, params)?;
        return Ok(coerced);
    };
    Ok(uncoerced)
}

#[derive(Debug, Clone)]
pub struct UnwrapCoercionCallback {
    pub cb: InnerCallback,
}
impl UnwrapCoercionCallback {
    pub fn run(
        self,
        core: &mut TypeCheckerCore,
        lhs_con: Option<ConstructorData>,
        params: &ConstructorAppParams<Value>,
        lhs_with_params: Value,
        edge_context: TypeEdge,
    ) -> Result<(), SpannedError> {
        let cat = self.cb.get_category();
        let coerced_type = apply_unwrap_coercion(core, lhs_con, params, lhs_with_params, cat)?;
        self.cb.run(core, coerced_type, edge_context)
    }

    pub fn run_with_structural_type(
        self,
        core: &mut TypeCheckerCore,
        lhs: Value,
        edge_context: TypeEdge,
    ) -> Result<(), SpannedError> {
        self.run(core, None, &ConstructorAppParams::empty(), lhs, edge_context)
    }
}

#[derive(Debug)]
struct SpineCoercion {
    // (covariant name, contravariant name) list
    param_mapping: Vec<(Option<StringId>, Option<StringId>)>,

    tree: RcParsedType, // direct reference to tree of the RHS for an unwrap func
    spine_params: ConstructorAppParams<Value>,
}
impl SpineCoercion {
    fn instantiate<P: Materialize>(
        &self,
        core: &mut TypeCheckerCore,
        constructor_params: &ConstructorAppParams<P>,
    ) -> Result<P, ICE> {
        let mut replacements = TreeMaterializerState::new();
        replacements.add_spine_params(&self.spine_params);
        for (names, types) in zip(self.param_mapping.iter(), constructor_params.0.iter()) {
            if let Some(name) = names.0 {
                let ty = types.0.ok_or_else(ice)?;

                // The validation of implicit coercions ensures that each variable can only be
                // used with one variance, but we still have to supply both to the instantiation
                // replacement map. Therefore, we just pass bot/top for the unused half.
                let mut p = (BOT, TOP);
                ty.add_to_pair(&mut p);
                replacements
                    .new_spine_poly_var_replacements
                    .insert(name, ConOrTypes::Types(p));
            }
            if let Some(name) = names.1 {
                let ty = types.1.ok_or_else(ice)?;

                let mut p = (BOT, TOP);
                ty.add_to_pair(&mut p);
                replacements
                    .new_spine_poly_var_replacements
                    .insert(name, ConOrTypes::Types(p));
            }
        }
        replacements.with(core).materialize(&self.tree)
    }
}
#[derive(Debug)]
enum CoercionType<P: Materialize> {
    Spine(SpineCoercion),
    Direct(P),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoercionTargetCategory {
    Never,
    Func,
    Record,
    Variant,
    Other,
}
impl CoercionTargetCategory {
    pub fn new(dt: DissectedType, has_params: bool) -> Self {
        use DissectedType::*;
        match dt {
            Never => {
                if has_params {
                    CoercionTargetCategory::Other
                } else {
                    CoercionTargetCategory::Never
                }
            }
            Func => CoercionTargetCategory::Func,
            Record => CoercionTargetCategory::Record,
            Variant => CoercionTargetCategory::Variant,
            Any | Hole(_) | Unknown => CoercionTargetCategory::Other,
            Constructor(data) => {
                if let Some(spine) = data.spine {
                    spine.get_category()
                } else {
                    CoercionTargetCategory::Other
                }
            }
        }
    }

    pub fn parse_v(core: &TypeCheckerCore, ty: Value) -> Result<Self, ICE> {
        let (dt, params) = core.v_dissect_sub(ty)?;
        Ok(Self::new(dt, params.is_some()))
    }

    pub fn parse_u(core: &TypeCheckerCore, ty: Use) -> Result<Self, ICE> {
        let (dt, params) = core.u_dissect_sub(ty)?;
        Ok(Self::new(dt, params.is_some()))
    }
}

#[derive(Debug)]
pub struct Coercion<P: Materialize> {
    coercion: CoercionType<P>,
    pub target_category: CoercionTargetCategory,
}
impl<P: Materialize> Coercion<P> {
    pub fn instantiate(&self, core: &mut TypeCheckerCore, constructor_params: &ConstructorAppParams<P>) -> Result<P, ICE> {
        match &self.coercion {
            CoercionType::Spine(spine_coercion) => spine_coercion.instantiate(core, constructor_params),
            CoercionType::Direct(ty) => Ok(*ty),
        }
    }
}

#[derive(Debug)]
pub enum InvalidCoercionError {
    ICE(ICE),

    NotAFunction,
    InvalidInputType,
    InputNotGeneral,
    VarNotUnique,

    InvalidUnwrapOutputType,
    InvalidWrapOutputType,
}
impl InvalidCoercionError {
    pub fn spanned(self, span: Span) -> SpannedError {
        use InvalidCoercionError::*;
        let s = match self {
            ICE(ice) => return ice.into(),
            NotAFunction => "Not a pure identity function.",
            InvalidInputType => "Input type does not match specified type.",
            InputNotGeneral => "Input type does not handle all possible parameter types.",
            VarNotUnique => "Coercion parameters are not uniquely determined by input type.",
            InvalidUnwrapOutputType => "Output type of unwrap coercion must be a function, record, variant, or never.",
            InvalidWrapOutputType => "Output type of wrap coercion must be a function or record.",
        };
        SpannedError::new1(format!("TypeError: Invalid implicit coercion definition. {}", s), span)
    }
}
impl From<ICE> for InvalidCoercionError {
    fn from(e: ICE) -> Self {
        InvalidCoercionError::ICE(e)
    }
}
fn ice2() -> InvalidCoercionError {
    InvalidCoercionError::ICE(ice())
}

fn check_val_param(
    core: &TypeCheckerCore,
    v: Option<Value>,
    ckey: ComparisonKey,
    m: &Option<TreeMaterializerState<'_>>,
) -> Result<Option<StringId>, InvalidCoercionError> {
    if let Some(p) = v {
        use ValOrHole::*;
        match core.get_val_or_hole(p)? {
            Never => Ok(None),
            Val(VTypeHead::VEphemeralPoly(p), _) if p.key == ckey => {
                let m = m.as_ref().ok_or_else(ice2)?;
                let count = m.state_v.poly_count.get(&p.name).copied().unwrap_or(0);
                if count != 1 {
                    return Err(InvalidCoercionError::VarNotUnique);
                }

                Ok(Some(p.name))
            }
            _ => Err(InvalidCoercionError::InputNotGeneral),
        }
    } else {
        Ok(None)
    }
}

fn check_use_param(
    core: &TypeCheckerCore,
    v: Option<Use>,
    ckey: ComparisonKey,
    m: &Option<TreeMaterializerState<'_>>,
) -> Result<Option<StringId>, InvalidCoercionError> {
    if let Some(p) = v {
        use UseOrHole::*;
        match core.get_use_or_hole(p)? {
            Any => Ok(None),
            Use(UTypeHead::UEphemeralPoly(p), ..) if p.key == ckey => {
                let m = m.as_ref().ok_or_else(ice2)?;
                let count = m.state_u.poly_count.get(&p.name).copied().unwrap_or(0);
                if count != 1 {
                    return Err(InvalidCoercionError::VarNotUnique);
                }

                Ok(Some(p.name))
            }
            _ => Err(InvalidCoercionError::InputNotGeneral),
        }
    } else {
        Ok(None)
    }
}

pub fn parse_and_register_coercion(
    core: &mut TypeCheckerCore,
    tycon_ind: TyConDefInd,
    f: Value,
    span: Span,
    is_unwrap: bool,
    allow_invalid_output_type: bool,
) -> Result<(), InvalidCoercionError> {
    // Note: The instantiated types will never actually be checked against anything, so the comparison key
    // here doesn't matter. It's just a dummy value since we have to provide something.
    let ckey = ComparisonKey(span);

    let top = core.v_as_spine_or_type(f)?;
    let (intantiated, m, pcount) = match &top {
        LoadedSpineOrType::Spine(spine, params) => {
            let (ty, m) = spine
                .template
                .instantiate_with_counts(core, Replacements::new(Some(ckey)), params)?;
            (ty, Some(m), spine.poly_params.len())
        }
        LoadedSpineOrType::Type(ty) => (*ty, None, 0),
    };

    use InvalidCoercionError::*;
    let (arg, ret) = if let ValOrHole::Val(
        &VTypeHead::VFunc {
            arg,
            ret,
            prop: FuncProperties { is_identity: true },
        },
        _,
    ) = core.get_val_or_hole(intantiated)?
    {
        (arg, ret)
    } else {
        return Err(NotAFunction);
    };

    let mut param_mapping = Vec::new();
    if is_unwrap {
        let (target, target_params) = core.u_dissect(arg)?;
        if let DissectedType::Constructor(data) = target
            && data.category == tycon_ind
        {
        } else {
            return Err(InvalidInputType);
        };

        for param in target_params.map(|p| p.0.as_slice()).unwrap_or_default() {
            let cov = check_use_param(core, param.0, ckey, &m)?;
            let contra = check_val_param(core, param.1, ckey, &m)?;
            param_mapping.push((cov, contra));
        }
    } else {
        let (target, target_params) = core.v_dissect(ret)?;
        if let DissectedType::Constructor(data) = target
            && data.category == tycon_ind
        {
        } else {
            return Err(InvalidInputType);
        };

        for param in target_params.map(|p| p.0.as_slice()).unwrap_or_default() {
            let cov = check_val_param(core, param.0, ckey, &m)?;
            let contra = check_use_param(core, param.1, ckey, &m)?;
            param_mapping.push((cov, contra));
        }
    }

    // Check whether there are any extra variables other than those constrainted by the input type.
    let mut used_count = 0;
    for (cov, contra) in &param_mapping {
        if cov.is_some() {
            used_count += 1;
        }
        if contra.is_some() {
            used_count += 1;
        }
    }
    if used_count != pcount {
        return Err(VarNotUnique);
    }

    if is_unwrap {
        let target_category = CoercionTargetCategory::parse_v(core, ret)?;

        use CoercionTargetCategory::*;
        let valid = match target_category {
            Never | Func | Record | Variant => true,
            Other => false,
        };
        if !valid && !allow_invalid_output_type {
            return Err(InvalidUnwrapOutputType);
        }

        let coercion = match top {
            LoadedSpineOrType::Spine(spine, params) => {
                let tree = match &spine.template {
                    SpineContents::Func(tree) => {
                        if let ParsedTypeHead::Func(_arg, ret, _) = &tree.1 {
                            ret.clone()
                        } else {
                            return Err(ice2());
                        }
                    }
                    SpineContents::Record(_) => return Err(ice2()),
                };
                CoercionType::Spine(SpineCoercion {
                    param_mapping,
                    tree,
                    spine_params: params,
                })
            }
            LoadedSpineOrType::Type(_) => CoercionType::Direct(ret),
        };
        let coercion = Coercion {
            coercion,
            target_category,
        };

        Ok(core.tycons.set_unwrap_coercion(tycon_ind, coercion)?)
    } else {
        let target_category = CoercionTargetCategory::parse_u(core, arg)?;

        use CoercionTargetCategory::*;
        let valid = matches!(target_category, Func | Record);
        if !valid && !allow_invalid_output_type {
            return Err(InvalidWrapOutputType);
        }

        let coercion = match top {
            LoadedSpineOrType::Spine(spine, params) => {
                let tree = match &spine.template {
                    SpineContents::Func(tree) => {
                        if let ParsedTypeHead::Func(arg, _ret, _) = &tree.1 {
                            arg.clone()
                        } else {
                            return Err(ice2());
                        }
                    }
                    SpineContents::Record(_) => return Err(ice2()),
                };
                CoercionType::Spine(SpineCoercion {
                    param_mapping,
                    tree,
                    spine_params: params,
                })
            }
            LoadedSpineOrType::Type(_) => CoercionType::Direct(arg),
        };
        let coercion = Coercion {
            coercion,
            target_category,
        };

        Ok(core.tycons.set_wrap_coercion(tycon_ind, coercion)?)
    }
}
