// Copyright (c) 2026 Robert Grosse. All rights reserved.

use std::fs;
use std::path::PathBuf;

use clap::Parser;
use cli_lib::js_executor::JsExecutor;
use compiler_lib::CompilationResult;
use compiler_lib::FileProvider;
use compiler_lib::State;

#[derive(Parser)]
#[command(name = "cli")]
#[command(about = "1SubML Compiler CLI")]
struct Args {
    /// ML files to compile
    files: Vec<PathBuf>,

    /// Base directory for resolving module imports
    #[arg(long, default_value = ".")]
    base_dir: PathBuf,

    /// Directory containing standard library modules
    #[arg(long, env = "ONESUBML_STDLIB")]
    stdlib_dir: Option<PathBuf>,

    /// Directory to cache JS execution results (optional)
    #[arg(long, env = "ONESUBML_CACHE_DIR")]
    cache_dir: Option<PathBuf>,
}

struct FileProviderImpl {
    search_paths: Vec<std::path::PathBuf>,
}
impl FileProviderImpl {
    pub fn new(search_paths: Vec<std::path::PathBuf>) -> Self {
        Self { search_paths }
    }
}
impl FileProvider for FileProviderImpl {
    fn get_file(&mut self, module_path: &str) -> Result<String, String> {
        let rel_path: PathBuf = module_path.split('.').collect();
        let rel_path = rel_path.with_extension("ml");
        for base in &self.search_paths {
            let full = base.join(&rel_path);
            if let Ok(content) = std::fs::read_to_string(&full) {
                return Ok(content);
            }
        }
        Err(format!("Module '{}' not found", module_path))
    }
}

fn main() {
    let args = Args::parse();
    let js_executor = JsExecutor::new(args.cache_dir);

    let mut search_paths = vec![args.base_dir.clone()];
    if let Some(ref stdlib_dir) = args.stdlib_dir {
        search_paths.push(stdlib_dir.clone());
    }
    let mut files = FileProviderImpl::new(search_paths);

    for fname in args.files {
        println!("Processing {}", fname.display());
        let data = match fs::read_to_string(&fname) {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Error reading {}: {}", fname.display(), e);
                std::process::exit(1);
            }
        };

        let mut state = State::new();
        let res = state.process(&data, &mut files);
        println!("{}", res);

        if let CompilationResult::Success(js_code) = res {
            println!("\nExecuting...");
            match js_executor.execute_js(&js_code) {
                Ok(output) => {
                    println!("Output:\n{}", output);
                }
                Err(e) => {
                    eprintln!("Execution error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}
