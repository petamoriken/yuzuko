//! Interactive evaluation. Chunks are classic scripts, not ES modules.
//!
//! Top-level bindings are copied onto the global object so the next chunk can
//! read them. Top-level `await` runs in one async function per chunk.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
  BindingIdentifier, BindingPattern, Class, Function, Statement,
  VariableDeclaration,
};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};

use crate::host;
use crate::line::{self, is_ident_name};
use crate::loader::modules::{self, exception_text};
use crate::loader::typescript;
use crate::runtime::{self, RuntimeError};

pub fn run_repl() -> Result<(), RuntimeError> {
  let cwd = std::env::current_dir().map_err(|error| {
    RuntimeError::message(format!("current directory: {error}"))
  })?;
  let origin = modules::file_url(&cwd.join("repl.js"))?;
  runtime::with_runtime(
    false,
    false,
    |_| {},
    |scope| {
      host::set_sandbox_dir(&cwd);
      host::set_entry_url(origin.clone());
      let mut editor = line::Editor::new();
      loop {
        let chunk = editor.read(&mut |object| complete_names(scope, object));
        let Some(chunk) = chunk else {
          return Ok(());
        };
        evaluate(scope, &chunk, &origin);
      }
    },
  )
}

pub fn input_is_complete(source: &str) -> bool {
  scan_complete(source)
}

fn evaluate(scope: &mut v8::PinScope, source: &str, origin: &str) {
  let prepared = match prepare(source) {
    Ok(prepared) => prepared,
    Err(message) => {
      println!("SyntaxError: {message}");
      return;
    }
  };
  if prepared.trim().is_empty() {
    return;
  }
  let value = {
    v8::tc_scope!(let tc, scope);
    let Some(code) = v8::String::new(tc, &prepared) else {
      println!("SyntaxError: script is too large");
      return;
    };
    let script_origin = runtime::script_origin(tc, Some(origin));
    let Some(script) = v8::Script::compile(tc, code, script_origin.as_ref())
    else {
      println!("{}", exception_text!(tc, "failed to compile"));
      return;
    };
    let Some(value) = script.run(tc) else {
      println!("{}", exception_text!(tc, "failed to run"));
      return;
    };
    v8::Global::new(tc, value)
  };
  let value = v8::Local::new(scope, value);
  let value = if let Ok(promise) = v8::Local::<v8::Promise>::try_from(value) {
    promise.mark_as_handled();
    match wait_for_promise(scope, promise) {
      Ok(value) => value,
      Err(reason) => {
        println!("{}", describe(scope, reason));
        drain_ready(scope);
        return;
      }
    }
  } else {
    value
  };
  // A later setTimeout or setInterval must not delay the prompt. Timers
  // whose deadline has already passed, including setTimeout(0), still run.
  drain_ready(scope);
  println!("{}", display_value(scope, value));
}

fn wait_for_promise<'a>(
  scope: &mut v8::PinScope<'a, '_>,
  promise: v8::Local<'a, v8::Promise>,
) -> Result<v8::Local<'a, v8::Value>, v8::Local<'a, v8::Value>> {
  loop {
    match promise.state() {
      v8::PromiseState::Fulfilled => return Ok(promise.result(scope)),
      v8::PromiseState::Rejected => return Err(promise.result(scope)),
      v8::PromiseState::Pending => {
        drain_ready(scope);
        if promise.state() != v8::PromiseState::Pending {
          continue;
        }
        // The chunk itself is still awaiting. Sleep only until the next
        // timer or host task that might settle it.
        let wait =
          earliest(crate::timers::next_wait(), host::queue().next_wait());
        if let Some(wait) = wait {
          runtime::loop_wait(Some(wait));
        } else if crate::jobs::pending()
          || crate::worker::keeps_alive(host::agent_id())
        {
          runtime::loop_wait(Some(std::time::Duration::from_millis(15)));
        } else {
          runtime::loop_wait(None);
        }
      }
    }
  }
}

/// Microtasks, ready host tasks, and one wave of timers that are already due.
///
/// A timer that is still in the future stays queued. A zero-delay interval, or
/// a `setTimeout` that schedules itself again, is not fired a second time in
/// this turn: the nesting clamp in the timer queue resets between fires, so
/// polling every due timer would spin and never print the id.
fn drain_ready(scope: &mut v8::PinScope) {
  let mut wave = crate::timers::due_ids();
  loop {
    scope.perform_microtask_checkpoint();
    let mut progressed = false;
    if crate::worker::drain_inbound(scope) {
      progressed = true;
    }
    if let Some(timer) = crate::timers::poll_due_in(&wave) {
      wave.retain(|id| *id != timer.id);
      crate::timers::fire(scope, timer);
      progressed = true;
    }
    if let Some(task) = host::queue().pop_ready() {
      task.run();
      progressed = true;
    }
    if crate::jobs::poll(scope) {
      progressed = true;
    }
    if !progressed {
      break;
    }
  }
}

fn earliest(
  left: Option<std::time::Duration>,
  right: Option<std::time::Duration>,
) -> Option<std::time::Duration> {
  match (left, right) {
    (Some(left), Some(right)) => Some(left.min(right)),
    (Some(left), None) => Some(left),
    (None, Some(right)) => Some(right),
    (None, None) => None,
  }
}

fn display_value(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> String {
  if value.is_undefined() {
    return "undefined".to_string();
  }
  if value.is_null() {
    return "null".to_string();
  }
  v8::tc_scope!(let tc, scope);
  value
    .to_string(tc)
    .map(|text| text.to_rust_string_lossy(tc))
    .unwrap_or_else(|| "undefined".to_string())
}

fn describe(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> String {
  modules::exception_stack(scope, value).unwrap_or_else(|| {
    v8::tc_scope!(let tc, scope);
    value
      .to_string(tc)
      .map(|text| text.to_rust_string_lossy(tc))
      .unwrap_or_else(|| "Error".to_string())
  })
}

fn complete_names(
  scope: &mut v8::PinScope,
  object: Option<&str>,
) -> Vec<String> {
  let source = match object {
    Some(object) => format!(
      "(function() {{\n  try {{\n    const __value = ({object});\n    return __yuzukoNames(__value);\n  }} catch (e) {{\n    return \"\";\n  }}\n}})()"
    ),
    None => {
      "(function() {\n  try {\n    return __yuzukoNames(globalThis);\n  } catch (e) {\n    return \"\";\n  }\n})()"
        .to_string()
    }
  };
  let listed = format!(
    "(function() {{\n  function __yuzukoNames(__value) {{\n    const __names = [];\n    const __seen = new Set();\n    let __cur = __value;\n    while (__cur != null) {{\n      for (const __key of Object.getOwnPropertyNames(__cur)) {{\n        if (!__seen.has(__key)) {{\n          __seen.add(__key);\n          __names.push(__key);\n        }}\n      }}\n      __cur = Object.getPrototypeOf(__cur);\n    }}\n    return __names.join(\"\\n\");\n  }}\n  return {source};\n}})()"
  );
  let Some(text) = eval_string(scope, &listed) else {
    return Vec::new();
  };
  text
    .split('\n')
    .filter(|name| is_ident_name(name))
    .map(str::to_string)
    .collect()
}

fn eval_string(scope: &mut v8::PinScope, source: &str) -> Option<String> {
  v8::tc_scope!(let tc, scope);
  let code = v8::String::new(tc, source)?;
  let script = v8::Script::compile(tc, code, None)?;
  let value = script.run(tc)?;
  if value.is_promise() {
    return None;
  }
  let text = value.to_string(tc)?;
  Some(text.to_rust_string_lossy(tc))
}

fn prepare(source: &str) -> Result<String, String> {
  let erased = typescript::erase_repl(source)?;
  if erased.trim().is_empty() {
    return Ok(String::new());
  }
  let source = paren_object(&erased);
  rewrite(&source)
}

fn paren_object(source: &str) -> String {
  let trimmed = source.trim();
  if trimmed.starts_with('{') && !trimmed.ends_with(';') {
    let wrapped = format!("({source}\n)");
    if js_parses(&wrapped) {
      return wrapped;
    }
  }
  source.to_string()
}

fn js_parses(source: &str) -> bool {
  let allocator = Allocator::default();
  let parsed =
    Parser::new(&allocator, source, SourceType::default().with_module(true))
      .parse();
  parsed.diagnostics.is_empty()
}

fn rewrite(source: &str) -> Result<String, String> {
  let allocator = Allocator::default();
  let parsed =
    Parser::new(&allocator, source, SourceType::default().with_module(true))
      .parse();
  if let Some(error) = parsed.diagnostics.first() {
    return Err(error.to_string());
  }
  let body = &parsed.program.body;
  if body.is_empty() {
    return Ok(String::new());
  }
  for statement in body {
    if is_module_statement(statement) {
      return Err(
        "static import and export are not available in the repl".to_string(),
      );
    }
  }
  let last_expr =
    matches!(body.last(), Some(Statement::ExpressionStatement(_)));
  let statements = if last_expr {
    &body[..body.len() - 1]
  } else {
    body.as_slice()
  };
  let mut out = String::from("(async () => {\n");
  for statement in statements {
    out.push_str(&emit_statement(source, statement));
  }
  if last_expr
    && let Some(Statement::ExpressionStatement(statement)) = body.last()
  {
    out.push_str("return (");
    out.push_str(slice(source, statement.expression.span()));
    out.push_str(");\n");
  }
  out.push_str("})()\n");
  Ok(out)
}

fn emit_statement(source: &str, statement: &Statement<'_>) -> String {
  match statement {
    Statement::VariableDeclaration(declaration) => {
      publish_binding(source, declaration.span(), bound_vars(declaration))
    }
    Statement::FunctionDeclaration(function) => {
      publish_binding(source, function.span(), bound_function(function))
    }
    Statement::ClassDeclaration(class) => {
      publish_binding(source, class.span(), bound_class(class))
    }
    _ => format!("{}\n", slice(source, statement.span())),
  }
}

fn publish_binding(
  source: &str,
  span: oxc_span::Span,
  names: Vec<String>,
) -> String {
  if names.is_empty() {
    return format!("{}\n", slice(source, span));
  }
  let mut out = String::from("await (async () => {\n");
  out.push_str(slice(source, span));
  out.push('\n');
  for name in names {
    out.push_str(&format!("globalThis.{name} = {name};\n"));
  }
  out.push_str("})();\n");
  out
}

fn bound_vars(declaration: &VariableDeclaration<'_>) -> Vec<String> {
  let mut names = Vec::new();
  for declarator in &declaration.declarations {
    bound_names(&declarator.id, &mut names);
  }
  names
}

fn bound_function(function: &Function<'_>) -> Vec<String> {
  function.id.as_ref().map(ident_name).into_iter().collect()
}

fn bound_class(class: &Class<'_>) -> Vec<String> {
  class.id.as_ref().map(ident_name).into_iter().collect()
}

fn bound_names(pattern: &BindingPattern<'_>, names: &mut Vec<String>) {
  match pattern {
    BindingPattern::BindingIdentifier(ident) => names.push(ident_name(ident)),
    BindingPattern::ObjectPattern(pattern) => {
      for property in &pattern.properties {
        bound_names(&property.value, names);
      }
      if let Some(rest) = &pattern.rest {
        bound_names(&rest.argument, names);
      }
    }
    BindingPattern::ArrayPattern(pattern) => {
      for element in pattern.elements.iter().flatten() {
        bound_names(element, names);
      }
      if let Some(rest) = &pattern.rest {
        bound_names(&rest.argument, names);
      }
    }
    BindingPattern::AssignmentPattern(pattern) => {
      bound_names(&pattern.left, names);
    }
  }
}

fn ident_name(ident: &BindingIdentifier<'_>) -> String {
  ident.name.as_str().to_string()
}

fn is_module_statement(statement: &Statement<'_>) -> bool {
  matches!(
    statement,
    Statement::ImportDeclaration(_)
      | Statement::ExportAllDeclaration(_)
      | Statement::ExportDefaultDeclaration(_)
      | Statement::ExportNamedDeclaration(_)
      | Statement::ExportFromDeclaration(_)
  )
}

fn slice(source: &str, span: oxc_span::Span) -> &str {
  let start = span.start as usize;
  let end = span.end as usize;
  if start <= end && end <= source.len() {
    &source[start..end]
  } else {
    ""
  }
}

fn scan_complete(source: &str) -> bool {
  let chars: Vec<char> = source.chars().collect();
  let mut index = 0;
  let mut mode = Scan::Code;
  let mut brackets: Vec<Bracket> = Vec::new();
  let mut trailing_dot = false;
  let mut expr_end = false;
  while index < chars.len() {
    let ch = chars[index];
    let next = chars.get(index + 1).copied();
    match mode {
      Scan::Line => {
        if ch == '\n' {
          mode = Scan::Code;
        }
        index += 1;
        continue;
      }
      Scan::Block => {
        if ch == '*' && next == Some('/') {
          mode = Scan::Code;
          index += 2;
        } else {
          index += 1;
        }
        continue;
      }
      Scan::Single | Scan::Double => {
        if ch == '\\' {
          index += if next.is_some() { 2 } else { 1 };
          continue;
        }
        let quote = if mode == Scan::Single { '\'' } else { '"' };
        if ch == quote {
          mode = Scan::Code;
          expr_end = true;
          trailing_dot = false;
        }
        index += 1;
        continue;
      }
      Scan::Template => {
        if ch == '\\' {
          index += if next.is_some() { 2 } else { 1 };
          continue;
        }
        if ch == '`' {
          mode = Scan::Code;
          expr_end = true;
          trailing_dot = false;
          index += 1;
          continue;
        }
        if ch == '$' && next == Some('{') {
          mode = Scan::Code;
          brackets.push(Bracket {
            kind: b'{',
            interp: true,
          });
          expr_end = false;
          trailing_dot = false;
          index += 2;
          continue;
        }
        index += 1;
        continue;
      }
      Scan::Code => {}
    }
    if ch.is_whitespace() {
      index += 1;
      continue;
    }
    if ch == '/' && next == Some('/') {
      mode = Scan::Line;
      index += 2;
      continue;
    }
    if ch == '/' && next == Some('*') {
      mode = Scan::Block;
      index += 2;
      continue;
    }
    if ch == '/' && !expr_end {
      if skip_regex(&chars, &mut index) {
        expr_end = true;
        trailing_dot = false;
      } else {
        expr_end = false;
        trailing_dot = false;
        index += 1;
      }
      continue;
    }
    match ch {
      '\'' => {
        mode = Scan::Single;
        expr_end = false;
        trailing_dot = false;
      }
      '"' => {
        mode = Scan::Double;
        expr_end = false;
        trailing_dot = false;
      }
      '`' => {
        mode = Scan::Template;
        expr_end = false;
        trailing_dot = false;
      }
      '(' => {
        brackets.push(Bracket {
          kind: b'(',
          interp: false,
        });
        expr_end = false;
        trailing_dot = false;
      }
      '[' => {
        brackets.push(Bracket {
          kind: b'[',
          interp: false,
        });
        expr_end = false;
        trailing_dot = false;
      }
      '{' => {
        brackets.push(Bracket {
          kind: b'{',
          interp: false,
        });
        expr_end = false;
        trailing_dot = false;
      }
      ')' | ']' | '}' => {
        let want = match ch {
          ')' => b'(',
          ']' => b'[',
          _ => b'{',
        };
        match brackets.pop() {
          Some(top) if top.kind == want => {
            if top.interp {
              mode = Scan::Template;
              expr_end = false;
            } else {
              expr_end = true;
            }
            trailing_dot = false;
          }
          _ => return true,
        }
      }
      '.' => {
        if next == Some('.') {
          index += 1;
          if chars.get(index + 1) == Some(&'.') {
            index += 1;
          }
          expr_end = false;
          trailing_dot = false;
        } else if next.is_some_and(|ch| ch.is_ascii_digit()) {
          index = scan_number(&chars, index);
          expr_end = true;
          trailing_dot = false;
          continue;
        } else {
          trailing_dot = true;
          expr_end = false;
        }
      }
      ch if ch.is_ascii_digit() => {
        index = scan_number(&chars, index);
        expr_end = true;
        trailing_dot = false;
        continue;
      }
      ch if is_ident_start(ch) => {
        index += 1;
        while index < chars.len() && is_ident_continue(chars[index]) {
          index += 1;
        }
        expr_end = true;
        trailing_dot = false;
        continue;
      }
      _ => {
        expr_end = false;
        trailing_dot = false;
      }
    }
    index += 1;
  }
  if mode == Scan::Line {
    mode = Scan::Code;
  }
  mode == Scan::Code && brackets.is_empty() && !trailing_dot
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scan {
  Code,
  Single,
  Double,
  Template,
  Line,
  Block,
}

struct Bracket {
  kind: u8,
  interp: bool,
}

fn skip_regex(chars: &[char], index: &mut usize) -> bool {
  let mut cursor = *index + 1;
  let mut class = false;
  while cursor < chars.len() {
    let ch = chars[cursor];
    if ch == '\n' || ch == '\r' {
      return false;
    }
    if ch == '\\' {
      cursor += 2;
      continue;
    }
    if ch == '[' {
      class = true;
    } else if ch == ']' {
      class = false;
    } else if ch == '/' && !class {
      cursor += 1;
      while cursor < chars.len() && chars[cursor].is_ascii_alphabetic() {
        cursor += 1;
      }
      *index = cursor;
      return true;
    }
    cursor += 1;
  }
  false
}

fn scan_number(chars: &[char], start: usize) -> usize {
  let mut index = start;
  if chars.get(index) == Some(&'0')
    && matches!(
      chars.get(index + 1),
      Some('x' | 'X' | 'b' | 'B' | 'o' | 'O')
    )
  {
    index += 2;
    while index < chars.len() && chars[index].is_ascii_hexdigit() {
      index += 1;
    }
    return index;
  }
  if chars.get(index) == Some(&'.') {
    index += 1;
  }
  while index < chars.len() && chars[index].is_ascii_digit() {
    index += 1;
  }
  if chars.get(index) == Some(&'.') {
    let after = chars.get(index + 1).copied();
    if after.is_some_and(|ch| ch.is_ascii_digit()) {
      index += 1;
      while index < chars.len() && chars[index].is_ascii_digit() {
        index += 1;
      }
    } else if after.is_none_or(|ch| !is_ident_start(ch) && ch != '.') {
      index += 1;
    }
  }
  if matches!(chars.get(index), Some('e' | 'E')) {
    let mut exponent = index + 1;
    if matches!(chars.get(exponent), Some('+' | '-')) {
      exponent += 1;
    }
    if chars.get(exponent).is_some_and(|ch| ch.is_ascii_digit()) {
      index = exponent;
      while index < chars.len() && chars[index].is_ascii_digit() {
        index += 1;
      }
    }
  }
  index
}

fn is_ident_start(ch: char) -> bool {
  ch == '_' || ch == '$' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
  is_ident_start(ch) || ch.is_ascii_digit()
}

#[cfg(test)]
mod tests {
  use super::input_is_complete;

  #[test]
  fn brackets_strings_templates_and_trailing_dots() {
    assert!(input_is_complete("1 + 1"));
    assert!(input_is_complete("(1 + 1)"));
    assert!(!input_is_complete("(1 +"));
    assert!(!input_is_complete("(1 +\n"));
    assert!(input_is_complete("(1 +\n1)"));
    assert!(!input_is_complete("{"));
    assert!(input_is_complete("{ a: 1 }"));
    assert!(!input_is_complete("[1,"));
    assert!(input_is_complete(")"));
    assert!(!input_is_complete("\"hi"));
    assert!(input_is_complete("\"hi\""));
    assert!(!input_is_complete("`hi ${1"));
    assert!(input_is_complete("`hi ${1}`"));
    assert!(!input_is_complete("`hi"));
    assert!(input_is_complete("`hi\nthere`"));
    assert!(!input_is_complete("/*"));
    assert!(input_is_complete("/* done */ 1"));
    assert!(!input_is_complete("obj."));
    assert!(!input_is_complete("Math."));
    assert!(input_is_complete("1."));
    assert!(input_is_complete("1.5"));
    assert!(input_is_complete("const labeled: number = 3"));
    assert!(input_is_complete(""));
  }
}
