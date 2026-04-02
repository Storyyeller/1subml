// Copyright (c) 2026 Robert Grosse. All rights reserved.

use std::collections::HashMap;

use compiler_lib::CompilationResult;
use compiler_lib::FileProvider;
use compiler_lib::State as CompilerState;
use wasm_bindgen::prelude::*;

struct WasmFileProviderImpl {
    files: HashMap<String, String>,
}
impl FileProvider for WasmFileProviderImpl {
    fn get_file(&mut self, module_path: &str) -> Result<String, String> {
        self.files
            .get(module_path)
            .cloned()
            .ok_or_else(|| format!("Module '{}' not found", module_path))
    }
}

#[wasm_bindgen]
pub struct State {
    s: CompilerState,
    files: WasmFileProviderImpl,

    out: Option<String>,
    err: Option<String>,
}
#[wasm_bindgen]
impl State {
    pub fn new() -> Self {
        State {
            s: CompilerState::new(),
            files: WasmFileProviderImpl { files: HashMap::new() },
            out: None,
            err: None,
        }
    }

    pub fn process(&mut self, source: &str) -> bool {
        let res = self.s.process(source, &mut self.files);
        match res {
            CompilationResult::Success(s) => {
                self.out = Some(s);
                true
            }
            CompilationResult::Error(e) => {
                self.err = Some(e);
                false
            }
        }
    }

    pub fn get_output(&mut self) -> Option<String> {
        self.out.take()
    }
    pub fn get_err(&mut self) -> Option<String> {
        self.err.take()
    }

    pub fn reset(&mut self) {
        self.s.reset();
    }

    pub fn set_file(&mut self, name: &str, contents: &str) {
        self.files.files.insert(name.to_string(), contents.to_string());
    }

    pub fn clear_files(&mut self) {
        self.files.files.clear();
    }
}
