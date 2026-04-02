// Copyright (c) 2026 Robert Grosse. All rights reserved.
use crate::ast::StringId;
use crate::core::*;
use crate::spans::ICE;
use crate::spans::Span;
use crate::spans::ice;
use crate::templates::*;
use crate::typeck::TypeBinding;
use crate::types::*;

use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;

/// Wrapper to compare by address. Need to keep lifetime to prevent dangling pointer errors when types are dropped and memory reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct CacheKey<'b>(*const ParsedType, PhantomData<&'b ParsedType>);
impl<'b> CacheKey<'b> {
    fn new(ty: &'b ParsedType) -> Self {
        Self(ty as *const _, PhantomData)
    }
}

pub struct PolarState<'b, P: Polarity> {
    cache: HashMap<CacheKey<'b>, P>,
    rec_types: HashMap<SourceLoc, P>,

    // These substitution maps are filled in by the caller if applicable
    pub spine_params: Vec<Option<P>>,
    // Allow caller to see how many times vars were instantiated, for use in coercion validation
    pub poly_count: HashMap<StringId, usize>,
}
impl<'b, P: Polarity> PolarState<'b, P> {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            rec_types: HashMap::new(),
            spine_params: Vec::new(),
            poly_count: HashMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ConOrTypes {
    Con(TyConDefInd),
    Types((Value, Use)),
}
impl ConOrTypes {
    pub fn get<P: Polarity>(&self, core: &mut TypeCheckerCore, span: Span) -> P {
        match self {
            ConOrTypes::Con(ind) => P::new_simple(core, *ind, span),
            ConOrTypes::Types(p) => P::extract(*p),
        }
    }
}

pub struct TreeMaterializerState<'b> {
    pub state_v: PolarState<'b, Value>,
    pub state_u: PolarState<'b, Use>,

    // Filled in by the caller if applicable
    pub temp_poly_var_replacements: HashMap<(SourceLoc, StringId), ConOrTypes>,
    pub new_spine_poly_var_replacements: HashMap<StringId, ConOrTypes>,
    pub spine_poly_default_comparison_key: Option<ComparisonKey>,
    pub flip_pairs: bool,
}
impl<'b> TreeMaterializerState<'b> {
    pub fn new() -> Self {
        Self {
            state_v: PolarState::new(),
            state_u: PolarState::new(),
            temp_poly_var_replacements: HashMap::new(),
            new_spine_poly_var_replacements: HashMap::new(),
            spine_poly_default_comparison_key: None,
            flip_pairs: false,
        }
    }

    pub fn with<'a>(&'a mut self, core: &'a mut TypeCheckerCore) -> TreeMaterializer<'a, 'b> {
        TreeMaterializer { core, s: self }
    }

    pub fn add_spine_params<P: Materialize>(&mut self, params: &ConstructorAppParams<P>) {
        for (vopt, uopt) in params.0.iter() {
            P::get_state(self).spine_params.push(*vopt);
            P::get_state_other(self).spine_params.push(*uopt);
        }
    }
}

pub trait Materialize: Polarity {
    fn materialize_head<'a, 'b>(m: &mut TreeMaterializer<'a, 'b>, ty: &'b ParsedType) -> Result<Self::Head, ICE>;
    fn materialize_head_other<'a, 'b>(
        m: &mut TreeMaterializer<'a, 'b>,
        ty: &'b ParsedType,
    ) -> Result<<Self::Opposite as Polarity>::Head, ICE>;

    fn get_state<'a, 'b>(s: &'a mut TreeMaterializerState<'b>) -> &'a mut PolarState<'b, Self>;
    fn get_state_other<'a, 'b>(s: &'a mut TreeMaterializerState<'b>) -> &'a mut PolarState<'b, Self::Opposite>;
}

impl Materialize for Value {
    fn materialize_head<'a, 'b>(m: &mut TreeMaterializer<'a, 'b>, ty: &'b ParsedType) -> Result<VTypeHead, ICE> {
        use ParsedTypeHead::*;

        Ok(match &ty.1 {
            &Case(ref cases) => {
                let mut vtype_case_arms = Vec::new();
                for &(tag, (tag_span, ref ty)) in cases {
                    let v = m.materialize_val(ty)?;
                    vtype_case_arms.push(((tag, v), tag_span));
                }

                // Grammar ensures that cases is nonempty
                if cases.len() <= 1 {
                    VTypeHead::VCase {
                        case: vtype_case_arms[0].0,
                    }
                } else {
                    VTypeHead::VCaseUnion(
                        vtype_case_arms
                            .into_iter()
                            .map(|(case, span)| m.core.new_val(VTypeHead::VCase { case }, span))
                            .collect(),
                    )
                }
            }
            &Constructor(ref tycon, ref kind, ref parsed_params) => {
                let conval = m.materialize_val(tycon)?;

                let mut params = Vec::new();
                for pair in parsed_params {
                    use VarianceInvPair::*;
                    params.push(match pair {
                        Co(ty) => (Some(m.materialize_val(ty)?), None),
                        Contra(ty) => (None, Some(m.materialize_use(ty)?)),
                        InvSingle(ty) => {
                            let v = m.materialize_val(ty)?;
                            let u = m.materialize_use(ty)?;
                            (Some(v), Some(u))
                        }
                        InvPair(ty_r, ty_w) => {
                            let v = m.materialize_val(ty_r)?;
                            let u = m.materialize_use(ty_w)?;
                            (Some(v), Some(u))
                        }
                    });
                }

                VTypeHead::VConstructorApplication(ConstructorAppData {
                    tycon: conval,
                    kind: kind.clone(),
                    params: ConstructorAppParams::new(params),
                })
            }
            &Func(ref arg, ref ret, prop) => {
                let arg = m.materialize_use(arg)?;
                let ret = m.materialize_val(ret)?;
                VTypeHead::VFunc { arg, ret, prop }
            }
            &Record(ref template_fields, ref template_aliases, ref extra_aliases) => {
                let mut fields = HashMap::with_capacity(template_fields.len());
                for &(name, (span, ref param)) in template_fields {
                    use VarianceInvPair::*;

                    match param {
                        Co(ty) => {
                            let v = m.materialize_val(ty)?;
                            fields.insert(name, (v, None, span));
                        }
                        Contra(_ty) => return Err(ice()),
                        InvSingle(ty) => {
                            let v = m.materialize_val(ty)?;
                            let u = m.materialize_use(ty)?;
                            fields.insert(name, (v, Some(u), span));
                        }
                        InvPair(ty_r, ty_w) => {
                            let v = m.materialize_val(ty_r)?;
                            let u = m.materialize_use(ty_w)?;
                            fields.insert(name, (v, Some(u), span));
                        }
                    }
                }

                let mut aliases = HashMap::new();
                for &(name, (_, (ref tree, ref kind))) in template_aliases {
                    let p = m.materialize_pair(tree)?;
                    let binding = TypeBinding::inspect(m.core, p, kind)?;
                    aliases.insert(name, binding);
                }
                for &(name, ind) in extra_aliases {
                    aliases.insert(name, TypeBinding::Con(ind));
                }

                VTypeHead::VObj { fields, aliases }
            }

            _ => {
                return Err(ice());
            }
        })
    }

    fn materialize_head_other<'a, 'b>(
        m: &mut TreeMaterializer<'a, 'b>,
        ty: &'b ParsedType,
    ) -> Result<<Self::Opposite as Polarity>::Head, ICE> {
        Use::materialize_head(m, ty)
    }

    fn get_state<'a, 'b>(s: &'a mut TreeMaterializerState<'b>) -> &'a mut PolarState<'b, Self> {
        &mut s.state_v
    }

    fn get_state_other<'a, 'b>(s: &'a mut TreeMaterializerState<'b>) -> &'a mut PolarState<'b, Self::Opposite> {
        &mut s.state_u
    }
}

impl Materialize for Use {
    fn materialize_head<'a, 'b>(m: &mut TreeMaterializer<'a, 'b>, ty: &'b ParsedType) -> Result<UTypeHead, ICE> {
        use ParsedTypeHead::*;

        Ok(match &ty.1 {
            &Case(ref cases) => {
                let mut utype_case_arms = Vec::new();
                for &(tag, (_, ref ty)) in cases {
                    let u = m.materialize_use(ty)?;
                    utype_case_arms.push((tag, u));
                }

                UTypeHead::UCase(UCaseData::new(utype_case_arms))
            }
            &Constructor(ref tycon, ref kind, ref parsed_params) => {
                let conuse = m.materialize_use(tycon)?;

                let mut params = Vec::new();
                for pair in parsed_params {
                    use VarianceInvPair::*;
                    params.push(match pair {
                        Co(ty) => (Some(m.materialize_use(ty)?), None),
                        Contra(ty) => (None, Some(m.materialize_val(ty)?)),
                        InvSingle(ty) => {
                            let u = m.materialize_use(ty)?;
                            let v = m.materialize_val(ty)?;
                            (Some(u), Some(v))
                        }
                        InvPair(ty_r, ty_w) => {
                            let u = m.materialize_use(ty_r)?;
                            let v = m.materialize_val(ty_w)?;
                            (Some(u), Some(v))
                        }
                    });
                }
                UTypeHead::UConstructorApplication(ConstructorAppData {
                    tycon: conuse,
                    kind: kind.clone(),
                    params: ConstructorAppParams::new(params),
                })
            }
            &Func(ref arg, ref ret, prop) => {
                let arg = m.materialize_val(arg)?;
                let ret = m.materialize_use(ret)?;
                UTypeHead::UFunc { arg, ret, prop }
            }
            &Record(ref fields, _, _) => {
                let mut utype_fields = Vec::with_capacity(fields.len());
                for &(name, (span, ref param)) in fields {
                    use VarianceInvPair::*;
                    match param {
                        Co(ty) => {
                            let u = m.materialize_use(ty)?;
                            utype_fields.push((name, (u, None, span)));
                        }
                        Contra(_ty) => return Err(ice()),
                        InvSingle(ty) => {
                            let v = m.materialize_val(ty)?;
                            let u = m.materialize_use(ty)?;
                            utype_fields.push((name, (u, Some(v), span)));
                        }
                        InvPair(ty_r, ty_w) => {
                            let v = m.materialize_val(ty_w)?;
                            let u = m.materialize_use(ty_r)?;
                            utype_fields.push((name, (u, Some(v), span)));
                        }
                    }
                }
                UTypeHead::UObj { fields: utype_fields }
            }

            _ => {
                return Err(ice());
            }
        })
    }

    fn materialize_head_other<'a, 'b>(
        m: &mut TreeMaterializer<'a, 'b>,
        ty: &'b ParsedType,
    ) -> Result<<Self::Opposite as Polarity>::Head, ICE> {
        Value::materialize_head(m, ty)
    }

    fn get_state<'a, 'b>(s: &'a mut TreeMaterializerState<'b>) -> &'a mut PolarState<'b, Self> {
        &mut s.state_u
    }

    fn get_state_other<'a, 'b>(s: &'a mut TreeMaterializerState<'b>) -> &'a mut PolarState<'b, Self::Opposite> {
        &mut s.state_v
    }
}

pub struct TreeMaterializer<'a, 'b> {
    pub core: &'a mut TypeCheckerCore,
    pub s: &'a mut TreeMaterializerState<'b>,
}

impl<'a, 'b> TreeMaterializer<'a, 'b> {
    fn materialize_sub<P: Materialize>(&mut self, ty: &'b ParsedType) -> Result<P, ICE> {
        use ParsedTypeHead::*;
        Ok(match &ty.1 {
            Case(..) | Constructor(..) | Func(..) | Record(..) => {
                let head = P::materialize_head(self, ty)?;
                P::new_node(self.core, head, ty.0)
            }

            RecHead(r) => {
                // Insert a placeholder entry into core. This will be used for recursive references.
                let ph = P::placeholder(self.core);
                P::get_state(self.s).rec_types.insert(r.loc, ph);

                let ph2 = if r.rec_contravariantly {
                    let ph = P::Opposite::placeholder(self.core);
                    P::get_state_other(self.s).rec_types.insert(r.loc, ph);
                    Some(ph)
                } else {
                    None
                };

                // Now materialize the body of the recursive type and fill in the placeholder.
                let head = P::materialize_head(self, &r.body)?;
                P::set_node(self.core, ph, head, ty.0)?;

                if r.rec_contravariantly {
                    let ph2 = ph2.unwrap();
                    let head_other = P::materialize_head_other(self, &r.body)?;
                    P::Opposite::set_node(self.core, ph2, head_other, ty.0)?;
                    P::get_state_other(self.s).rec_types.remove(&r.loc);
                }

                P::get_state(self.s).rec_types.remove(&r.loc);
                ph
            }
            &RecVar(loc) => P::get_state(self.s).rec_types.get(&loc).copied().ok_or_else(ice)?,

            Any => P::any(self.core, ty.0),
            Never => P::never(self.core, ty.0),
            &Type(pair) => P::extract(pair),
            &TempPolyVar(loc, names) => {
                let name = names.get(P::is_u() ^ self.s.flip_pairs);
                match self.s.temp_poly_var_replacements.get(&(loc, name)) {
                    Some(r) => r.get(self.core, ty.0),
                    None => return Err(ice()),
                }
            }

            &SpineParam(i) => P::get_state(self.s).spine_params.get(i).copied().flatten().ok_or_else(ice)?,

            &SpinePolyVar(names) => {
                //let name = P::get_state(self.s).spine_pair_name_remap.get(&name).copied().unwrap_or(name);
                let name = names.get(P::is_u() ^ self.s.flip_pairs);

                if let Some(r) = self.s.new_spine_poly_var_replacements.get(&name) {
                    r.get(self.core, ty.0)
                } else if let Some(key) = self.s.spine_poly_default_comparison_key {
                    // Increment stat count
                    P::get_state(self.s)
                        .poly_count
                        .entry(name)
                        .and_modify(|c| *c += 1)
                        .or_insert(1);
                    P::new_poly_escape(self.core, key, name, ty.0)
                } else {
                    return Err(ice());
                }
            }
        })
    }

    pub fn materialize<P: Materialize>(&mut self, ty: &'b ParsedType) -> Result<P, ICE> {
        let key = CacheKey::new(ty);
        if let Some(&t) = P::get_state(self.s).cache.get(&key) {
            return Ok(t);
        }

        let t = self.materialize_sub::<P>(ty)?;
        P::get_state(self.s).cache.insert(key, t);
        Ok(t)
    }

    pub fn materialize_val(&mut self, ty: &'b ParsedType) -> Result<Value, ICE> {
        self.materialize::<Value>(ty)
    }

    pub fn materialize_use(&mut self, ty: &'b ParsedType) -> Result<Use, ICE> {
        self.materialize::<Use>(ty)
    }

    pub fn materialize_pair(&mut self, ty: &'b ParsedType) -> Result<(Value, Use), ICE> {
        let v = self.materialize_val(ty)?;
        let u = self.materialize_use(ty)?;
        Ok((v, u))
    }
}
