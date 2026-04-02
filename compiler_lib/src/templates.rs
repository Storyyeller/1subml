// Copyright (c) 2026 Robert Grosse. All rights reserved.
use crate::ast;
use crate::ast::Kind;
use crate::ast::StringId;
use crate::ast::Variance;
use crate::kinds::KindVar;
use crate::spans::*;
use crate::types::*;
use std::iter::zip;
use std::rc::Rc;

// Used to track where a local type was defined (as a polymorphic type variable or
// a recursive type), when dealing with nested polymorphic and/or recursive types.
// Invariant: Real values are always > 0. 0 is reserved for "no location".
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceLoc(pub u32);

#[derive(Debug, Clone)]
pub enum VarianceInvPair {
    Co(RcParsedType),
    Contra(RcParsedType),
    InvSingle(RcParsedType),
    InvPair(RcParsedType, RcParsedType),
}

#[derive(Debug, Clone)]
pub struct RecHeadData {
    pub loc: SourceLoc,
    /// Whether the recursive type variable is used contravariantly
    pub rec_contravariantly: bool,
    pub body: RcParsedType,
}

#[derive(Debug, Clone, Copy)]
pub enum NamePair {
    Single(StringId),
    Pair(StringId, StringId),
}
impl NamePair {
    pub fn get(&self, flip: bool) -> StringId {
        match self {
            NamePair::Single(name) => *name,
            NamePair::Pair(name1, name2) => {
                if flip {
                    *name2
                } else {
                    *name1
                }
            }
        }
    }

    pub fn flip(&self) -> Self {
        match self {
            NamePair::Single(name) => NamePair::Single(*name),
            NamePair::Pair(name1, name2) => NamePair::Pair(*name2, *name1),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ParsedTypeHead {
    // Invariant: vec is sorted by tag
    Case(Vec<(StringId, (Span, RcParsedType))>),
    Func(RcParsedType, RcParsedType, FuncProperties),
    // Invariant: vec is sorted by tag
    Record(
        Vec<(StringId, (Span, VarianceInvPair))>,
        Vec<(StringId, (Span, (RcParsedType, KindVar)))>,
        // extra aliases: Only used temporarily during mod instantiation.
        Vec<(StringId, TyConDefInd)>,
    ),
    // Invariant: Kind must always be Arrow
    Constructor(RcParsedType, ast::SKind, Vec<VarianceInvPair>),
    // Invariant: sub node is always Case, Func, Record, or Constructor
    RecHead(RecHeadData),
    RecVar(SourceLoc),

    // Not used by spine nodes
    Type((Value, Use)),
    Any,
    Never,

    // Only used by intermediate parsed types prior to spine construction
    TempPolyVar(SourceLoc, NamePair),

    // Only used by spine nodes
    SpinePolyVar(NamePair),
    SpineParam(usize),
}
pub type ParsedType = (Span, ParsedTypeHead);
pub type RcParsedType = Rc<ParsedType>;

pub trait TemplateVisitor: Sized {
    type Out;

    fn visit_leaf(&mut self, head: &ParsedTypeHead, span: Span, variance: Variance) -> Result<(), Self::Out>;

    fn finish(self) -> Self::Out;

    // Convenience method: Not intended to be overriden.
    fn walk(mut self, tree: &ParsedType) -> Self::Out {
        if let Err(out) = walk_tree_sub(&mut self, tree, Variance::Covariant) {
            return out;
        }
        self.finish()
    }
}
fn walk_inv_pair<T: TemplateVisitor>(visitor: &mut T, pair: &VarianceInvPair, variance: Variance) -> Result<(), T::Out> {
    match pair {
        VarianceInvPair::Co(r) => walk_tree_sub(visitor, r, variance),
        VarianceInvPair::Contra(r) => walk_tree_sub(visitor, r, variance.flip()),
        VarianceInvPair::InvSingle(r) => walk_tree_sub(visitor, r, Variance::Invariant),
        VarianceInvPair::InvPair(r, w) => {
            walk_tree_sub(visitor, r, variance)?;
            walk_tree_sub(visitor, w, variance.flip())
        }
    }
}

fn walk_tree_sub<T: TemplateVisitor>(visitor: &mut T, tree: &ParsedType, variance: Variance) -> Result<(), T::Out> {
    use ParsedTypeHead::*;
    match tree.1 {
        Case(ref branches) => {
            for (_, (_span, branch)) in branches.iter() {
                walk_tree_sub(visitor, branch, variance)?;
            }
        }
        Func(ref arg, ref ret, _) => {
            walk_tree_sub(visitor, arg, variance.flip())?;
            walk_tree_sub(visitor, ret, variance)?;
        }
        Record(ref fields, ref aliases, _) => {
            for (_, (_, param)) in fields.iter() {
                walk_inv_pair(visitor, param, variance)?;
            }
            for (_, (_, (tree, _))) in aliases.iter() {
                walk_tree_sub(visitor, tree, Variance::Invariant)?;
            }
        }
        Constructor(ref con, _, ref params) => {
            walk_tree_sub(visitor, con, variance)?;
            for param in params.iter() {
                walk_inv_pair(visitor, param, variance)?;
            }
        }
        RecHead(ref r) => {
            let position = if r.rec_contravariantly {
                Variance::Invariant
            } else {
                variance
            };
            walk_tree_sub(visitor, &r.body, position)?;
        }

        _ => {
            visitor.visit_leaf(&tree.1, tree.0, variance)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecUsageResult {
    Contravariant,
    Covariant,
    Unused,
}
pub fn get_rec_head_usage(tree: &RcParsedType, var: SourceLoc) -> RecUsageResult {
    struct RecUsageVisitor {
        var: SourceLoc,
        seen: bool,
    }
    impl TemplateVisitor for RecUsageVisitor {
        type Out = RecUsageResult;

        fn visit_leaf(&mut self, head: &ParsedTypeHead, _span: Span, variance: Variance) -> Result<(), Self::Out> {
            if let ParsedTypeHead::RecVar(loc) = head
                && *loc == self.var
            {
                self.seen = true;
                if variance != Variance::Covariant {
                    return Err(RecUsageResult::Contravariant);
                }
            }
            Ok(())
        }

        fn finish(self) -> Self::Out {
            if self.seen {
                RecUsageResult::Covariant
            } else {
                RecUsageResult::Unused
            }
        }
    }

    RecUsageVisitor { var, seen: false }.walk(tree)
}

#[derive(Debug, Clone, Copy)]
pub struct WalkMutContext<'a> {
    pub variance: Variance,
    pub kind: &'a Kind,
    pub top_rec_var: SourceLoc,
}
impl WalkMutContext<'_> {
    fn flip(&self) -> Self {
        let mut m = *self;
        m.variance = m.variance.flip();
        m
    }

    fn invariant(&self) -> Self {
        let mut m = *self;
        m.variance = Variance::Invariant;
        m
    }

    fn with_kind<'a>(&self, kind: &'a Kind) -> WalkMutContext<'a> {
        WalkMutContext {
            variance: self.variance,
            kind,
            top_rec_var: self.top_rec_var,
        }
    }
}

pub trait TemplateVisitorMut: Sized {
    type Err;

    fn visit_preorder(&mut self, _tree: &mut RcParsedType, _context: WalkMutContext<'_>) -> Result<bool, Self::Err> {
        Ok(false)
    }
    fn visit_postorder(&mut self, tree: &mut ParsedTypeHead, context: WalkMutContext<'_>) -> Result<(), Self::Err>;

    // Convenience method: Not intended to be overriden.
    fn walk(&mut self, tree: &mut RcParsedType, source_loc: SourceLoc) -> Result<(), Self::Err> {
        let context = WalkMutContext {
            variance: Variance::Covariant,
            kind: &Kind::Star,
            top_rec_var: source_loc,
        };
        walk_tree_mut_sub(self, tree, context)
    }
}

fn walk_inv_pair_mut<T: TemplateVisitorMut>(
    visitor: &mut T,
    pair: &mut VarianceInvPair,
    context: WalkMutContext<'_>,
) -> Result<(), T::Err> {
    match pair {
        VarianceInvPair::Co(r) => walk_tree_mut_sub(visitor, r, context),
        VarianceInvPair::Contra(r) => walk_tree_mut_sub(visitor, r, context.flip()),
        VarianceInvPair::InvSingle(r) => walk_tree_mut_sub(visitor, r, context.invariant()),
        VarianceInvPair::InvPair(r, w) => {
            walk_tree_mut_sub(visitor, r, context)?;
            walk_tree_mut_sub(visitor, w, context.flip())
        }
    }
}

fn walk_tree_mut_sub<T: TemplateVisitorMut>(
    visitor: &mut T,
    tree: &mut RcParsedType,
    mut context: WalkMutContext<'_>,
) -> Result<(), T::Err> {
    if visitor.visit_preorder(tree, context)? {
        return Ok(());
    }
    let tree = Rc::make_mut(tree);

    use ParsedTypeHead::*;
    match &mut tree.1 {
        Case(branches) => {
            for (_, (_span, branch)) in branches.iter_mut() {
                walk_tree_mut_sub(visitor, branch, context)?;
            }
        }
        Func(arg, ret, _) => {
            walk_tree_mut_sub(visitor, arg, context.flip())?;
            walk_tree_mut_sub(visitor, ret, context)?;
        }
        Record(fields, aliases, _) => {
            for (_, (_, param)) in fields.iter_mut() {
                walk_inv_pair_mut(visitor, param, context)?;
            }
            for (_, (_, (tree, _))) in aliases.iter_mut() {
                walk_tree_mut_sub(visitor, tree, context.invariant())?;
            }
        }
        Constructor(con, con_kind, params) => {
            walk_tree_mut_sub(visitor, con, context.with_kind(&con_kind.0))?;
            for (param, svkind) in zip(params, con_kind.0.params()) {
                walk_inv_pair_mut(visitor, param, context.with_kind(&svkind.kind.0))?;
            }
        }
        RecHead(r) => {
            if r.rec_contravariantly {
                context.variance = Variance::Invariant;
            };
            context.top_rec_var = r.loc;
            walk_tree_mut_sub(visitor, &mut r.body, context)?;
        }

        _ => {
            // visitor.visit_leaf(&tree.1, tree.0, context)?;
        }
    }
    visitor.visit_postorder(&mut tree.1, context)?;
    Ok(())
}
