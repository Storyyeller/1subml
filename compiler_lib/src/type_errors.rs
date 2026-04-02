// Copyright (c) 2026 Robert Grosse. All rights reserved.
use std::collections::HashMap;
use std::collections::HashSet;

use crate::ast::*;
use crate::core::*;
use crate::exhaustiveness::*;
use crate::introspect_types::*;
use crate::kinds::*;
use crate::spans::*;
use crate::spines::*;
use crate::types::*;

pub struct DupNameChecker(HashMap<StringId, Span>, &'static str);
impl DupNameChecker {
    pub fn new(err_type: &'static str) -> Self {
        Self(HashMap::new(), err_type)
    }

    pub fn add(&mut self, name: StringId, span: Span) -> Result<(), SpannedError> {
        if let Some(old_span) = self.0.insert(name, span) {
            Err(SpannedError::new2(
                format!("SyntaxError: Repeated {} name.", self.1),
                span,
                "Note: Conflicts with previous definition here:",
                old_span,
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum HoleSrc {
    /// An explicit _ in a type annotation in the source code.
    Explicit(Span),
    /// Insert :_ after span (pattern or mut field with missing optional annotation)
    OptAscribe(Span),
    /// Same as OptAscribe, but higher priority.
    FuncArgAscribe(Span),
    // span :: _
    ReturnTypeAscribe(Span),
    // span[_]
    IdentityAscribe(Span),

    /// Wrap the given expr in an explicit type annotation
    CheckedExpr(Span),
    // _ span - Use is used to filter out useless hints later
    AddPatternCoerceHint(Span, Use),

    // span with [name=_]
    AddSubstitutions(Span, StringId),
    // span; name=_
    AddSubstitutionToEnd(Span, StringId),
    // (span :> _ -> _ with [name=_])
    AddFuncCoerce(Span, StringId),

    // (let name: _ = name; span)
    LetRebind(Span, StringId),
}
impl HoleSrc {
    /// Used to select which hint to display for type errors.
    pub fn priority(&self) -> usize {
        use HoleSrc::*;
        match self {
            Explicit(..) => 100,
            ReturnTypeAscribe(..) | FuncArgAscribe(..) => 95,
            OptAscribe(..) => 85,
            AddPatternCoerceHint(..) | IdentityAscribe(..) | AddSubstitutionToEnd(..) => 70,

            AddSubstitutions(..) => 21,
            CheckedExpr(..) => 20,
            AddFuncCoerce(..) => 10,
            LetRebind(..) => 0,
        }
    }

    pub fn add_to_error(&self, e: &mut SpannedError, strings: &lasso::Rodeo) {
        use HoleSrc::*;
        match self {
            Explicit(span) => e.push_span(*span),
            FuncArgAscribe(span) | OptAscribe(span) => e.push_insert("", *span, ": _"),
            ReturnTypeAscribe(span) => e.push_insert("", *span, " :: _"),
            IdentityAscribe(span) => e.push_insert("", *span, "[_]"),
            CheckedExpr(span) => e.push_insert("(", *span, ": _)"),
            AddPatternCoerceHint(span, ..) => e.push_insert("_ ", *span, ""),

            AddSubstitutions(span, name) => e.push_insert("", *span, format!(" with [{}=_]", strings.resolve(name))),
            AddSubstitutionToEnd(span, name) => e.push_insert("", *span, format!("; {}=_", strings.resolve(name))),
            AddFuncCoerce(span, name) => {
                e.push_insert("(", *span, format!(" :> _ -> _ with [{}=_])", strings.resolve(name)))
            }

            LetRebind(span, name) => {
                let name = strings.resolve(name);
                e.push_insert(format!("(let {}: _ = {}; ", name, name), *span, ")");
            }
        }
    }
}

pub enum PartialTypeError {
    E(SpannedError),

    MissingField(Span, Span, StringId),
    ImmutableField(Span, Span, StringId),

    UnhandledVariant(Span, Span, StringId, Option<MissingPattern>),

    TypeEscape(DottedName, Span, Span, Span, ScopeLvl),
    UnificationConflict(Value),

    TypeConflict,
}
impl PartialTypeError {
    fn get_base_error(self, core: &TypeCheckerCore, strings: &lasso::Rodeo, pair: (Value, Use)) -> SpannedError {
        use PartialTypeError::*;
        match self {
            E(e) => e,
            MissingField(lhs_span, rhs_span, name) => {
                let mut e = SpannedError::new1(format!("TypeError: Missing field {}.", strings.resolve(&name)), rhs_span);
                e.push("Note: Field is required here:", rhs_span);
                e.push("Note: But the record is defined without that field here:", lhs_span);
                e
            }
            ImmutableField(lhs_span, rhs_span, name) => {
                let mut e = SpannedError::new1(
                    format!("TypeError: Cannot set immutable field {}.", strings.resolve(&name)),
                    rhs_span,
                );
                e.push("Note: Field is required to be mutable here:", rhs_span);
                e.push("Note: But the record is defined with that field immutable here:", lhs_span);
                e
            }

            TypeEscape(name, ty_span, lhs_span, rhs_span, _) => SpannedError::new3(
                format!("TypeError: Type {} defined here escapes its scope.", name.to_str(strings)),
                ty_span,
                "Note: A value of this type originates here:",
                lhs_span,
                "Note: And is consumed here after escaping the defining scope:",
                rhs_span,
            ),
            UnhandledVariant(lhs_span, rhs_span, name, reason) => {
                let mut e =
                    SpannedError::new1(format!("TypeError: Unhandled variant {}.", strings.resolve(&name)), lhs_span);
                if let Some(reason) = reason {
                    e.push(
                        format!(
                            "Note: Variant may not be handled here. For example, the match pattern {} may not be covered.",
                            reason.pattern
                        ),
                        rhs_span,
                    );
                    reason.print_ignored(&mut e);
                } else {
                    e.push("Note: Variant may not be handled here:", rhs_span);
                }
                e
            }
            UnificationConflict(lhs1) => {
                let res = unification_conflict_err(core, strings, lhs1, pair.0, pair.1);
                match res {
                    Ok(e) => e,
                    Err(e) => e.into(),
                }
            }

            TypeConflict => {
                let res = type_category_mismatch_err(core, strings, pair.0, pair.1);
                match res {
                    Ok(e) => e,
                    Err(e) => e.into(),
                }
            }
        }
    }

    pub fn finish(self, core: &TypeCheckerCore, strings: &lasso::Rodeo, pair: (Value, Use)) -> SpannedError {
        use PartialTypeError::*;

        let scopelvl = match &self {
            TypeEscape(_, _, _, _, lvl) => *lvl,
            _ => NO_HOLES,
        };
        let conflicting_src = match &self {
            UnificationConflict(lhs1, ..) => Some(*lhs1),
            _ => None,
        };

        let mut e = self.get_base_error(core, strings, pair);

        // First follow the FlowReasons backwards to get a list of holes (inference variables) and
        // roots involved in the detected type contradiction.
        let (mut holes, roots) = backtrack_hole_list(core, pair);

        if let Some(v) = conflicting_src {
            // For unification errors, we have two different source values. Only keep holes that are reachable from both. Note that the unification node itself will always be included at the end of both, so we will always have at least one hole in common.
            let (holes2, _) = backtrack_hole_list(core, (v, pair.1));
            let hole_inds = holes2.into_iter().map(|t| t.0).collect::<HashSet<_>>();
            holes.retain(|(i, _)| hole_inds.contains(i));
        }
        // Now remove the hole indexes
        let mut holes = holes.into_iter().map(|(_, v)| v).collect::<Vec<_>>();

        // Pattern coerce hints aren't like normal type inference vars. They can indicate the abscence of
        // a coercion, in which case it is not helpful to show them to the user. Therefore, we filter thme out.
        holes.retain(|v| {
            if let HoleSrc::AddPatternCoerceHint(_, u) = v.src {
                // Keep a hole if a) it points to a real coercion (i.e. not UFilterOutConstructors)
                // or b) it points to the source of the current type conflict (in which case the the
                // conflicting type could be a type/type constructor, which would force us to keep it.)
                let is_passthrough = matches!(
                    core.r.get(u.0),
                    Some(TypeNode::Use((UTypeHead::UFilterOutConstructors(..), ..)))
                );
                if is_passthrough && u != pair.1 {
                    return false;
                }
            }

            true
        });

        // println!("{:?} holes before filtering {:?}", scopelvl, holes);

        // For type escape errors, only consider holes in outer scopes
        holes.retain(|v| v.scopelvl <= scopelvl);
        // println!("{:?} found {} holes {:?}", pair, holes.len(), holes);

        let n = holes.len();
        let best = holes
            .into_iter()
            .enumerate()
            .max_by_key(|&(i, v)| v.src.priority() + i * (n - 1 - i));

        if let Some(hole) = best {
            e.push_str(
                "Hint: To narrow down the cause of the type mismatch, consider adding an explicit type annotation here:",
            );

            hole.1.src.add_to_error(&mut e, strings);
        } else {
            // If there were no type inference variables we could hint for, try flow roots instead
            // Skip showing roots that were already chosen as a span in the error message.
            let spans = e.spans().collect::<HashSet<_>>();

            for span in roots {
                if !spans.contains(&span) {
                    e.push("Note: Type mismatch was detected starting from this expression:", span);
                    break;
                }
            }
        }

        e
    }
}

impl From<SpannedError> for PartialTypeError {
    fn from(err: SpannedError) -> Self {
        PartialTypeError::E(err)
    }
}
impl From<ICE> for PartialTypeError {
    fn from(err: ICE) -> Self {
        let spanned_err: SpannedError = err.into();
        spanned_err.into()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////

fn add_hole(core: &TypeCheckerCore, holes_list: &mut Vec<(TypeNodeInd, InferenceVarData)>, h: TypeNodeInd) {
    match core.r.get(h) {
        Some(&TypeNode::Var(data, ..)) => holes_list.push((h, data)),

        _ => unreachable!(),
    }
}

fn backtrack_hole_list_sub(
    core: &TypeCheckerCore,
    seen: &mut HashSet<(Value, Use)>,
    holes_list: &mut Vec<(TypeNodeInd, InferenceVarData)>,
    roots_list: &mut Vec<Span>,
    mut pair: (Value, Use),
) {
    while !seen.contains(&pair) {
        // println!("checking {} {}", pair.0.0.0, pair.1.0.0);
        seen.insert(pair);
        let reason = core.r.get_edge(pair.0.0, pair.1.0).unwrap().reason;
        // println!("reason {:?}", reason);
        match reason {
            FlowReason::Root(span) => {
                roots_list.push(span);
                break;
            }
            FlowReason::Transitivity(h) => {
                backtrack_hole_list_sub(core, seen, holes_list, roots_list, (pair.0, Use(h)));
                add_hole(core, holes_list, h);
                pair = (Value(h), pair.1);
            }
            FlowReason::Check(v, u) => pair = (v, u),
        }
    }
}

fn backtrack_hole_list(core: &TypeCheckerCore, pair: (Value, Use)) -> (Vec<(TypeNodeInd, InferenceVarData)>, Vec<Span>) {
    let mut seen = HashSet::new();
    let mut holes_list = Vec::new();
    let mut roots_list = Vec::new();
    backtrack_hole_list_sub(core, &mut seen, &mut holes_list, &mut roots_list, pair);
    (holes_list, roots_list)
}

////////////////////////////////////////////////////////////////////////////////////////////

fn v_dissect_err(core: &TypeCheckerCore, v: Value) -> Result<DissectedTypeSpan<'_>, ICE> {
    use TypeErrCategory::*;
    use VTypeHead::*;
    // Invariant: v must be Value that points to a normal type node
    let (vhead, span) = core.get_vhead(v)?;
    Ok(DissectedTypeSpan(
        match vhead {
            VCaseUnion(_) => DissectedTypeForErr::new1(Variant),
            VTop => DissectedTypeForErr::new1(Any),
            VFunc { .. } => DissectedTypeForErr::new1(Func),
            VObj { .. } => DissectedTypeForErr::new1(Record),
            VCase { .. } => DissectedTypeForErr::new1(Variant),
            VConstructorApplication(data) => {
                let kind = Some(&data.kind.0);
                match core.get_val_or_hole(data.tycon)? {
                    ValOrHole::Never => DissectedTypeForErr::new3(Never, kind, None),
                    ValOrHole::Hole(_) => DissectedTypeForErr::new3(Hole, kind, None),
                    ValOrHole::Val(vhead, _) => match vhead {
                        VTop => DissectedTypeForErr::new3(Any, kind, None),
                        VTypeConstructor(data) => constructor_err(core, data, kind)?,
                        VEphemeralPoly(data) => DissectedTypeForErr::new1(Polymorphic(data.name)),
                        _ => return Err(ice()),
                    },
                }
            }
            VTypeConstructor(data) => constructor_err(core, data, None)?,
            VEphemeralPoly(data) => DissectedTypeForErr::new1(Polymorphic(data.name)),
        },
        span,
    ))
}

fn u_dissect_err(core: &TypeCheckerCore, u: Use) -> Result<DissectedTypeSpan<'_>, ICE> {
    use TypeErrCategory::*;
    use UTypeHead::*;
    // Invariant: u must be Use that points to a normal type node
    let (uhead, span, src) = core.get_uhead(u)?;
    let mut new = DissectedTypeSpan(
        match uhead {
            UBot => DissectedTypeForErr::new1(Never),
            UFunc { .. } => DissectedTypeForErr::new1(Func),
            UObj { .. } => DissectedTypeForErr::new1(Record),
            UCase { .. } => DissectedTypeForErr::new1(Variant),
            UConstructorApplication(data) => {
                let kind = Some(&data.kind.0);
                match core.get_use_or_hole(data.tycon)? {
                    UseOrHole::Any => DissectedTypeForErr::new3(Any, kind, None),
                    UseOrHole::Hole(_) => DissectedTypeForErr::new3(Hole, kind, None),
                    UseOrHole::Unification => DissectedTypeForErr::new3(Unknown, kind, None),
                    UseOrHole::Use(uhead, _, _src) => match uhead {
                        UBot => DissectedTypeForErr::new3(Never, kind, None),
                        UEphemeralPoly(data) => DissectedTypeForErr::new1(Polymorphic(data.name)),
                        UTypeConstructor(data) => constructor_err(core, data, kind)?,
                        _ => return Err(ice()),
                    },
                }
            }
            UTypeConstructor(data) => constructor_err(core, data, None)?,
            UEphemeralPoly(data) => DissectedTypeForErr::new1(Polymorphic(data.name)),
            UFilterOutConstructors(..) | UIntersection(..) => DissectedTypeForErr::new1(Unknown),
        },
        span,
    );
    new.0.use_src = Some(src);
    Ok(new)
}

fn constructor_err<'a>(
    core: &'a TypeCheckerCore,
    ctor: &'a ConstructorData,
    kind: Option<&'a Kind>,
) -> Result<DissectedTypeForErr<'a>, ICE> {
    use TypeErrCategory::*;

    let cdef = core.tycons.get(ctor.category);
    Ok(match cdef {
        TyConDef::Builtin(name) => DissectedTypeForErr::new3(Builtin(*name), kind, None),
        TyConDef::Custom(data) => DissectedTypeForErr::new1(Named(data.name, data.span)),
        TyConDef::Polymorphic(_) => {
            let spine = ctor.spine.as_deref().ok_or_else(ice)?;
            let category = match spine.template {
                SpineContents::Func(..) => Func,
                SpineContents::Record(..) => Record,
            };

            let spine_key = core.tycons.get_spine_key(ctor.category).ok_or_else(ice)?;
            DissectedTypeForErr::new3(category, None, Some(spine_key))
        }
    })
}

#[derive(Debug)]
enum TypeErrCategory {
    Never,
    Any,
    Func,
    Record,
    Variant,
    Builtin(StringId),
    Named(DottedName, Span),
    Polymorphic(StringId),
    Hole,
    Unknown,
}
#[derive(Debug)]
struct DissectedTypeForErr<'a> {
    category: TypeErrCategory,
    kind: Option<&'a Kind>,
    spine: Option<&'a SpineStructureKey>,
    use_src: Option<UseSrc>,
}
impl<'a> DissectedTypeForErr<'a> {
    fn new1(category: TypeErrCategory) -> Self {
        Self::new3(category, None, None)
    }

    fn new3(category: TypeErrCategory, kind: Option<&'a Kind>, spine: Option<&'a SpineStructureKey>) -> Self {
        Self {
            category,
            kind,
            spine,
            use_src: None,
        }
    }

    fn initial_msg(&self, strings: &lasso::Rodeo) -> TMsg {
        use TypeErrCategory::*;
        match self.category {
            Never => have_ty("never"),
            Any => have_ty("any"),
            Func => be_a("function"),
            Record => be_a("record"),
            Variant => be_a("variant"),
            Named(name, _) => TMsg::HaveTy(name.to_str(strings), None),
            Builtin(name) | Polymorphic(name) => have_ty(strings.resolve(&name)),
            Hole => have_ty("_"),
            Unknown => be_a("???"), // Probably unreachable, so don't sweat it too much.
        }
    }

    fn elaborate_mono_vs_poly(&self, m: &mut TMsg) {
        use TypeErrCategory::*;
        match self.category {
            Record => {
                *m = be_a(if self.spine.is_some() {
                    "polymorphic record"
                } else {
                    "monomorphic record"
                })
            }
            Func => {
                *m = be_a(if self.spine.is_some() {
                    "polymorphic function"
                } else {
                    "monomorphic function"
                })
            }
            _ => {}
        }
    }

    fn elaborate_named_origin(&self, m: &mut TMsg) {
        use TypeErrCategory::*;
        if let TMsg::HaveTy(_s, origin) = m {
            match self.category {
                Any | Never | Builtin(_) => *origin = Some(TyOrigin::Builtin),
                Named(_, def_span) => *origin = Some(TyOrigin::Custom(def_span)),
                Polymorphic(_) => *origin = Some(TyOrigin::Polymorphic),
                _ => {}
            }
        }
    }

    fn elaborate_kind(&self, m: &mut TMsg) {
        if let TMsg::HaveTy(s, origin) = m {
            // Only add kind annotations for builtin types (any/never) or inference vars.
            if matches!(origin, None | Some(TyOrigin::Builtin)) {
                let kind = self.kind.unwrap_or(&Kind::Star);
                *s = format!("{} with kind {}", s, print_kind(kind));
                *origin = None;
            }
        }
    }
}

struct DissectedTypeSpan<'a>(DissectedTypeForErr<'a>, Span);
impl DissectedTypeSpan<'_> {
    fn initial_msg(&self, strings: &lasso::Rodeo) -> TMsgSpan {
        let msg = self.0.initial_msg(strings);
        let extra = match self.0.use_src {
            Some(UseSrc::CallExpr) => Some("function call".to_string()),
            Some(UseSrc::BinOpExpr(s)) => Some(format!("{} operator", s.as_str())),
            _ => None,
        };
        TMsgSpan {
            msg,
            span: self.1,
            extra,
        }
    }
}

fn get_elaborated_messages(
    t1: DissectedTypeSpan<'_>,
    t2: DissectedTypeSpan<'_>,
    strings: &lasso::Rodeo,
) -> (TMsgSpan, TMsgSpan) {
    let mut m1 = t1.initial_msg(strings);
    let mut m2 = t2.initial_msg(strings);

    if m1.msg == m2.msg {
        t1.0.elaborate_mono_vs_poly(&mut m1.msg);
        t2.0.elaborate_mono_vs_poly(&mut m2.msg);

        t1.0.elaborate_named_origin(&mut m1.msg);
        t2.0.elaborate_named_origin(&mut m2.msg);
    }

    if m1.msg == m2.msg {
        // If spine types are still equal at this point, it means we have two distinct spine types with the same caetgory. Just display the keys in this case.
        if let (Some(s1), Some(s2)) = (t1.0.spine, t2.0.spine) {
            m1.msg = have_ty(s1);
            m2.msg = have_ty(s2);
        } else {
            // By this point, the only possible difference left should be the kinds. Add kinds for builtin types or holes.
            t1.0.elaborate_kind(&mut m1.msg);
            t2.0.elaborate_kind(&mut m2.msg);
        }
    }

    (m1, m2)
}

#[derive(Debug, PartialEq, Eq)]
enum TyOrigin {
    Builtin,
    Custom(Span),
    Polymorphic,
}

#[derive(Debug, PartialEq, Eq)]
enum TMsg {
    BeA(String),
    HaveTy(String, Option<TyOrigin>),
}
impl TMsg {
    fn print(&self) -> String {
        match self {
            TMsg::BeA(s) => format!("be a {}", s),
            TMsg::HaveTy(s, _) => format!("have type {}", s),
        }
    }

    fn add_origin(&self, e: &mut SpannedError) {
        if let TMsg::HaveTy(s, Some(origin)) = self {
            match origin {
                TyOrigin::Builtin => e.push_str(format!("Note: {} is a builtin type.", s)),
                TyOrigin::Custom(span) => e.push(format!("Note: {} is the type defined here:", s), *span),
                TyOrigin::Polymorphic => e.push_str(format!("Note: {} is a polymorphic type variable.", s)),
            }
        }
    }
}

fn be_a(s: &str) -> TMsg {
    TMsg::BeA(s.to_owned())
}
fn have_ty(s: &str) -> TMsg {
    TMsg::HaveTy(s.to_owned(), None)
}

struct TMsgSpan {
    msg: TMsg,
    span: Span,
    extra: Option<String>,
}
impl TMsgSpan {
    fn print(&self, e: &mut SpannedError, first: &str) {
        let extra = self
            .extra
            .as_ref()
            .map(|s| format!(" by the {} expression", s))
            .unwrap_or_default();
        let msg = format!("{} {}{} here:", first, self.msg.print(), extra);
        e.push(msg, self.span);
        self.msg.add_origin(e);
    }
}

fn type_category_mismatch_err(
    core: &TypeCheckerCore,
    strings: &lasso::Rodeo,
    lhs: Value,
    rhs: Use,
) -> Result<SpannedError, ICE> {
    let lhs = v_dissect_err(core, lhs)?;
    let rhs = u_dissect_err(core, rhs)?;
    let (found, expected) = get_elaborated_messages(lhs, rhs, strings);

    let mut e = SpannedError::new();
    expected.print(&mut e, "TypeError: Value is required to");
    found.print(&mut e, "However, that value may");

    if let TMsg::BeA(s) = expected.msg
        && s == "monomorphic record"
    {
        e.push_str("Hint: To access fields on a polymorphic record, you need to first bind it as a module. (mod M = ...)");
    }

    Ok(e)
}

fn unification_conflict_err(
    core: &TypeCheckerCore,
    strings: &lasso::Rodeo,
    lhs1: Value,
    lhs2: Value,
    rhs: Use,
) -> Result<SpannedError, ICE> {
    let lhs1 = v_dissect_err(core, lhs1)?;
    let lhs2 = v_dissect_err(core, lhs2)?;
    let (_, rhs_span, _) = core.get_uhead(rhs)?;

    let (found1, found2) = get_elaborated_messages(lhs1, lhs2, strings);

    let mut e = SpannedError::new1("TypeError: Incompatible types here:", rhs_span);
    found2.print(&mut e, "Note: The value may");
    found1.print(&mut e, "However, that value may also");
    Ok(e)
}
