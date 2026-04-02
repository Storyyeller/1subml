// Copyright (c) 2026 Robert Grosse. All rights reserved.
use std::collections::HashSet;

use crate::ast::ImportId;
use crate::codegen::*;
use crate::parser::*;
use crate::spans::*;
use crate::typeck::*;

pub trait FileProvider {
    fn get_file(&mut self, module_path: &str) -> Result<String, String>;
}

pub struct State {
    parser: Parser,
    strings: lasso::Rodeo,

    checker: TypeckState,
    compiler: Compiler,

    compiled_modules: HashSet<ImportId>,
    circular_import_stack: HashSet<ImportId>,
}
impl State {
    pub fn new() -> Self {
        let mut strings = lasso::Rodeo::new();
        let checker = TypeckState::new(&mut strings);

        State {
            parser: Parser::new(),
            strings,

            checker,
            compiler: Compiler::new(),

            compiled_modules: HashSet::new(),
            circular_import_stack: HashSet::new(),
        }
    }

    pub fn spans(&self) -> &SpanManager {
        &self.parser.spans
    }

    fn compile_module(&mut self, id: ImportId, span: Span, files: &mut dyn FileProvider) -> Result<(), SpannedError> {
        if self.compiled_modules.contains(&id) {
            return Ok(());
        }
        if self.circular_import_stack.contains(&id) {
            return Err(SpannedError::new1("SyntaxError: Circular import detected.", span));
        }
        self.circular_import_stack.insert(id);

        let s = self.strings.resolve(&id.0);
        let source = files
            .get_file(s)
            .map_err(|e| SpannedError::new1(format!("ImportError: {}", e), span))?;

        let (ast, deps) = self.parser.parse(&mut self.strings, &source)?;
        for (dep, span) in deps {
            self.compile_module(dep, span, files)?;
        }

        self.checker.with(&mut self.strings).check_module_file(id, &ast)?;
        self.compiler.pending_imports.push((id, ast));

        self.compiled_modules.insert(id);
        self.circular_import_stack.remove(&id);
        Ok(())
    }

    pub fn compile_main(&mut self, source: &str, files: &mut dyn FileProvider) -> Result<String, SpannedError> {
        let (ast, deps) = self.parser.parse(&mut self.strings, source)?;
        for (dep, span) in deps {
            self.compile_module(dep, span, files)?;
        }

        self.checker.with(&mut self.strings).check_script(&ast)?;

        let mut ctx = Context(&mut self.compiler, &self.strings);
        let js_ast = compile_script(&mut ctx, &ast)?;
        Ok(js_ast.to_source())
    }
}
