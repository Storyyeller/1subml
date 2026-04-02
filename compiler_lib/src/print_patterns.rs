// Copyright (c) 2026 Robert Grosse. All rights reserved.
use crate::ast::StringId;
use crate::ordered_map::OrderedMap;
use crate::tuples::*;

#[derive(Debug, Clone, Copy)]
pub enum PathComponent {
    Field(StringId),
    Case(StringId),
}
/// Code for printing out example patterns in error messages.
pub struct MergedPathNode {
    fields: OrderedMap<StringId, Box<MergedPathNode>>,
    case: Option<(StringId, Box<MergedPathNode>)>,
}
impl MergedPathNode {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            fields: OrderedMap::new(),
            case: None,
        })
    }

    fn merge(&mut self, path: PathComponent) -> &mut Self {
        match path {
            PathComponent::Field(f) => self.fields.entry_or_insert_with(f, MergedPathNode::new),
            PathComponent::Case(c) => self.case.get_or_insert_with(|| (c, MergedPathNode::new())).1.as_mut(),
        }
    }

    pub fn merge_path(&mut self, path: &[PathComponent], final_case: StringId) {
        let mut node = self;
        for p in path {
            node = node.merge(*p);
        }
        node.merge(PathComponent::Case(final_case));
    }

    fn print_sub(&self, strings: &lasso::Rodeo, out: &mut String) {
        if let Some((c, sub)) = &self.case {
            out.push('`');
            out.push_str(strings.resolve(c));
            // Don't bother printing out the subpattern if it's empty, (i.e. just say "`None" instead of "`None _").
            if sub.case.is_some() || !sub.fields.keys.is_empty() {
                out.push(' ');
                sub.print_sub(strings, out);
            }
        } else if !self.fields.keys.is_empty() {
            let tuple_len = is_partial_tuple_record(strings, self.fields.keys.iter().copied());
            if let Some(len) = tuple_len {
                out.push('(');
                for i in 0..len {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    let key = get_tuple_name(strings, i);
                    if let Some(node) = key.and_then(|k| self.fields.m.get(&k)) {
                        node.print_sub(strings, out);
                    } else {
                        out.push('_');
                    }
                }
                out.push(')');
            } else {
                let mut first = true;
                out.push('{');
                for (f, sub) in self.fields.iter() {
                    if first {
                        first = false;
                    } else {
                        out.push_str(", ");
                    }

                    out.push_str(strings.resolve(f));
                    out.push_str(": ");
                    sub.print_sub(strings, out);
                }
                out.push('}');
            }
        } else {
            out.push('_');
        }
    }

    pub fn print(&self, strings: &lasso::Rodeo) -> String {
        let mut s = String::new();
        self.print_sub(strings, &mut s);
        s
    }
}
