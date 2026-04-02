// Copyright (c) 2026 Robert Grosse. All rights reserved.
#![deny(unused_must_use)]

mod ast;
mod build_graph;
mod codegen;
mod coercion;
mod core;
mod exhaustiveness;
mod grammar;
mod instantiate;
mod introspect_types;
mod js;
mod kinds;
mod ordered_map;
mod parse_patterns;
mod parse_types;
mod parser;
mod print_patterns;
mod prune_unused_poly_vars;
mod reachability;
mod restamp;
mod short_str;
mod spans;
mod spines;
mod subsumption;
mod templates;
mod tuples;
mod type_errors;
mod typeck;
mod types;
mod unification;
mod unwindmap;
mod vec_index;

pub use crate::build_graph::FileProvider;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompilationResult {
    Success(String), // Contains compiled JS code
    Error(String),   // Contains error message
}
impl std::fmt::Display for CompilationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompilationResult::Success(js_code) => write!(f, "SUCCESS\n{}", js_code),
            CompilationResult::Error(error_msg) => write!(f, "ERROR\n{}", error_msg),
        }
    }
}

pub struct State(build_graph::State);
impl State {
    pub fn new() -> Self {
        State(build_graph::State::new())
    }

    pub fn process(&mut self, source: &str, files: &mut dyn FileProvider) -> CompilationResult {
        let res = self.0.compile_main(source, files);
        match res {
            Ok(s) => CompilationResult::Success(s),
            Err(e) => CompilationResult::Error(e.print(self.0.spans())),
        }
    }

    pub fn reset(&mut self) {
        self.0 = build_graph::State::new();
    }
}
