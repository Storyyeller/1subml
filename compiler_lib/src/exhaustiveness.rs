// Copyright (c) 2026 Robert Grosse. All rights reserved.
use crate::ast::StringId;
use crate::ordered_map::OrderedMap;
use crate::parse_patterns::*;
use crate::print_patterns::*;
use crate::spans::*;
use std::collections::HashMap;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct MissingPattern {
    pub pattern: String,
    ignored: Option<(Span, SplitHint)>,
}
impl MissingPattern {
    pub fn print_ignored(&self, err: &mut SpannedError) {
        if let &Some((ignored, ref hint)) = &self.ignored {
            err.push("Note: That case is covered by the match pattern here, but precise exhaustiveness checking requires decomposable patterns. Consider splitting this match pattern into multiple cases.", ignored);

            match hint {
                SplitHint::Pat(pat) => err.push_str(format!(
                    "Hint: Split the above case into multiple cases by adding explicit tags at position {}.",
                    pat
                )),
            }
        }
    }
}
#[derive(Debug, Clone)]
enum SplitHint {
    Pat(String),
}

crate::index_type!(DPathId);
crate::index_type!(CaseId);

struct DecisionNode {
    path: Vec<PathComponent>,
    key: DecisionKey,

    required_tags: Vec<StringId>,
    children: Vec<DPathId>,
}

struct DNodes {
    nodes: Vec<DecisionNode>,
    current_path: Vec<PathComponent>,
    top_level: Vec<DPathId>,
}
impl DNodes {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            current_path: Vec::new(),
            top_level: Vec::new(),
        }
    }

    fn visit_root(&mut self, root: &PatternNode) {
        self.visit_pattern(root, usize::MAX);
    }

    fn visit_pattern(&mut self, p: &PatternNode, parent: usize) {
        match &p.sub {
            PatternNodeSub::Leaf => {}
            PatternNodeSub::R(r) => self.visit_record(r, parent),
            PatternNodeSub::V(v) => self.visit_variant(v, parent),
        }
    }

    fn visit_record(&mut self, p: &RecordNode, parent: usize) {
        for (field, sub) in p.fields.iter() {
            self.current_path.push(PathComponent::Field(*field));
            self.visit_pattern(&sub.1, parent);
            self.current_path.pop();
        }
    }

    fn visit_variant(&mut self, p: &VariantNode, parent: usize) {
        let id = DPathId::new(self.nodes.len());
        self.nodes.push(DecisionNode {
            path: self.current_path.clone(),
            key: std::ptr::from_ref(p),
            required_tags: p.cases.keys.clone(),
            children: Vec::new(),
        });
        // Lack of a parent is indicated by parent=usize max, in which case we add to self.top_level instead.
        let parent_list = self
            .nodes
            .get_mut(parent)
            .map(|n| &mut n.children)
            .unwrap_or(&mut self.top_level);
        parent_list.push(id);

        for (tag, sub) in p.cases.iter() {
            self.current_path.push(PathComponent::Case(*tag));
            self.visit_pattern(sub, id.i());
            self.current_path.pop();
        }
    }
}

struct Case {
    span: Span,
    tags: HashMap<DPathId, StringId>,
    covers: HashSet<(DPathId, StringId)>,
    has_guard: bool,
}

struct ExhaustivenessChecker<'a> {
    // constant data
    match_span: Span,
    strings: &'a lasso::Rodeo,
    dnodes: Vec<DecisionNode>,
    cases: Vec<Case>,
    tag_here_str: StringId,

    // mutable state used during recursive checking
    context_stack: Vec<(DPathId, StringId)>,
    ignored_cases: Vec<(CaseId, SplitHint)>,

    // output data
    nonexhaustive_wildcard_reasons: HashMap<DecisionKey, MissingPattern>,
}
impl<'a> ExhaustivenessChecker<'a> {
    fn print_example_pattern(&self) -> String {
        let mut merger = MergedPathNode::new();
        for (p, t) in self.context_stack.iter() {
            merger.merge_path(&self.dnodes[p.i()].path, *t);
        }
        merger.print(self.strings)
    }

    fn get_ignored_match(&self) -> Option<(Span, SplitHint)> {
        for (i, hint) in self.ignored_cases.iter() {
            let case = &self.cases[i.i()];
            // Note use of map_or(true)
            // The ignored case matches if the tag is the same *or* it has a wildcard at that position.
            if self
                .context_stack
                .iter()
                .all(|(p, t)| case.tags.get(p).is_none_or(|t2| t2 == t))
            {
                return Some((case.span, hint.clone()));
            }
        }
        None
    }

    fn get_missing_pattern_expl(&self) -> MissingPattern {
        let pattern = self.print_example_pattern();
        let ignored = self.get_ignored_match();
        MissingPattern { pattern, ignored }
    }

    fn check(&mut self, mut active_paths: Vec<DPathId>, mut cases: Vec<CaseId>) -> Result<(), SpannedError> {
        // If any case is all wildcards, return ok
        if cases.iter().any(|a| {
            let case = &self.cases[a.i()];
            active_paths.iter().all(|p| !case.tags.contains_key(p))
        }) {
            return Ok(());
        }

        // For each activate path, determine the number of ignores when splitting on it, and find
        // the best. At the same time, remove any paths where no cases have tags.
        let mut best = None;
        active_paths.retain(|p| {
            let mut tags = HashSet::new();
            for a in cases.iter() {
                if let Some(tag) = self.cases[a.i()].tags.get(p) {
                    tags.insert(*tag);
                }
            }
            if tags.is_empty() {
                return false; // prune from active paths
            }

            let mut num_ignores = 0;
            for a in cases.iter() {
                let case = &self.cases[a.i()];
                if case.tags.contains_key(p) {
                    continue;
                }

                for tag in tags.iter() {
                    if case.covers.contains(&(*p, *tag)) {
                        num_ignores += 1;
                    }
                }
            }
            if best.is_none_or(|(_, best_num)| num_ignores < best_num) {
                best = Some((*p, num_ignores));
            }
            true
        });

        let (p, _) = best.ok_or_else(ice)?;
        // Update active paths
        active_paths.retain(|ap| *ap != p);
        active_paths.extend(self.dnodes[p.i()].children.iter());

        // Generate a pattern string for use in error messages for non-decomposable patterns.
        let printed_p = {
            let mut merger = MergedPathNode::new();
            merger.merge_path(&self.dnodes[p.i()].path, self.tag_here_str);
            merger.print(self.strings)
        };

        // Split off cases with tags, keeping wildcards in original list.
        //
        // Note: wildcard cases are deliberately NOT included in the per-tag recursive calls
        // below. Including them would cause exponential blowup because each wildcard case
        // would be duplicated into every tag branch at every split level. Instead, wildcard
        // cases are added to the `ignored_cases` list. When a non-exhaustiveness error is
        // reported, `get_ignored_match` checks if any ignored case actually covers the
        // counterexample and adds a note suggesting the user split the case for precise checking.
        // This means the checker may report false positives (claiming non-exhaustive when
        // wildcard cases do cover the case), but the note explains the situation to the user.
        let mut cases_by_tag = OrderedMap::new();
        cases.retain(|a| {
            let case = &self.cases[a.i()];
            if let Some(tag) = case.tags.get(&p) {
                cases_by_tag.entry_or_insert_with(*tag, Vec::new).push(*a);
                false // remove
            } else {
                true // keep in list
            }
        });

        if cases.is_empty() {
            for t in self.dnodes[p.i()].required_tags.iter() {
                if !cases_by_tag.m.contains_key(t) {
                    self.context_stack.push((p, *t));
                    let mp = self.get_missing_pattern_expl();

                    let mut err = SpannedError::new1("SyntaxError: Match expression is not exhaustive.", self.match_span);
                    err.push_str(format!("Note: For example, {} may not be covered.", mp.pattern));
                    mp.print_ignored(&mut err);
                    return Err(err);
                }
            }

            // Store message explaining why match at this position had no wildcards.
            let key = self.dnodes[p.i()].key;
            if !self.nonexhaustive_wildcard_reasons.contains_key(&key) {
                self.nonexhaustive_wildcard_reasons
                    .insert(key, self.get_missing_pattern_expl());
            }
        }

        // Now recursively check each tag
        let ignore_len = self.ignored_cases.len();
        for (t, new_cases) in cases_by_tag.into_iter() {
            self.context_stack.push((p, t));
            // Add in ignored wildcards
            for a in cases.iter() {
                let case = &self.cases[a.i()];
                if !case.covers.contains(&(p, t)) {
                    let hint = SplitHint::Pat(printed_p.clone());
                    self.ignored_cases.push((*a, hint));
                }
            }

            self.check(active_paths.clone(), new_cases)?;
            self.context_stack.pop();
            self.ignored_cases.truncate(ignore_len);
        }

        // And recursively check wildcards
        if !cases.is_empty() {
            self.check(active_paths, cases)?;
        }

        Ok(())
    }
}

pub struct ExhaustivenessResult {
    pub nonexhaustive_wildcard_reasons: HashMap<DecisionKey, MissingPattern>,
    pub covers: HashMap<DecisionKey, HashSet<(CaseId, StringId)>>,
}

pub fn check_exhaustiveness(
    match_span: Span,
    strings: &mut lasso::Rodeo,
    merged_tree: &PatternNode,
    input_cases: Vec<(Span, bool, Vec<(DecisionKey, StringId)>)>,
) -> Result<ExhaustivenessResult, SpannedError> {
    let mut dnodes = DNodes::new();
    dnodes.visit_root(merged_tree);

    let mut rmap = HashMap::new();
    for (i, node) in dnodes.nodes.iter().enumerate() {
        rmap.insert(node.key, DPathId::new(i));
    }

    // Normalize and compute covers
    let mut cases: Vec<Case> = Vec::new();
    for (span, has_guard, decisions) in input_cases {
        // Convert node pointers to dpathids.
        let tags = decisions
            .into_iter()
            .map(|(k, t)| {
                let p = rmap.get(&k).ok_or_else(ice)?;
                Ok::<_, ICE>((*p, t))
            })
            .collect::<Result<HashMap<_, _>, _>>()?;

        // Check for covers among previous cases
        let mut covers = HashSet::new();
        for prev_arm in cases.iter() {
            if prev_arm.has_guard {
                continue;
            }

            let mut extra_pairs = Vec::new();
            let mut mismatch = false;
            for (p, t) in prev_arm.tags.iter() {
                if let Some(t2) = tags.get(p) {
                    if t != t2 {
                        mismatch = true;
                        break;
                    }
                } else {
                    extra_pairs.push((*p, *t));
                }
            }
            if mismatch {
                continue;
            }

            if extra_pairs.is_empty() {
                return Err(SpannedError::new2(
                    "SyntaxError: Match case is unreachable.",
                    span,
                    "Note: All values are covered by previous match case here:",
                    prev_arm.span,
                ));
            } else if extra_pairs.len() == 1 {
                covers.insert(extra_pairs[0]);
            }
        }

        cases.push(Case {
            span,
            tags,
            covers,
            has_guard,
        });
    }

    let unguarded_cases_ids = cases
        .iter()
        .enumerate()
        .filter_map(|(i, a)| if !a.has_guard { Some(CaseId::new(i)) } else { None })
        .collect();

    let mut checker = ExhaustivenessChecker {
        tag_here_str: strings.get_or_intern("<TAGS HERE>"),
        match_span,
        strings,
        dnodes: dnodes.nodes,
        cases,
        context_stack: Vec::new(),
        ignored_cases: Vec::new(),
        nonexhaustive_wildcard_reasons: HashMap::new(),
    };
    checker.check(dnodes.top_level, unguarded_cases_ids)?;

    // Convert covers to expected format
    let mut covers = HashMap::new();
    for (i, case) in checker.cases.iter().enumerate() {
        let id = CaseId::new(i);
        for (dpath_id, tag) in case.covers.iter().copied() {
            let key = checker.dnodes[dpath_id.i()].key;
            covers.entry(key).or_insert_with(HashSet::new).insert((id, tag));
        }
    }

    Ok(ExhaustivenessResult {
        nonexhaustive_wildcard_reasons: checker.nonexhaustive_wildcard_reasons,
        covers,
    })
}
