// Copyright (c) 2026 Robert Grosse. All rights reserved.
use crate::ordered_map::OrderedMap;
use crate::vec_index::NodeIndex;

pub trait EdgeDataTrait<I: NodeIndex, ExtNodeData>: Clone {
    fn update(&mut self, other: &Self) -> bool;
    fn with_intermediate_node(&self, hole: &ExtNodeData, ind: I, connecting_edge: &Self) -> Self;
}

struct ReachabilityNode<I: NodeIndex, ExtNodeData, ExtEdgeData> {
    data: ExtNodeData,
    flows_from: OrderedMap<I, ExtEdgeData>,
    flows_to: OrderedMap<I, ExtEdgeData>,
}
impl<I: NodeIndex, N, E> ReachabilityNode<I, N, E> {
    fn fix_and_truncate(&mut self, i: I) {
        self.flows_from.retain(|&k| k < i);
        self.flows_to.retain(|&k| k < i);
    }
}

pub struct Reachability<I: NodeIndex, ExtNodeData, ExtEdgeData> {
    nodes: Vec<ReachabilityNode<I, ExtNodeData, ExtEdgeData>>,

    // Nodes past this point may be reverted in case of a type error
    // Value of 0 indicates no mark is set (or if a mark is set, there's nothing to do anyway)
    pub rewind_mark: I,
    journal: Vec<(I, I, Option<ExtEdgeData>)>,
}
impl<I: NodeIndex, ExtNodeData, ExtEdgeData: EdgeDataTrait<I, ExtNodeData>> Reachability<I, ExtNodeData, ExtEdgeData> {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            rewind_mark: I::from_usize(0),
            journal: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn get(&self, i: I) -> Option<&ExtNodeData> {
        self.nodes.get(i.to_usize()).map(|rn| &rn.data)
    }
    pub fn get_mut(&mut self, i: I) -> Option<&mut ExtNodeData> {
        self.nodes.get_mut(i.to_usize()).map(|rn| &mut rn.data)
    }
    pub fn get_mut_pair(&mut self, lhs: I, rhs: I) -> Option<(&mut ExtNodeData, &mut ExtNodeData)> {
        if lhs > rhs {
            let (a, b) = self.get_mut_pair(rhs, lhs)?;
            return Some((b, a));
        }

        let (left, right) = self.nodes.split_at_mut_checked(rhs.to_usize())?;
        Some((&mut left.get_mut(lhs.to_usize())?.data, &mut right.get_mut(0)?.data))
    }

    pub fn get_edge(&self, lhs: I, rhs: I) -> Option<&ExtEdgeData> {
        self.nodes.get(lhs.to_usize()).and_then(|rn| rn.flows_to.m.get(&rhs))
    }

    pub fn add_node(&mut self, data: ExtNodeData) -> I {
        let i = self.len();

        let n = ReachabilityNode {
            data,
            flows_from: OrderedMap::new(),
            flows_to: OrderedMap::new(),
        };
        self.nodes.push(n);
        I::from_usize(i)
    }

    fn update_edge_value(&mut self, lhs: I, rhs: I, val: ExtEdgeData) {
        let old = self.nodes[lhs.to_usize()].flows_to.insert(rhs, val.clone());
        self.nodes[rhs.to_usize()].flows_from.insert(lhs, val);

        // If the nodes are >= rewind_mark, they'll be removed during rewind anyway
        // so we only have to journal edge values when both are below the mark.
        if lhs < self.rewind_mark && rhs < self.rewind_mark {
            self.journal.push((lhs, rhs, old));
        }
    }

    pub fn add_edge(&mut self, lhs: I, rhs: I, edge_val: ExtEdgeData, out: &mut Vec<(I, I, ExtEdgeData)>) {
        let mut work = vec![(lhs, rhs, edge_val)];

        while let Some((lhs, rhs, mut edge_val)) = work.pop() {
            if lhs == rhs {
                continue;
            }

            let old_edge = self.nodes[lhs.to_usize()].flows_to.m.get_mut(&rhs);
            if let Some(old) = old_edge {
                let mut old = old.clone();
                if old.update(&edge_val) {
                    edge_val = old; // updated value will be inserted into map below
                } else {
                    // New edge value did not cause an update compared to existing edge value.
                    continue;
                }
            };
            self.update_edge_value(lhs, rhs, edge_val.clone());

            for (&lhs2, lhs2_edge) in self.nodes[lhs.to_usize()].flows_from.iter() {
                let new_edge = edge_val.with_intermediate_node(&self.nodes[lhs.to_usize()].data, lhs, lhs2_edge);
                work.push((lhs2, rhs, new_edge));
            }

            for (&rhs2, rhs2_edge) in self.nodes[rhs.to_usize()].flows_to.iter() {
                let new_edge = edge_val.with_intermediate_node(&self.nodes[rhs.to_usize()].data, rhs, rhs2_edge);
                work.push((lhs, rhs2, new_edge));
            }

            // Inform the caller that a new edge was added
            out.push((lhs, rhs, edge_val));
        }
    }

    pub fn save(&mut self) {
        self.rewind_mark = I::from_usize(self.nodes.len());
    }

    pub fn revert(&mut self) {
        let i = self.rewind_mark;
        self.rewind_mark = I::from_usize(0);
        self.nodes.truncate(i.to_usize());

        while let Some((lhs, rhs, val)) = self.journal.pop() {
            if let Some(val) = val {
                self.nodes[lhs.to_usize()].flows_to.m.get_mut(&rhs).map(|e| *e = val.clone());
                self.nodes[rhs.to_usize()].flows_from.m.get_mut(&lhs).map(|e| *e = val);
            } else {
                self.nodes[lhs.to_usize()].flows_to.m.remove(&rhs);
                self.nodes[rhs.to_usize()].flows_from.m.remove(&lhs);
            }
        }

        // If we removed edges above, the edge maps will have extra keys
        // fix_and_truncate will fix that in addition to truncating edges >= i
        for n in self.nodes.iter_mut() {
            n.fix_and_truncate(i);
        }
    }

    pub fn make_permanent(&mut self) {
        self.rewind_mark = I::from_usize(0);
        self.journal.clear();
    }
}
