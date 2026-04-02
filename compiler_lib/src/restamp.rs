// Copyright (c) 2026 Robert Grosse. All rights reserved.
use crate::core::*;
use crate::introspect_types::*;
use crate::spans::*;
use crate::types::*;

fn restamp_vcon_app(core: &mut TypeCheckerCore, mut v: Value, span: Span) -> Result<Value, ICE> {
    if let ValOrHole::Val(vhead, _) = core.get_val_or_hole(v)? {
        match vhead {
            VTypeHead::VTypeConstructor(data) if data.restamp => {
                let mut data = data.clone();
                data.restamp = false;
                v = core.new_val(VTypeHead::VTypeConstructor(data), span);
            }
            VTypeHead::VConstructorApplication(data) => {
                let mut data = data.clone();
                let tycon = restamp_vcon_app(core, data.tycon, span)?;
                if tycon != data.tycon {
                    data.tycon = tycon;
                    v = core.new_val(VTypeHead::VConstructorApplication(data), span);
                }
            }
            _ => {}
        }
    }
    Ok(v)
}

fn restamp_ucon_app(core: &mut TypeCheckerCore, mut u: Use, span: Span) -> Result<Use, ICE> {
    if let UseOrHole::Use(uhead, _, UseSrc::None) = core.get_use_or_hole(u)? {
        match uhead {
            UTypeHead::UTypeConstructor(data) if data.restamp => {
                let mut data = data.clone();
                data.restamp = false;
                u = core.new_use(UTypeHead::UTypeConstructor(data), span);
            }
            UTypeHead::UConstructorApplication(data) => {
                let mut data = data.clone();
                let tycon = restamp_ucon_app(core, data.tycon, span)?;
                if tycon != data.tycon {
                    data.tycon = tycon;
                    u = core.new_use(UTypeHead::UConstructorApplication(data), span);
                }
            }
            _ => {}
        }
    }
    Ok(u)
}

pub fn restamp_func_or_val(core: &mut TypeCheckerCore, mut v: Value, span: Span) -> Result<Value, ICE> {
    if let ValOrHole::Val(head, _) = core.get_val_or_hole(v)? {
        match head {
            &VTypeHead::VFunc { arg, ret, prop } => {
                let arg2 = restamp_ucon_app(core, arg, span)?;
                let ret2 = restamp_vcon_app(core, ret, span)?;
                if arg2 != arg || ret2 != ret {
                    v = core.new_val(
                        VTypeHead::VFunc {
                            arg: arg2,
                            ret: ret2,
                            prop,
                        },
                        span,
                    );
                }
            }
            VTypeHead::VConstructorApplication(..) | VTypeHead::VTypeConstructor(_) => {
                v = restamp_vcon_app(core, v, span)?;
            }
            _ => {}
        }
    }
    Ok(v)
}
