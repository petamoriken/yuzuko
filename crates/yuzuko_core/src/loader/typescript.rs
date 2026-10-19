//! Type stripping for TypeScript modules. JavaScript syntax is left alone.

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{TransformOptions, Transformer, TypeScriptOptions};

use crate::RuntimeError;

/// Erases TypeScript syntax from one REPL chunk. The chunk is a script.
pub fn erase_repl(source: &str) -> Result<String, String> {
  let allocator = Allocator::default();
  // A module parse accepts top-level `await`. The chunk is still not
  // evaluated as a module.
  let source_type = SourceType::default()
    .with_typescript(true)
    .with_module(true);
  let parsed = Parser::new(&allocator, source, source_type).parse();
  if let Some(error) = parsed.diagnostics.first() {
    return Err(error.to_string());
  }
  let mut program = parsed.program;
  let scoping = SemanticBuilder::new()
    .with_check_syntax_error(true)
    .build(&program)
    .semantic
    .into_scoping();
  let options = TransformOptions {
    typescript: TypeScriptOptions {
      only_remove_type_imports: false,
      allow_namespaces: true,
      ..TypeScriptOptions::default()
    },
    ..TransformOptions::default()
  };
  let transformed =
    Transformer::new(&allocator, Path::new("repl.ts"), &options)
      .build_with_scoping(scoping, &mut program);
  if let Some(error) = transformed.diagnostics.first() {
    return Err(error.to_string());
  }
  Ok(Codegen::new().build(&program).code)
}

pub fn prepare_source(
  path: &Path,
  source: &str,
) -> Result<String, RuntimeError> {
  let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
  if matches!(extension, "tsx" | "jsx") {
    return Err(RuntimeError::message(format!(
      "{}: JSX is not supported",
      path.display()
    )));
  }
  if !matches!(extension, "ts" | "mts") {
    return Ok(source.to_string());
  }

  let allocator = Allocator::default();
  let source_type = SourceType::from_path(path).map_err(|err| {
    RuntimeError::message(format!("{}: {err}", path.display()))
  })?;
  let parsed = Parser::new(&allocator, source, source_type).parse();
  if let Some(error) = parsed.diagnostics.first() {
    return Err(RuntimeError::message(format!(
      "{}: {error}",
      path.display()
    )));
  }
  let mut program = parsed.program;
  let scoping = SemanticBuilder::new()
    .with_check_syntax_error(true)
    .build(&program)
    .semantic
    .into_scoping();
  let options = TransformOptions {
    typescript: TypeScriptOptions {
      only_remove_type_imports: false,
      allow_namespaces: true,
      ..TypeScriptOptions::default()
    },
    ..TransformOptions::default()
  };
  let transformed = Transformer::new(&allocator, path, &options)
    .build_with_scoping(scoping, &mut program);
  if !transformed.diagnostics.is_empty() {
    let err = &transformed.diagnostics[0];
    return Err(RuntimeError::message(format!("{}: {err}", path.display())));
  }
  let printed = Codegen::new()
    .with_options(CodegenOptions {
      source_map_path: Some(path.to_path_buf()),
      ..CodegenOptions::default()
    })
    .build(&program);
  Ok(printed.code)
}
