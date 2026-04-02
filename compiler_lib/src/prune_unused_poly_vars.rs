// Copyright (c) 2026 Robert Grosse. All rights reserved.
use crate::ast::*;
use crate::kinds::KindVar;
use crate::spans::*;
use crate::templates::*;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;

#[derive(Debug, Clone, Copy)]
enum Replacement {
    Any,
    Never,
}

struct VarReplacements(HashMap<StringId, Replacement>, SourceLoc);
impl VarReplacements {
    fn new(loc: SourceLoc) -> Self {
        Self(HashMap::new(), loc)
    }

    fn add(&mut self, name: StringId, r: Replacement) {
        self.0.insert(name, r);
    }
}
impl TemplateVisitorMut for VarReplacements {
    type Err = ICE;

    fn visit_postorder(&mut self, tree: &mut ParsedTypeHead, context: WalkMutContext<'_>) -> Result<(), Self::Err> {
        use ParsedTypeHead::*;
        use Variance::*;
        match tree {
            TempPolyVar(loc, names) if *loc == self.1 => {
                let name = names.get(context.variance == Contravariant);
                if let Some(r) = self.0.get(&name) {
                    *tree = match r {
                        Replacement::Any => Any,
                        Replacement::Never => Never,
                    };
                }
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn prune_unused_poly_vars(
    strings: &lasso::Rodeo,
    loc: SourceLoc,
    ty: &mut RcParsedType,
    tparams: &mut Vec<(StringId, Span, SKind)>,
) -> Result<(), ICE> {
    let usage = PolyVarUses::new(Some(loc)).walk(ty);

    // For functions, covariant -> never, contravariant -> any.
    // For records, flip that since functions are instantiated on
    // the Value side, while records are instantiated on the Use side.
    let is_record = matches!(ty.1, ParsedTypeHead::Record(..));
    let mut vars_to_replace = VarReplacements::new(loc);
    let mut aliases_to_add = Vec::new();
    tparams.retain(|(name, span, kind)| {
        use Replacement::*;
        if usage.pair_vars.contains(name) {
            return true; // members of a pair can never be replaced
        }

        if usage.cov.contains_key(name) && usage.contra.contains_key(name) {
            true
        } else if usage.cov.contains_key(name) {
            // Only used covariantly, so replace with never if func or any if record.
            let r = if is_record { Any } else { Never };
            vars_to_replace.add(*name, r);
            aliases_to_add.push((*name, *span, kind.clone(), ParsedTypeHead::Any));
            false
        } else if usage.contra.contains_key(name) {
            // Only used contravariantly, so replace with any if func or never if record.
            let r = if is_record { Never } else { Any };
            vars_to_replace.add(*name, r);
            aliases_to_add.push((*name, *span, kind.clone(), ParsedTypeHead::Never));
            false
        } else {
            // Completely unused - since it's unused, there's no need to do a tree walk to replace it.
            false
        }
    });

    // Perform variable replacements, if any.
    if !vars_to_replace.0.is_empty() {
        vars_to_replace.walk(ty, loc)?;
    }

    // Add type aliases for removed variables,
    if is_record && !aliases_to_add.is_empty() {
        let ty = Rc::make_mut(ty);
        if let ParsedTypeHead::Record(_, aliases, _) = &mut ty.1 {
            for (name, span, kind, head) in aliases_to_add {
                aliases.push((name, (span, (Rc::new((span, head)), KindVar::Known(kind)))));
            }
            // Now that we've added aliases, we need to re-sort them to maintain invariant.
            aliases.sort_by_key(|(name, _)| strings.resolve(name));
        } else {
            return Err(ice());
        }
    }

    Ok(())
}

pub struct PolyVarUses {
    loc: Option<SourceLoc>,

    pub cov: HashMap<StringId, Span>,
    pub contra: HashMap<StringId, Span>,
    pub pair_vars: HashSet<StringId>,
}
impl PolyVarUses {
    pub fn new(loc: Option<SourceLoc>) -> Self {
        Self {
            loc,

            cov: HashMap::new(),
            contra: HashMap::new(),
            pair_vars: HashSet::new(),
        }
    }

    fn add(&mut self, names: NamePair, span: Span, variance: Variance) {
        use Variance::*;
        match names {
            NamePair::Single(name) => {
                if variance != Contravariant {
                    self.cov.insert(name, span);
                }
                if variance != Covariant {
                    self.contra.insert(name, span);
                }
            }
            NamePair::Pair(name1, name2) => {
                if variance != Contravariant {
                    self.cov.insert(name1, span);
                }
                if variance != Covariant {
                    self.contra.insert(name2, span);
                }
                if variance == Invariant {
                    self.pair_vars.insert(name1);
                    self.pair_vars.insert(name2);
                }
            }
        }
    }

    pub fn get_co(&self, name: StringId) -> Option<Span> {
        self.cov.get(&name).copied()
    }

    pub fn get_contra(&self, name: StringId) -> Option<Span> {
        self.contra.get(&name).copied()
    }
}
impl TemplateVisitor for PolyVarUses {
    type Out = Self;

    fn visit_leaf(&mut self, head: &ParsedTypeHead, span: Span, variance: Variance) -> Result<(), Self::Out> {
        match head {
            &ParsedTypeHead::SpinePolyVar(names) => self.add(names, span, variance),
            &ParsedTypeHead::TempPolyVar(loc, names) => {
                if self.loc == Some(loc) {
                    self.add(names, span, variance);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(self) -> Self::Out {
        self
    }
}
