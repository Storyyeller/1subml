// Copyright (c) 2026 Robert Grosse. All rights reserved.
use std::collections::HashMap;
use std::collections::HashSet;

use crate::ast;
use crate::ast::*;
use crate::coercion::*;
use crate::core::ScopeLvl;
use crate::core::TypeCheckerCore;
use crate::exhaustiveness::*;
use crate::ordered_map::OrderedMap;
use crate::parse_types::*;
use crate::spans::Span;
use crate::spans::SpannedError;
use crate::spans::ice;
use crate::type_errors::DupNameChecker;
use crate::type_errors::HoleSrc;
use crate::typeck::*;
use crate::types::*;
use crate::unification::*;

type Tag = StringId;
type FieldName = StringId;
type BindingTypes = Vec<(StringId, (Span, Value))>;

pub struct RecordNode {
    span: Span,
    coercion: CoercionHintAccumulator,
    pub fields: OrderedMap<FieldName, (Span, Box<PatternNode>)>,
}
pub struct VariantNode {
    span: Span,
    coercion: CoercionHintAccumulator,
    pub cases: OrderedMap<Tag, Box<PatternNode>>,
}

fn inconsistent_pattern_error(rspan: Span, vspan: Span) -> SpannedError {
    SpannedError::new2(
        "SyntaxError: Inconsistent match patterns. The input is required to be a record here:",
        rspan,
        "Note: But it is also required to be a variant here:",
        vspan,
    )
}

pub enum PatternNodeSub {
    Leaf,
    R(RecordNode),
    V(VariantNode),
}
impl PatternNodeSub {
    fn make_record(&mut self, span: Span) -> Result<&mut RecordNode, SpannedError> {
        match self {
            PatternNodeSub::Leaf => {
                *self = PatternNodeSub::R(RecordNode {
                    span,
                    coercion: CoercionHintAccumulator::new(),
                    fields: OrderedMap::new(),
                });
                if let PatternNodeSub::R(r) = self {
                    Ok(r)
                } else {
                    Err(ice().into())
                }
            }
            PatternNodeSub::R(r) => Ok(r),
            PatternNodeSub::V(v) => Err(inconsistent_pattern_error(span, v.span)),
        }
    }

    fn make_variant(&mut self, span: Span) -> Result<&mut VariantNode, SpannedError> {
        match self {
            PatternNodeSub::Leaf => {
                *self = PatternNodeSub::V(VariantNode {
                    span,
                    coercion: CoercionHintAccumulator::new(),
                    cases: OrderedMap::new(),
                });
                if let PatternNodeSub::V(v) = self {
                    Ok(v)
                } else {
                    Err(ice().into())
                }
            }
            PatternNodeSub::R(r) => Err(inconsistent_pattern_error(r.span, span)),
            PatternNodeSub::V(v) => Ok(v),
        }
    }
}

struct BindingsAccumulator {
    bindings: BindingTypes,
    dup: DupNameChecker,
}
impl BindingsAccumulator {
    fn new() -> Self {
        Self {
            bindings: Vec::new(),
            dup: DupNameChecker::new("pattern binding"),
        }
    }

    fn add(
        &mut self,
        parser: &mut TypeParser<'_>,
        name: Option<StringId>,
        name_span: Span,
        ty: &Option<ast::STypeExpr>,
    ) -> Result<Option<Use>, SpannedError> {
        Ok(if let Some(name) = name {
            self.dup.add(name, name_span)?;
            let (v, u) = parser.parse_type_or_hole(ty.as_ref(), name_span, HoleSrc::OptAscribe(name_span))?;
            self.bindings.push((name, (name_span, v)));
            Some(u)
        } else if let Some(ty) = ty {
            Some(parser.parse_type(ty)?.1)
        } else {
            None
        })
    }
}

pub type DecisionKey = *const VariantNode;
struct PatternOut {
    bindings: BindingsAccumulator,
    decisions: Vec<(DecisionKey, Tag)>,
}
impl PatternOut {
    fn new() -> Self {
        Self {
            bindings: BindingsAccumulator::new(),
            decisions: Vec::new(),
        }
    }
}

pub struct PatternNode {
    pub sub: PatternNodeSub,
    wildcard_types: Vec<(CaseId, Use)>,
    dummy_span: Option<Span>, // Only used for the intersection node, whose span will never actually be used. Must be Some if wildcard_types is nonempty.
}
impl PatternNode {
    fn new() -> Self {
        Self {
            sub: PatternNodeSub::Leaf,
            wildcard_types: Vec::new(),
            dummy_span: None,
        }
    }

    fn add_var_pat(
        &mut self,
        pat: &ast::VarPattern,
        arm: CaseId,
        parser: &mut TypeParser<'_>,
        out: &mut PatternOut,
    ) -> Result<(), SpannedError> {
        let (name, span) = pat.0;
        let ty = &pat.1;

        let new = out.bindings.add(parser, name, span, ty)?;
        if let Some(new) = new {
            self.wildcard_types.push((arm, new));
            self.dummy_span = Some(span);
        }

        Ok(())
    }

    fn add_pattern(
        &mut self,
        pat: &ast::LetPattern,
        arm: CaseId,
        parser: &mut TypeParser<'_>,
        out: &mut PatternOut,
    ) -> Result<(), SpannedError> {
        use ast::LetPattern::*;
        match pat {
            Case(hint, tag, inner) => {
                let v = self.sub.make_variant(tag.1)?;
                v.coercion.add(hint, parser)?;
                out.decisions.push((std::ptr::from_ref(v), tag.0));
                let sub = v.cases.entry_or_insert_with(tag.0, || Box::new(PatternNode::new()));
                if let Some(inner) = inner {
                    sub.add_pattern(inner, arm, parser, out)?;
                }
            }
            Record(hint, (fields, span), as_pat) => {
                let r = self.sub.make_record(*span)?;
                r.coercion.add(hint, parser)?;
                use ast::RecordPatternMember::*;
                for (field_name, Field(field_pat)) in fields.iter() {
                    let sub = r
                        .fields
                        .entry_or_insert_with(field_name.0, || (field_name.1, Box::new(PatternNode::new())));
                    sub.1.add_pattern(field_pat, arm, parser, out)?;
                }

                if let Some(as_pat) = as_pat {
                    self.add_var_pat(as_pat, arm, parser, out)?;
                }
            }
            Var(vp) => {
                self.add_var_pat(vp, arm, parser, out)?;
            }
        }
        Ok(())
    }
}

struct CoercionHintAccumulator {
    explicit_no_coercion: bool,
    out: Option<Value>,
    inputs: Vec<Use>,
}
impl CoercionHintAccumulator {
    fn new() -> Self {
        Self {
            explicit_no_coercion: false,
            out: None,
            inputs: Vec::new(),
        }
    }

    fn add(
        &mut self,
        hint: &Option<(ast::PathTypeWithArgs, Span)>,
        parser: &mut TypeParser<'_>,
    ) -> Result<(), SpannedError> {
        if let Some((path, span)) = hint {
            if let ast::PathType::Hole = path.path.0 {
                self.explicit_no_coercion = true;
                return Ok(());
            }

            let (v, u) = parser.parse_named_type(path, *span)?;
            self.inputs.push(u);
            self.out = Some(v);
        }
        Ok(())
    }

    fn apply(
        &self,
        rhs: Use,
        core: &mut TypeCheckerCore,
        pattern_span: Span,
        is_record: bool,
        scopelvl: ScopeLvl,
    ) -> Result<Use, SpannedError> {
        let mut inputs = self.inputs.clone();

        if self.explicit_no_coercion {
            inputs.push(rhs);
        } else {
            let cb = if is_record {
                InnerCallback::Record(rhs)
            } else {
                InnerCallback::Variant(rhs)
            };
            let cb = UnwrapCoercionCallback { cb };

            if let Some(out) = self.out {
                create_unification_nodes(core, scopelvl, pattern_span, cb, out)?;
            } else {
                let u = create_pattern_unification_var(core, scopelvl, pattern_span, cb)?;
                inputs.push(u);
            }
        };
        Ok(core.intersect(inputs, pattern_span))
    }
}

fn to_type(
    pat: &PatternNode,
    core: &mut TypeCheckerCore,
    edata: &ExhaustivenessResult,
    scopelvl: ScopeLvl,
) -> Result<Use, SpannedError> {
    let mut wildcards = pat.wildcard_types.iter().map(|(_arm, ty)| *ty).collect::<Vec<_>>();

    use PatternNodeSub::*;
    Ok(match pat.sub {
        Leaf => {
            if wildcards.is_empty() {
                TOP
            } else {
                let span = pat.dummy_span.ok_or_else(ice)?;
                core.intersect(wildcards, span)
            }
        }
        R(ref r) => {
            let fields = r
                .fields
                .iter()
                .map(|(field_name, (field_span, subpat))| {
                    let subty = to_type(subpat, core, edata, scopelvl)?;
                    Ok((*field_name, (subty, None, *field_span)))
                })
                .collect::<Result<Vec<_>, SpannedError>>()?;
            let u = core.obj_use(fields, r.span);
            wildcards.push(u);

            let u = core.intersect(wildcards, r.span);
            r.coercion.apply(u, core, r.span, true, scopelvl)?
        }
        V(ref v) => {
            let key = std::ptr::from_ref(v);

            let cases = v
                .cases
                .iter()
                .map(|(tag, subpat)| {
                    let subty = to_type(subpat, core, edata, scopelvl)?;
                    Ok((*tag, subty))
                })
                .collect::<Result<HashMap<_, _>, SpannedError>>()?;

            let empty_set = HashSet::new();
            let covers = edata.covers.get(&key).unwrap_or(&empty_set);

            let mut intersection_cache = HashMap::new();

            let mut case_wildcards = HashMap::new();
            for tag in v.cases.keys.iter().copied() {
                let hits = pat
                    .wildcard_types
                    .iter()
                    .copied()
                    .filter_map(|(arm, ty)| if covers.contains(&(arm, tag)) { None } else { Some(ty) })
                    .collect::<Vec<_>>();

                let u = *intersection_cache
                    .entry(hits)
                    .or_insert_with_key(|hits| core.intersect(hits.clone(), v.span));

                case_wildcards.insert(tag, u);
            }

            let exhaustive_reason = edata.nonexhaustive_wildcard_reasons.get(&key).cloned();
            let wildcard = if exhaustive_reason.is_some() {
                None
            } else {
                Some(
                    *intersection_cache
                        .entry(wildcards)
                        .or_insert_with_key(|hits| core.intersect(hits.clone(), v.span)),
                )
            };

            let head = UCaseData {
                cases,
                case_wildcards,
                wildcard,
                exhaustive_reason,
            };
            let head = UTypeHead::UCase(head);
            let u = core.new_use(head, v.span);
            v.coercion.apply(u, core, v.span, false, scopelvl)?
        }
    })
}

// An arm may have multiple cases, each with different bindings
// So we need to track which bindings are defined in all cases for an arm, and take the union of their types.
struct BindingList(Span, Option<Vec<Value>>);
pub struct MergedBindingsForArm {
    m: HashMap<StringId, BindingList>,
}
impl MergedBindingsForArm {
    fn new() -> Self {
        Self { m: HashMap::new() }
    }

    fn add_first(&mut self, bindings: &BindingTypes) {
        for &(name, (span, v)) in bindings.iter() {
            self.m.insert(name, BindingList(span, Some(vec![v])));
        }
    }

    fn add_subsequent(&mut self, bindings: &BindingTypes) {
        let temp = bindings.iter().copied().collect::<HashMap<_, _>>();

        for (name, list) in self.m.iter_mut() {
            if let Some((_, v)) = temp.get(name) {
                if let Some(vals) = &mut list.1 {
                    vals.push(*v);
                }
            } else {
                list.1 = None;
            }
        }
    }

    pub fn add_bindings(
        self,
        vars: &mut BindingMap<ValueBinding>,
        core: &mut TypeCheckerCore,
        body_span: Span,
        scopelvl: ScopeLvl,
    ) {
        for (name, BindingList(bind_span, vals)) in self.m.into_iter() {
            if let Some(vals) = vals {
                let merged_type = if vals.len() == 1 {
                    vals[0]
                } else {
                    // Multiple cases, so we need to add a hole to "union" the types.
                    let src = HoleSrc::LetRebind(body_span, name);
                    let (v, u) = core.var(src, scopelvl);
                    for val in vals {
                        core.flow(val, u, bind_span);
                    }
                    v
                };
                vars.insert(name, ValueBinding::new(merged_type, bind_span));
            } else {
                // Var was defined in some cases but not others, so we need to prevent it from being used at all in the body to avoid confusion.
                vars.hide(name, bind_span);
            }
        }
    }
}

pub struct ParsedArm<'a> {
    // Guards are checked with per-case bindings, so we need to preserve them for later checking
    pub case_results: Vec<(BindingTypes, &'a SExpr)>,

    pub merged_bindings: MergedBindingsForArm,
    pub body: &'a SExpr,
}

pub fn parse_match_cases<'a>(
    me: &'a ast::expr::MatchExpr,
    parser: &mut TypeParser<'_>,
) -> Result<(Use, Vec<ParsedArm<'a>>), SpannedError> {
    let mut pattern_tree = PatternNode::new();

    // Used for exhaustiveness checking.
    let mut cases = Vec::new();
    // Output results (used by typeck)
    let mut arms = Vec::new();
    for arm in me.arms.iter() {
        let mut case_results = Vec::new();
        let mut is_first_case = true;

        let mut merged_bindings = MergedBindingsForArm::new();

        for case in arm.cases.iter() {
            let case_id = CaseId::new(cases.len());
            let mut out = PatternOut::new();
            pattern_tree.add_pattern(&case.pattern.0, case_id, parser, &mut out)?;

            let bindings = out.bindings.bindings;
            cases.push((case.pattern.1, case.guard.is_some(), out.decisions));

            if is_first_case {
                merged_bindings.add_first(&bindings);
                is_first_case = false;
            } else {
                merged_bindings.add_subsequent(&bindings);
            }

            if let Some(guard) = case.guard.as_deref() {
                case_results.push((bindings, guard));
            }
        }

        arms.push(ParsedArm {
            case_results,
            merged_bindings,
            body: &arm.expr,
        });
    }

    let edata = check_exhaustiveness(me.expr.1, parser.strings, &pattern_tree, cases)?;

    let ty = to_type(&pattern_tree, parser.core, &edata, parser.scopelvl)?;
    Ok((ty, arms))
}

pub fn parse_unconditional_match(
    pat: &ast::LetPattern,
    parser: &mut TypeParser<'_>,
) -> Result<(Use, BindingTypes), SpannedError> {
    let mut merged = PatternNode::new();

    let mut out = PatternOut::new();
    merged.add_pattern(pat, CaseId::MAX, parser, &mut out)?;

    let edata = ExhaustivenessResult {
        covers: HashMap::new(),
        nonexhaustive_wildcard_reasons: HashMap::new(),
    };

    let ty = to_type(&merged, parser.core, &edata, parser.scopelvl)?;
    Ok((ty, out.bindings.bindings))
}
