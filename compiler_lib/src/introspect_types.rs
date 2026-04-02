// Copyright (c) 2026 Robert Grosse. All rights reserved.
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::StringId;
use crate::core::*;
use crate::spans::ICE;
use crate::spans::Span;
use crate::spans::ice;
use crate::spines::SpineConstructor;
use crate::typeck::*;
use crate::types::*;

#[derive(Debug)]
pub enum ValOrHole<'a> {
    Never,
    Val(&'a VTypeHead, Span),
    Hole(&'a InferenceVarData),
}
impl TypeCheckerCore {
    pub fn get_val_or_hole(&self, v: Value) -> Result<ValOrHole<'_>, ICE> {
        use ValOrHole::*;
        if v == BOT {
            return Ok(Never);
        }

        let node = self.r.get(v.0).ok_or_else(ice)?;
        match node {
            TypeNode::Var(data) => Ok(Hole(data)),
            TypeNode::Value((vhead, span)) => Ok(Val(vhead, *span)),
            _ => Err(ice()),
        }
    }

    pub fn get_vhead(&self, v: Value) -> Result<(&VTypeHead, Span), ICE> {
        use ValOrHole::*;
        match self.get_val_or_hole(v)? {
            Val(vhead, span) => Ok((vhead, span)),
            _ => Err(ice()),
        }
    }
}

#[derive(Debug)]
pub enum UseOrHole<'a> {
    Any,
    Use(&'a UTypeHead, Span, UseSrc),
    Hole(&'a InferenceVarData),
    Unification, // This should never happen
}
impl TypeCheckerCore {
    pub fn get_use_or_hole(&self, u: Use) -> Result<UseOrHole<'_>, ICE> {
        use UseOrHole::*;
        if u == TOP {
            return Ok(Any);
        }

        let node = self.r.get(u.0).ok_or_else(ice)?;
        match node {
            TypeNode::Var(data) => Ok(Hole(data)),
            TypeNode::Use((uhead, span, src)) => Ok(Use(uhead, *span, *src)),
            TypeNode::LazyUse(..) => Ok(Unification), // This should never happen
            _ => Err(ice()),
        }
    }

    pub fn get_uhead(&self, u: Use) -> Result<(&UTypeHead, Span, UseSrc), ICE> {
        use UseOrHole::*;
        match self.get_use_or_hole(u)? {
            Use(uhead, span, src) => Ok((uhead, span, src)),
            _ => Err(ice()),
        }
    }
}

#[derive(Debug)]
pub enum LoadedSpineOrType<T> {
    Spine(Rc<SpineConstructor>, ConstructorAppParams<Value>),
    Type(T),
}

#[derive(Debug)]
pub enum DissectedType {
    Never,
    Any,

    Func,
    Record,
    Variant,

    Constructor(ConstructorData),
    Hole(InferenceVarData),
    Unknown,
}

impl TypeCheckerCore {
    pub fn v_dissect_sub(&self, v: Value) -> Result<(DissectedType, Option<&ConstructorAppParams<Value>>), ICE> {
        use DissectedType::*;
        let vhead = match self.get_val_or_hole(v)? {
            ValOrHole::Never => return Ok((Never, None)),
            ValOrHole::Hole(data) => return Ok((Hole(*data), None)),
            ValOrHole::Val(vhead, _) => vhead,
        };

        use VTypeHead::*;
        let mut params = None;
        let dt = match vhead {
            VConstructorApplication(data) => {
                let sub = self.v_dissect_sub(data.tycon)?;
                if sub.1.is_some() {
                    return Err(ice());
                }
                params = Some(&data.params);
                sub.0
            }
            VTypeConstructor(data) => Constructor(data.clone()),

            VCaseUnion(..) => Variant,
            VTop => Any,
            VFunc { .. } => Func,
            VObj { .. } => Record,
            VCase { .. } => Variant,
            VEphemeralPoly(..) => Any,
        };
        Ok((dt, params))
    }

    pub fn v_dissect(&self, v: Value) -> Result<(DissectedType, Option<&ConstructorAppParams<Value>>), ICE> {
        self.v_dissect_sub(v)
    }

    pub fn v_as_spine_or_type(&self, v: Value) -> Result<LoadedSpineOrType<Value>, ICE> {
        if let (DissectedType::Constructor(data), params) = self.v_dissect(v)?
            && let Some(spine) = data.spine
        {
            return Ok(LoadedSpineOrType::Spine(spine, params.cloned().unwrap_or_default()));
        }
        Ok(LoadedSpineOrType::Type(v))
    }

    pub fn u_dissect_sub(&self, u: Use) -> Result<(DissectedType, Option<&ConstructorAppParams<Use>>), ICE> {
        use DissectedType::*;
        let uhead = match self.get_use_or_hole(u)? {
            UseOrHole::Any => return Ok((Any, None)),
            UseOrHole::Hole(data) => return Ok((Hole(*data), None)),
            UseOrHole::Unification => return Err(ice()),
            UseOrHole::Use(uhead, ..) => uhead,
        };

        use UTypeHead::*;
        let mut params = None;
        let dt = match uhead {
            UConstructorApplication(data) => {
                let sub = self.u_dissect_sub(data.tycon)?;
                if sub.1.is_some() {
                    return Err(ice());
                }
                params = Some(&data.params);
                sub.0
            }
            UTypeConstructor(data) => Constructor(data.clone()),

            UBot => Never,
            UFunc { .. } => Func,
            UObj { .. } => Record,
            UCase { .. } => Variant,
            UEphemeralPoly(..) => Never,

            UFilterOutConstructors(..) => Unknown,
            UIntersection(..) => Unknown,
        };
        Ok((dt, params))
    }

    pub fn u_dissect(&self, u: Use) -> Result<(DissectedType, Option<&ConstructorAppParams<Use>>), ICE> {
        self.u_dissect_sub(u)
    }
}

impl TypeCheckerCore {
    pub fn get_aliases(&self, v: Value) -> Result<&HashMap<StringId, TypeBinding>, ICE> {
        let node = self.r.get(v.0).ok_or_else(ice)?;
        if let TypeNode::Value((VTypeHead::VObj { aliases, .. }, _)) = node {
            Ok(aliases)
        } else {
            Err(ice())
        }
    }
}
