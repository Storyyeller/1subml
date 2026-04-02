// Copyright (c) 2026 Robert Grosse. All rights reserved.
use crate::ast::StringId;
use crate::reachability;
use crate::reachability::*;
use crate::spans::*;
use crate::type_errors::HoleSrc;
use crate::types::*;
use crate::unification::UnificationCallback;

/// Tracks which types were in scope when a given hole was created
/// This is an integer which is incremeneted whenever one or more
/// types are added to the bindings.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScopeLvl(u32);
impl ScopeLvl {
    pub const MIN: Self = ScopeLvl(0);

    pub fn inc(&mut self) {
        self.0 += 1;
    }

    pub fn incremented(&self) -> Self {
        ScopeLvl(self.0 + 1)
    }
}
pub const NO_HOLES: ScopeLvl = ScopeLvl(u32::MAX);

#[derive(Debug, Clone, Copy)]
pub struct InferenceVarData {
    pub scopelvl: ScopeLvl,
    pub src: HoleSrc,
}

//////////////////////////////////////////////////////////////////////////////////////////////
#[derive(Debug)]
pub enum TypeNode {
    Var(InferenceVarData),
    Value(VTypeNode),
    Use(UTypeNode),
    LazyUse(Option<UnificationCallback>, Span),

    // Invariant: No placeholders exist when flow() is called, so they are never present during type checking.
    Placeholder,
}

/// Used to track the reason a flow edge was added so we can backtrack when printing errors
#[derive(Debug, Clone, Copy)]
pub enum FlowReason {
    Root(Span),
    Transitivity(TypeNodeInd),
    Check(Value, Use),
}

#[derive(Debug, Clone)]
pub struct TypeEdge {
    pub scopelvl: ScopeLvl,
    pub reason: FlowReason,
}
impl TypeEdge {
    pub fn root(span: Span) -> Self {
        Self {
            scopelvl: NO_HOLES,
            reason: FlowReason::Root(span),
        }
    }
}
impl EdgeDataTrait<TypeNodeInd, TypeNode> for TypeEdge {
    fn with_intermediate_node(&self, hole: &TypeNode, ind: TypeNodeInd, connecting_edge: &Self) -> Self {
        let mut scopelvl = std::cmp::min(self.scopelvl, connecting_edge.scopelvl);
        if let TypeNode::Var(d) = hole {
            scopelvl = std::cmp::min(scopelvl, d.scopelvl);
        }

        Self {
            scopelvl,
            reason: FlowReason::Transitivity(ind),
        }
    }

    fn update(&mut self, other: &Self) -> bool {
        let mut changed = false;
        if other.scopelvl < self.scopelvl {
            self.scopelvl = other.scopelvl;
            self.reason = other.reason;
            changed = true;
        }
        changed
    }
}
//////////////////////////////////////////////////////////////////////////////////////////////

pub struct TypeCheckerCore {
    pub r: reachability::Reachability<TypeNodeInd, TypeNode, TypeEdge>,
    pub tycons: TyConArena,
    pub pending_edges: Vec<(Value, Use, TypeEdge)>,

    mutation_log: Vec<(TypeNodeInd, UnificationCallback)>,
}
impl TypeCheckerCore {
    pub fn new() -> Self {
        Self {
            r: Reachability::new(),
            tycons: TyConArena::new(),
            pending_edges: Vec::new(),

            mutation_log: Vec::new(),
        }
    }
    pub fn add_builtin_type(&mut self, name: StringId) -> TyConDefInd {
        self.tycons.add(TyConDef::Builtin(name))
    }

    pub fn add_pending_edge(&mut self, lhs: Value, rhs: Use, edge_context: TypeEdge) {
        self.pending_edges.push((lhs, rhs, edge_context));
    }

    pub fn flow(&mut self, lhs: Value, rhs: Use, expl_span: Span) {
        self.add_pending_edge(lhs, rhs, TypeEdge::root(expl_span));
    }

    pub fn run_pending_checks(&mut self, strings: &lasso::Rodeo) -> Result<(), SpannedError> {
        // Reverse the initial stack to process the oldest edges first.
        self.pending_edges.reverse();
        let mut type_pairs_to_check = Vec::new();
        while let Some((lhs, rhs, edge_context)) = self.pending_edges.pop() {
            // println!(" pending_edge: {}->{}", lhs.0.0, rhs.0.0);
            // Check for top/bottom types
            if lhs == BOT || rhs == TOP {
                continue;
            }

            self.r.add_edge(lhs.0, rhs.0, edge_context, &mut type_pairs_to_check);

            // Check if adding that edge resulted in any new type pairs needing to be checked
            while let Some((lhs_ind, rhs_ind, mut edge_context)) = type_pairs_to_check.pop() {
                let mut refs = self.r.get_mut_pair(lhs_ind, rhs_ind).ok_or_else(ice)?;

                let lhs = Value(lhs_ind);
                let rhs = Use(rhs_ind);

                edge_context.reason = FlowReason::Check(lhs, rhs);
                if let (TypeNode::Value(_lhs_head), TypeNode::LazyUse(cb, rhs_span)) = refs {
                    let rhs_span = *rhs_span;

                    let cb = cb.take().ok_or_else(ice)?;
                    if rhs_ind < self.r.rewind_mark {
                        self.mutation_log.push((rhs_ind, cb.clone()));
                    }

                    let (new_head, src) = cb.run(self, lhs, edge_context.clone())?;
                    *self.r.get_mut(rhs.0).ok_or_else(ice)? = TypeNode::Use((new_head, rhs_span, UseSrc::Unification(src)));

                    // After running the cb and replacing rhs with a real UTypeHead, we need to
                    // reborrow the refs so we can run the normal check below.
                    refs = self.r.get_mut_pair(lhs_ind, rhs_ind).ok_or_else(ice)?;
                }

                if let (TypeNode::Value(lhs_head), TypeNode::Use(rhs_head)) = refs {
                    let res = check_heads(
                        &self.tycons,
                        lhs,
                        lhs_head,
                        rhs,
                        rhs_head,
                        edge_context,
                        &mut self.pending_edges,
                    );
                    if let Err(e) = res {
                        return Err(e.finish(self, strings, (lhs, rhs)));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn new_val(&mut self, val_type: VTypeHead, span: Span) -> Value {
        Value(self.r.add_node(TypeNode::Value((val_type, span))))
    }

    pub fn new_use(&mut self, use_type: UTypeHead, span: Span) -> Use {
        self.new_use_with_src(use_type, span, UseSrc::None)
    }

    pub fn new_use_with_src(&mut self, use_type: UTypeHead, span: Span, src: UseSrc) -> Use {
        Use(self.r.add_node(TypeNode::Use((use_type, span, src))))
    }

    pub fn var(&mut self, src: HoleSrc, scopelvl: ScopeLvl) -> (Value, Use) {
        let data = InferenceVarData { scopelvl, src };
        let i = self.r.add_node(TypeNode::Var(data));
        (Value(i), Use(i))
    }

    pub fn simple_val(&mut self, ty: TyConDefInd, span: Span) -> Value {
        let ty = ConstructorData::new(ty);
        self.new_val(VTypeHead::VTypeConstructor(ty), span)
    }
    pub fn simple_use(&mut self, ty: TyConDefInd, span: Span, src: UseSrc) -> Use {
        let ty = ConstructorData::new(ty);
        self.new_use_with_src(UTypeHead::UTypeConstructor(ty), span, src)
    }
    pub fn simple(&mut self, ty: TyConDefInd, span: Span) -> (Value, Use) {
        (self.simple_val(ty, span), self.simple_use(ty, span, UseSrc::None))
    }

    pub fn simple_restamp(&mut self, ty: TyConDefInd, span: Span) -> (Value, Use) {
        let mut ty = ConstructorData::new(ty);
        ty.restamp = true;

        let v = self.new_val(VTypeHead::VTypeConstructor(ty.clone()), span);
        let u = self.new_use(UTypeHead::UTypeConstructor(ty), span);
        (v, u)
    }

    pub fn obj_use(&mut self, fields: Vec<(StringId, (Use, Option<Value>, Span))>, span: Span) -> Use {
        self.new_use(UTypeHead::UObj { fields }, span)
    }

    // span should never actually be used, but we have to include one anyway
    pub fn intersect(&mut self, mut v: Vec<Use>, span: Span) -> Use {
        v.sort_unstable();
        v.dedup();
        if v.last() == Some(&TOP) {
            v.pop();
        }

        if v.is_empty() {
            return TOP;
        } else if v.len() == 1 {
            return v.pop().unwrap();
        }

        self.new_use(UTypeHead::UIntersection(v), span)
    }

    pub fn add_hole_if_higher_priority(&mut self, ty: Value, src: HoleSrc, scopelvl: ScopeLvl, span: Span) -> Value {
        if let Some(TypeNode::Var(d)) = self.r.get(ty.0)
            && src.priority() > d.src.priority()
        {
            let (v, u) = self.var(src, scopelvl);
            self.flow(ty, u, span);
            return v;
        }

        ty
    }

    ///////////////////////////////////////////////////////////////////////////////////////////

    pub fn ephemeral(&mut self, d: EphemeralPolyType, span: Span) -> (Value, Use) {
        let v = self.new_val(VTypeHead::VEphemeralPoly(d), span);
        let u = self.new_use(UTypeHead::UEphemeralPoly(d), span);
        (v, u)
    }

    pub fn val_placeholder(&mut self) -> Value {
        Value(self.r.add_node(TypeNode::Placeholder))
    }
    pub fn use_placeholder(&mut self) -> Use {
        Use(self.r.add_node(TypeNode::Placeholder))
    }
    pub fn set_val(&mut self, ph: Value, head: VTypeHead, span: Span) -> Result<(), ICE> {
        let r = self.r.get_mut(ph.0).ok_or_else(ice)?;
        if let TypeNode::Placeholder = *r {
            *r = TypeNode::Value((head, span));
        } else {
            return Err(ice());
        }
        Ok(())
    }
    pub fn set_use(&mut self, ph: Use, head: UTypeHead, span: Span) -> Result<(), ICE> {
        let r = self.r.get_mut(ph.0).ok_or_else(ice)?;
        if let TypeNode::Placeholder = *r {
            *r = TypeNode::Use((head, span, UseSrc::None));
        } else {
            return Err(ice());
        }
        Ok(())
    }
    pub fn set_unification_callback(&mut self, ph: Use, cb: UnificationCallback, span: Span) -> Result<(), ICE> {
        let r = self.r.get_mut(ph.0).ok_or_else(ice)?;
        if let TypeNode::Placeholder = *r {
            *r = TypeNode::LazyUse(Some(cb), span);
        } else {
            return Err(ice());
        }
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////////
    pub fn save(&mut self) {
        // Todo: also save tycons?
        self.r.save();
    }
    pub fn revert(&mut self) {
        self.pending_edges.clear();
        self.r.revert();

        for (i, cb) in self.mutation_log.drain(..) {
            if let Some(r) = self.r.get_mut(i)
                && let TypeNode::Use((_, span, _)) = r
            {
                let cb = cb.clone();
                *r = TypeNode::LazyUse(Some(cb), *span);
            }
        }
    }
    pub fn make_permanent(&mut self) {
        self.r.make_permanent();
        self.mutation_log.clear();
    }
}
