// Copyright (c) 2026 Robert Grosse. All rights reserved.
use std::collections::HashMap;
use std::iter::zip;
use std::rc::Rc;

use crate::ast::*;
use crate::coercion::*;
use crate::core::*;
use crate::exhaustiveness::MissingPattern;
use crate::kinds::kinds_are_equal;
use crate::short_str::ShortStr;
use crate::spans::*;
use crate::spines::*;
use crate::type_errors::*;
use crate::typeck::*;
use crate::unification::*;

// Index for ordinary type nodes
crate::index_type!(TypeNodeInd);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Value(pub TypeNodeInd);
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Use(pub TypeNodeInd);

pub const BOT: Value = Value(TypeNodeInd::MAX);
pub const TOP: Use = Use(TypeNodeInd::MAX);

pub trait Polarity: Sized + Copy + std::fmt::Debug {
    type Head;
    type Opposite: Polarity<Opposite = Self>;

    fn is_u() -> bool;

    fn new_node(core: &mut TypeCheckerCore, head: Self::Head, span: Span) -> Self;
    fn placeholder(core: &mut TypeCheckerCore) -> Self;
    fn set_node(core: &mut TypeCheckerCore, ph: Self, head: Self::Head, span: Span) -> Result<(), ICE>;
    fn extract(pair: (Value, Use)) -> Self;

    fn new_simple(core: &mut TypeCheckerCore, ty: TyConDefInd, span: Span) -> Self;
    fn new_poly_escape(core: &mut TypeCheckerCore, key: ComparisonKey, name: StringId, span: Span) -> Self;

    fn add_to_pair(self, pair: &mut (Value, Use));

    fn any(core: &mut TypeCheckerCore, span: Span) -> Self;
    fn never(core: &mut TypeCheckerCore, span: Span) -> Self;
}

impl Polarity for Value {
    type Head = VTypeHead;
    type Opposite = Use;

    fn is_u() -> bool {
        false
    }

    fn new_node(core: &mut TypeCheckerCore, head: VTypeHead, span: Span) -> Value {
        core.new_val(head, span)
    }
    fn placeholder(core: &mut TypeCheckerCore) -> Value {
        core.val_placeholder()
    }
    fn set_node(core: &mut TypeCheckerCore, ph: Value, head: VTypeHead, span: Span) -> Result<(), ICE> {
        core.set_val(ph, head, span)
    }
    fn extract(pair: (Value, Use)) -> Value {
        pair.0
    }

    fn new_simple(core: &mut TypeCheckerCore, ty: TyConDefInd, span: Span) -> Self {
        core.simple_val(ty, span)
    }

    fn new_poly_escape(core: &mut TypeCheckerCore, key: ComparisonKey, name: StringId, span: Span) -> Self {
        core.new_val(VTypeHead::VEphemeralPoly(EphemeralPolyType { key, name }), span)
    }

    fn add_to_pair(self, pair: &mut (Value, Use)) {
        pair.0 = self;
    }

    fn any(core: &mut TypeCheckerCore, span: Span) -> Self {
        core.new_val(VTypeHead::VTop, span)
    }
    fn never(_core: &mut TypeCheckerCore, _span: Span) -> Self {
        BOT
    }
}

impl Polarity for Use {
    type Head = UTypeHead;
    type Opposite = Value;

    fn is_u() -> bool {
        true
    }

    fn new_node(core: &mut TypeCheckerCore, head: UTypeHead, span: Span) -> Use {
        core.new_use(head, span)
    }
    fn placeholder(core: &mut TypeCheckerCore) -> Use {
        core.use_placeholder()
    }
    fn set_node(core: &mut TypeCheckerCore, ph: Use, head: UTypeHead, span: Span) -> Result<(), ICE> {
        core.set_use(ph, head, span)
    }
    fn extract(pair: (Value, Use)) -> Use {
        pair.1
    }

    fn new_simple(core: &mut TypeCheckerCore, ty: TyConDefInd, span: Span) -> Self {
        core.simple_use(ty, span, UseSrc::None)
    }

    fn new_poly_escape(core: &mut TypeCheckerCore, key: ComparisonKey, name: StringId, span: Span) -> Self {
        core.new_use(UTypeHead::UEphemeralPoly(EphemeralPolyType { key, name }), span)
    }

    fn add_to_pair(self, pair: &mut (Value, Use)) {
        pair.1 = self;
    }

    fn any(_core: &mut TypeCheckerCore, _span: Span) -> Self {
        TOP
    }
    fn never(core: &mut TypeCheckerCore, span: Span) -> Self {
        core.new_use(UTypeHead::UBot, span)
    }
}

#[derive(Debug, Clone)]
pub struct ConstructorData {
    pub category: TyConDefInd,
    pub spine: Option<Rc<SpineConstructor>>,
    pub restamp: bool,
}
impl ConstructorData {
    pub fn new(category: TyConDefInd) -> Self {
        Self {
            category,
            spine: None,
            restamp: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConstructorAppParams<P: Polarity>(pub Vec<(Option<P>, Option<P::Opposite>)>);
impl<P: Polarity> ConstructorAppParams<P> {
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    pub fn new(v: Vec<(Option<P>, Option<P::Opposite>)>) -> Self {
        Self(v)
    }
}
// For some reason, Rust can't derive this.
impl<P: Polarity> Default for ConstructorAppParams<P> {
    fn default() -> Self {
        Self::empty()
    }
}

impl ConstructorAppParams<Value> {
    pub fn check_against(
        &self,
        rhs: &ConstructorAppParams<Use>,
        edge_context: &TypeEdge,
        out: &mut Vec<(Value, Use, TypeEdge)>,
    ) {
        for (p1, p2) in zip(self.0.iter().copied(), rhs.0.iter().copied()) {
            if let (Some(v), Some(u)) = (p1.0, p2.0) {
                out.push((v, u, edge_context.clone()));
            }
            if let (Some(u), Some(v)) = (p1.1, p2.1) {
                out.push((v, u, edge_context.clone()));
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConstructorAppData<P: Polarity> {
    pub tycon: P,
    // Invariant: Kind must always be Arrow
    pub kind: SKind,
    pub params: ConstructorAppParams<P>,
}

// Used to track distinct comparison instances. Span should be the span
// of the coercion expression that triggered the comparison.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ComparisonKey(pub Span);

#[derive(Debug, Clone, Copy)]
pub struct EphemeralPolyType {
    pub key: ComparisonKey,
    pub name: StringId,
}

#[derive(Debug, Clone)]
pub struct UCaseData {
    pub cases: HashMap<StringId, Use>,
    pub case_wildcards: HashMap<StringId, Use>,
    pub wildcard: Option<Use>,
    pub exhaustive_reason: Option<MissingPattern>,
}
impl UCaseData {
    pub fn new(cases: Vec<(StringId, Use)>) -> Self {
        Self {
            cases: cases.into_iter().collect(),
            case_wildcards: HashMap::new(),
            wildcard: None,
            exhaustive_reason: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FuncProperties {
    pub is_identity: bool,
}

#[derive(Debug, Clone)]
pub enum VTypeHead {
    // Invariant: Children must be VCase
    VCaseUnion(Vec<Value>),

    VTop,
    VFunc {
        arg: Use,
        ret: Value,
        prop: FuncProperties,
    },
    VObj {
        fields: HashMap<StringId, (Value, Option<Use>, Span)>,
        aliases: HashMap<StringId, TypeBinding>,
    },
    VCase {
        case: (StringId, Value),
    },
    VConstructorApplication(ConstructorAppData<Value>),

    VTypeConstructor(ConstructorData),

    VEphemeralPoly(EphemeralPolyType),
}

#[derive(Debug, Clone)]
pub enum UTypeHead {
    // Unlike VCaseUnion, there are no type constraints on children
    UIntersection(Vec<Use>),

    UBot,
    UFunc {
        arg: Value,
        ret: Use,
        prop: FuncProperties,
    },
    UObj {
        fields: Vec<(StringId, (Use, Option<Value>, Span))>,
    },
    UCase(UCaseData),
    UConstructorApplication(ConstructorAppData<Use>),

    UTypeConstructor(ConstructorData),

    UEphemeralPoly(EphemeralPolyType),

    // Accept any structural type and pass through,
    // err on type constructors or constructor applications
    UFilterOutConstructors(Use),
}

#[derive(Debug, Clone, Copy)]
pub enum UseSrc {
    None,
    Unification(UnifiedSource),
    CallExpr,
    BinOpExpr(ShortStr),
}

pub type VTypeNode = (VTypeHead, Span);
pub type UTypeNode = (UTypeHead, Span, UseSrc);

fn type_conflict_err(src: UseSrc) -> PartialTypeError {
    if let UseSrc::Unification(uni) = src {
        uni.err()
    } else {
        PartialTypeError::TypeConflict
    }
}

pub fn check_heads(
    type_ctors: &TyConArena,
    lhs_ind: Value,
    lhs: &VTypeNode,
    rhs_ind: Use,
    rhs: &UTypeNode,
    edge_context: TypeEdge,
    out: &mut Vec<(Value, Use, TypeEdge)>,
) -> Result<(), PartialTypeError> {
    use UTypeHead::*;
    use VTypeHead::*;

    let lhs_span = lhs.1;
    let rhs_span = rhs.1;
    match (&lhs.0, &rhs.0) {
        (_, &UIntersection(ref types)) => {
            for rhs2 in types.iter().copied() {
                out.push((lhs_ind, rhs2, edge_context.clone()));
            }
        }

        (_, &UFilterOutConstructors(target)) => match (&lhs.0, rhs.2) {
            (VTypeConstructor(..) | VConstructorApplication(..), UseSrc::Unification(uni)) => {
                return Err(uni.err());
            }
            _ => {
                out.push((lhs_ind, target, edge_context));
            }
        },

        (&VCaseUnion(ref types), _) => {
            for lhs2 in types.iter().copied() {
                out.push((lhs2, rhs_ind, edge_context.clone()));
            }
        }

        // Check for basic types - the type constructors on both sides have to match.
        (
            &VFunc {
                arg: arg1,
                ret: ret1,
                prop: prop1,
            },
            &UFunc {
                arg: arg2,
                ret: ret2,
                prop: prop2,
            },
        ) => {
            if prop2.is_identity && !prop1.is_identity {
                return Err(SpannedError::new2(
                    "TypeError: Function is required to be a pure identity function here:",
                    rhs_span,
                    "Note: But it may be a regular function here:",
                    lhs_span,
                )
                .into());
            }

            // flip the order since arguments are contravariant
            out.push((arg2, arg1, edge_context.clone()));
            out.push((ret1, ret2, edge_context));
        }
        (&VObj { fields: ref fields1, .. }, &UObj { fields: ref fields2 }) => {
            // Check if the accessed field is defined
            for &(name, (rhs_r, rhs_w, rhs_span)) in fields2.iter() {
                if let Some(&(lhs_r, lhs_w, lhs_span)) = fields1.get(&name) {
                    out.push((lhs_r, rhs_r, edge_context.clone()));

                    // Check for mutability
                    if let Some(rhs_w) = rhs_w {
                        if let Some(lhs_w) = lhs_w {
                            // Contravariant
                            out.push((rhs_w, lhs_w, edge_context.clone()));
                        } else {
                            return Err(PartialTypeError::ImmutableField(lhs_span, rhs_span, name));
                        }
                    }
                } else {
                    return Err(PartialTypeError::MissingField(lhs_span, rhs_span, name));
                }
            }
        }
        (&VCase { case: (name, lhs2) }, &UCase(ref d)) => {
            // Check if the right case is handled
            if let Some(rhs2) = d.cases.get(&name).copied() {
                out.push((lhs2, rhs2, edge_context.clone()));

                // Invariant: Subset of d.cases
                // Unwrapped cases match the variant object as a whole,
                // so we pass lhs_ind rather than lhs2
                if let Some(rhs2) = d.case_wildcards.get(&name).copied() {
                    out.push((lhs_ind, rhs2, edge_context.clone()));
                }
            } else if let Some(rhs2) = d.wildcard {
                out.push((lhs_ind, rhs2, edge_context));
            } else {
                return Err(PartialTypeError::UnhandledVariant(
                    lhs_span,
                    rhs_span,
                    name,
                    d.exhaustive_reason.clone(),
                ));
            }
        }

        (&VTypeConstructor(ref lhs_data), &UTypeConstructor(ref rhs_data)) => {
            if lhs_data.category != rhs_data.category {
                return Err(type_conflict_err(rhs.2));
            }

            // Check that abstract types don't escape their scope
            let ty_def = type_ctors.get(lhs_data.category);
            if let TyConDef::Custom(ty_def) = ty_def
                && edge_context.scopelvl < ty_def.scopelvl
            {
                return Err(PartialTypeError::TypeEscape(
                    ty_def.name,
                    ty_def.span,
                    lhs_span,
                    rhs_span,
                    edge_context.scopelvl,
                ));
            }
        }

        (&VConstructorApplication(ref lhs_app), &UConstructorApplication(ref rhs_app)) => {
            // First check that the type constructors match
            if !kinds_are_equal(&lhs_app.kind.0, &rhs_app.kind.0) {
                return Err(type_conflict_err(rhs.2));
            }

            // Check the parameters
            lhs_app.params.check_against(&rhs_app.params, &edge_context, out);
            // Make sure the constructors match too.
            out.push((lhs_app.tycon, rhs_app.tycon, edge_context));
        }

        (&VEphemeralPoly(ref lhs), &UEphemeralPoly(ref rhs)) if lhs.name == rhs.name => {
            if lhs.key != rhs.key || edge_context.scopelvl != NO_HOLES {
                return Err(SpannedError::new2(
                    "TypeError: Polymorphic type variable escapes its scope. Type originates here:",
                    lhs_span,
                    "Note: And is used here after escaping its scope.",
                    rhs_span,
                )
                .into());
            }
        }

        _ => {
            return Err(type_conflict_err(rhs.2));
        }
    };
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum DottedName {
    Single(StringId),
    Pair(StringId, StringId),
}
impl DottedName {
    pub fn to_str(&self, strings: &lasso::Rodeo) -> String {
        match self {
            DottedName::Single(a) => strings.resolve(a).to_string(),
            DottedName::Pair(a, b) => format!("{}.{}", strings.resolve(a), strings.resolve(b)),
        }
    }
}

#[derive(Debug)]
pub struct CustomTyConDef {
    pub name: DottedName,
    pub span: Span,
    pub scopelvl: ScopeLvl,
    pub kind: Kind,

    // Coercions are stored as Rcs to get around the borrow checker - they're not actually shared.
    pub unwrap_coercion: Option<Rc<Coercion<Value>>>,
    // Only used for the target type of subsumption expressions.
    pub wrap_coercion: Option<Rc<Coercion<Use>>>,
}
impl CustomTyConDef {
    pub fn new(name: DottedName, span: Span, scopelvl: ScopeLvl, kind: Kind) -> Self {
        Self {
            name,
            span,
            scopelvl,
            kind,

            unwrap_coercion: None,
            wrap_coercion: None,
        }
    }
}

pub enum TyConDef {
    Builtin(StringId),
    Custom(CustomTyConDef),
    Polymorphic(Kind),
}
impl TyConDef {
    pub fn span(&self) -> Option<Span> {
        match self {
            TyConDef::Custom(a) => Some(a.span),
            _ => None,
        }
    }

    pub fn kind(&self) -> &Kind {
        match self {
            TyConDef::Custom(a) => &a.kind,
            TyConDef::Builtin(..) => &Kind::Star,
            TyConDef::Polymorphic(kind) => kind,
        }
    }
}

crate::index_type!(TyConDefInd);

pub struct TyConArena {
    defs: Vec<TyConDef>,
    // Dedupe spine constructors
    spines: HashMap<SpineStructureKey, TyConDefInd>,
}
impl TyConArena {
    pub fn new() -> Self {
        Self {
            defs: Vec::new(),
            spines: HashMap::new(),
        }
    }

    pub fn add(&mut self, def: TyConDef) -> TyConDefInd {
        let i = self.defs.len();
        self.defs.push(def);
        TyConDefInd::new(i)
    }

    pub fn add_custom(
        &mut self,
        base_name: Option<StringId>,
        name: StringId,
        span: Span,
        scopelvl: ScopeLvl,
        kind: Kind,
    ) -> TyConDefInd {
        let name = if let Some(base_name) = base_name {
            DottedName::Pair(base_name, name)
        } else {
            DottedName::Single(name)
        };
        self.add(TyConDef::Custom(CustomTyConDef::new(name, span, scopelvl, kind)))
    }

    pub fn add_polymorphic(&mut self, key: SpineStructureKey, kind: &Kind) -> TyConDefInd {
        if let Some(&ind) = self.spines.get(&key) {
            return ind;
        }

        let ind = self.add(TyConDef::Polymorphic(kind.clone()));
        self.spines.insert(key, ind);
        ind
    }

    pub fn get(&self, ind: TyConDefInd) -> &TyConDef {
        &self.defs[ind.i()]
    }

    pub fn get_spine_key(&self, ind: TyConDefInd) -> Option<&SpineStructureKey> {
        // Doesn't have to be fast because this is only used for error messages.
        for (key, &val) in self.spines.iter() {
            if val == ind {
                return Some(key);
            }
        }
        None
    }

    pub fn set_unwrap_coercion(&mut self, ind: TyConDefInd, coercion: Coercion<Value>) -> Result<(), ICE> {
        if let TyConDef::Custom(ref mut a) = self.defs[ind.i()] {
            a.unwrap_coercion = Some(Rc::new(coercion));
        } else {
            return Err(ice());
        }
        Ok(())
    }

    pub fn set_wrap_coercion(&mut self, ind: TyConDefInd, coercion: Coercion<Use>) -> Result<(), ICE> {
        if let TyConDef::Custom(ref mut a) = self.defs[ind.i()] {
            a.wrap_coercion = Some(Rc::new(coercion));
        } else {
            return Err(ice());
        }
        Ok(())
    }
}
