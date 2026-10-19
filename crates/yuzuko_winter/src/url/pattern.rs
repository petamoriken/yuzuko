//! `URLPattern`, backed by the published `urlpattern` crate.
//!
//! Group regular expressions run in V8. The crate's Rust `regex` engine
//! accepts `(?R)` and reports an unmatched optional group as an empty
//! string.

use std::cell::Cell;

use urlpattern::UrlPatternInit;
use urlpattern::UrlPatternMatchInput;
use urlpattern::UrlPatternOptions;
use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_macros::op;

type CompiledPattern = urlpattern::UrlPattern<EcmaRegExp>;

pub struct UrlPattern {
  inner: CompiledPattern,
}

unsafe impl v8::cppgc::GarbageCollected for UrlPattern {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"URLPattern"
  }
}

#[op(name = "URLPattern")]
impl UrlPattern {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let _regexp = enter_regexp_scope(scope);
    let input = if args.length() == 0 {
      None
    } else {
      Some(args.get(0))
    };
    let second = if args.length() < 2 || args.get(1).is_null_or_undefined() {
      None
    } else {
      Some(args.get(1))
    };
    let third = if args.length() < 3 || args.get(2).is_null_or_undefined() {
      None
    } else {
      Some(args.get(2))
    };
    let input_is_string = input.is_some_and(|value| {
      !value.is_null_or_undefined() && !is_init_object(scope, value)
    });
    let (base, ignore_case) =
      split_base_and_options(scope, input_is_string, second, third)?;
    let Some(init) = read_init(scope, input, base, true)? else {
      return Err(OpError::type_("The base URL is invalid"));
    };
    let pattern = CompiledPattern::parse(
      init,
      UrlPatternOptions {
        ignore_case,
        regex_syntax: urlpattern::RegexSyntax::EcmaScript,
      },
    )
    .map_err(|error| OpError::type_(error.to_string()))?;
    Ok(Self { inner: pattern })
  }

  #[getter]
  fn protocol(&self) -> String {
    self.inner.protocol().to_string()
  }

  #[getter]
  fn username(&self) -> String {
    self.inner.username().to_string()
  }

  #[getter]
  fn password(&self) -> String {
    self.inner.password().to_string()
  }

  #[getter]
  fn hostname(&self) -> String {
    self.inner.hostname().to_string()
  }

  #[getter]
  fn port(&self) -> String {
    self.inner.port().to_string()
  }

  #[getter]
  fn pathname(&self) -> String {
    self.inner.pathname().to_string()
  }

  #[getter]
  fn search(&self) -> String {
    self.inner.search().to_string()
  }

  #[getter]
  fn hash(&self) -> String {
    self.inner.hash().to_string()
  }

  #[getter]
  fn has_reg_exp_groups(&self) -> bool {
    self.inner.has_regexp_groups()
  }

  fn test(
    &self,
    scope: &mut v8::PinScope,
    input: Option<v8::Local<v8::Value>>,
    base: Option<String>,
  ) -> Result<bool, OpError> {
    let _regexp = enter_regexp_scope(scope);
    let input = present(input);
    let Some(matched) = classify_match(scope, input, base.as_deref())? else {
      return Ok(false);
    };
    self
      .inner
      .test(matched)
      .map_err(|error| OpError::type_(error.to_string()))
  }

  fn exec(
    &self,
    scope: &mut v8::PinScope,
    input: Option<v8::Local<v8::Value>>,
    base: Option<v8::Local<v8::Value>>,
  ) -> Result<Js, OpError> {
    let _regexp = enter_regexp_scope(scope);
    let input = present(input);
    let base = present(base);
    let base_text = match base {
      Some(base) => Some(text(scope, base)?),
      None => None,
    };
    let Some(matched) = classify_match(scope, input, base_text.as_deref())?
    else {
      return Ok(null_js(scope));
    };
    let result = self
      .inner
      .exec(matched)
      .map_err(|error| OpError::type_(error.to_string()))?;
    let Some(result) = result else {
      return Ok(null_js(scope));
    };
    let recorded = input.unwrap_or_else(|| v8::Object::new(scope).into());
    let recorded_base =
      if input.is_some_and(|value| !is_init_object(scope, value)) {
        base
      } else {
        None
      };
    Ok(result_object(scope, &result, Some(recorded), recorded_base))
  }

  #[static_method]
  fn compare_component(
    scope: &mut v8::PinScope,
    component: String,
    left: v8::Local<v8::Value>,
    right: v8::Local<v8::Value>,
  ) -> Result<i32, OpError> {
    let left_parts = parts_of(scope, left, &component)?;
    let right_parts = parts_of(scope, right, &component)?;
    Ok(compare_parts(&left_parts, &right_parts))
  }

  fn generate(
    &self,
    scope: &mut v8::PinScope,
    component: String,
    groups: v8::Local<v8::Value>,
  ) -> Result<String, OpError> {
    let groups = read_groups(scope, groups)?;
    generate_component(&self.inner, &component, &groups)
  }
}

fn split_base_and_options(
  scope: &mut v8::PinScope,
  input_is_string: bool,
  second: Option<v8::Local<v8::Value>>,
  third: Option<v8::Local<v8::Value>>,
) -> Result<(Option<String>, bool), OpError> {
  if !input_is_string
    && (second.is_some_and(|value| value.is_string()) || third.is_some())
  {
    return Err(OpError::type_("A base URL requires a string URL pattern"));
  }
  match (second, third) {
    (None, None) => Ok((None, false)),
    (Some(second), None) if second.is_string() => {
      Ok((Some(text(scope, second)?), false))
    }
    (Some(second), None) => Ok((None, ignore_case(scope, second))),
    (Some(second), Some(third)) => {
      let base = text(scope, second)?;
      Ok((Some(base), ignore_case(scope, third)))
    }
    (None, Some(third)) => Ok((None, ignore_case(scope, third))),
  }
}

fn ignore_case(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> bool {
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return false;
  };
  let key = v8::String::new(scope, "ignoreCase").unwrap();
  object
    .get(scope, key.into())
    .is_some_and(|value| value.boolean_value(scope))
}

fn read_init(
  scope: &mut v8::PinScope,
  input: Option<v8::Local<v8::Value>>,
  base: Option<String>,
  strict_base: bool,
) -> Result<Option<UrlPatternInit>, OpError> {
  let base_url = match base {
    Some(base) => match parse_base(&base, strict_base)? {
      Some(url) => Some(url),
      None => return Ok(None),
    },
    None => None,
  };
  let Some(input) = input.filter(|value| !value.is_null_or_undefined()) else {
    return Ok(Some(UrlPatternInit {
      base_url,
      ..Default::default()
    }));
  };
  if !is_init_object(scope, input) {
    let pattern = text(scope, input)?;
    return UrlPatternInit::parse_constructor_string::<EcmaRegExp>(
      &pattern, base_url,
    )
    .map(Some)
    .map_err(|error| OpError::type_(error.to_string()));
  }
  let object = v8::Local::<v8::Object>::try_from(input)
    .map_err(|_| OpError::type_("The URL pattern is invalid"))?;
  let mut init = UrlPatternInit {
    base_url,
    ..UrlPatternInit::default()
  };
  init.protocol = optional_field(scope, object, "protocol")?;
  init.username = optional_field(scope, object, "username")?;
  init.password = optional_field(scope, object, "password")?;
  init.hostname = optional_field(scope, object, "hostname")?;
  init.port = optional_field(scope, object, "port")?;
  init.pathname = optional_field(scope, object, "pathname")?;
  init.search = optional_field(scope, object, "search")?;
  init.hash = optional_field(scope, object, "hash")?;
  if init.base_url.is_none()
    && let Some(base) = optional_field(scope, object, "baseURL")?
  {
    match parse_base(&base, strict_base)? {
      Some(url) => init.base_url = Some(url),
      None => return Ok(None),
    }
  }
  Ok(Some(init))
}

fn parse_base(base: &str, strict: bool) -> Result<Option<url::Url>, OpError> {
  match url::Url::parse(base) {
    Ok(url) => Ok(Some(url)),
    Err(_) if strict => Err(OpError::type_("The base URL is invalid")),
    Err(_) => Ok(None),
  }
}

fn object_is_url(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> bool {
  let symbol = v8::Symbol::get_to_string_tag(scope);
  object
    .get(scope, symbol.into())
    .and_then(|value| value.to_string(scope))
    .is_some_and(|text| text.to_rust_string_lossy(scope) == "URL")
}

fn optional_field(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<String>, OpError> {
  let key = v8::String::new(scope, name).unwrap();
  let Some(value) = object.get(scope, key.into()) else {
    return Err(OpError::type_("The URL pattern is invalid"));
  };
  if value.is_undefined() {
    return Ok(None);
  }
  Ok(Some(text(scope, value)?))
}

fn text(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<String, OpError> {
  value
    .to_string(scope)
    .map(|text| text.to_rust_string_lossy(scope))
    .ok_or_else(|| OpError::type_("The URL pattern is invalid"))
}

fn present(
  value: Option<v8::Local<v8::Value>>,
) -> Option<v8::Local<v8::Value>> {
  value.filter(|value| !value.is_null_or_undefined())
}

fn null_js(scope: &mut v8::PinScope) -> Js {
  let value: v8::Local<v8::Value> = v8::null(scope).into();
  Js(v8::Global::new(scope, value))
}

fn is_init_object(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> bool {
  if value.is_string() || !value.is_object() {
    return false;
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return false;
  };
  !object_is_url(scope, object)
}

fn classify_match(
  scope: &mut v8::PinScope,
  input: Option<v8::Local<v8::Value>>,
  base: Option<&str>,
) -> Result<Option<UrlPatternMatchInput>, OpError> {
  let Some(input) = input.filter(|value| !is_init_object(scope, *value)) else {
    if base.is_some() {
      return Err(OpError::type_(
        "A base URL requires a string URL pattern input",
      ));
    }
    let Some(init) = read_init(scope, input, None, false)? else {
      return Ok(None);
    };
    return Ok(Some(UrlPatternMatchInput::Init(init)));
  };
  let href = text(scope, input)?;
  let Some(url) = resolve_url(&href, base) else {
    return Ok(None);
  };
  Ok(Some(UrlPatternMatchInput::Url(url)))
}

fn resolve_url(href: &str, base: Option<&str>) -> Option<url::Url> {
  let base = match base {
    Some(base) => match url::Url::parse(base) {
      Ok(url) => Some(url),
      Err(_) => return None,
    },
    None => None,
  };
  url::Url::options().base_url(base.as_ref()).parse(href).ok()
}

fn result_object(
  scope: &mut v8::PinScope,
  result: &urlpattern::UrlPatternResult,
  input: Option<v8::Local<v8::Value>>,
  base: Option<v8::Local<v8::Value>>,
) -> Js {
  let object = v8::Object::new(scope);
  set_component(scope, object, "protocol", &result.protocol);
  set_component(scope, object, "username", &result.username);
  set_component(scope, object, "password", &result.password);
  set_component(scope, object, "hostname", &result.hostname);
  set_component(scope, object, "port", &result.port);
  set_component(scope, object, "pathname", &result.pathname);
  set_component(scope, object, "search", &result.search);
  set_component(scope, object, "hash", &result.hash);
  let input = input.filter(|value| !value.is_null_or_undefined());
  let base = base.filter(|value| !value.is_null_or_undefined());
  let inputs = v8::Array::new(
    scope,
    i32::from(input.is_some()) + i32::from(base.is_some()),
  );
  let mut index = 0;
  if let Some(input) = input {
    inputs.set_index(scope, index, input);
    index += 1;
  }
  if let Some(base) = base {
    inputs.set_index(scope, index, base);
  }
  let key = v8::String::new(scope, "inputs").unwrap();
  object.set(scope, key.into(), inputs.into());
  let value: v8::Local<v8::Value> = object.into();
  Js(v8::Global::new(scope, value))
}

fn set_component(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  component: &urlpattern::UrlPatternComponentResult,
) {
  let record = v8::Object::new(scope);
  let input = v8::String::new(scope, &component.input).unwrap();
  let input_key = v8::String::new(scope, "input").unwrap();
  record.set(scope, input_key.into(), input.into());
  let groups = v8::Object::new(scope);
  for (name, value) in &component.groups {
    let key = v8::String::new(scope, name).unwrap();
    let value = match value {
      Some(value) => {
        let text = v8::String::new(scope, value).unwrap();
        let value: v8::Local<v8::Value> = text.into();
        value
      }
      None => v8::undefined(scope).into(),
    };
    groups.set(scope, key.into(), value);
  }
  let groups_key = v8::String::new(scope, "groups").unwrap();
  record.set(scope, groups_key.into(), groups.into());
  let key = v8::String::new(scope, name).unwrap();
  object.set(scope, key.into(), record.into());
}

pub fn install(scope: &mut v8::PinScope) {
  UrlPattern::install(scope);
  define_has_regexp_groups(scope);
}

fn define_has_regexp_groups(scope: &mut v8::PinScope) {
  let global = scope.get_current_context().global(scope);
  let class_key = v8::String::new(scope, "URLPattern").unwrap();
  let Some(ctor) = global.get(scope, class_key.into()) else {
    return;
  };
  let Ok(ctor) = v8::Local::<v8::Object>::try_from(ctor) else {
    return;
  };
  let proto_key = v8::String::new(scope, "prototype").unwrap();
  let Some(proto) = ctor.get(scope, proto_key.into()) else {
    return;
  };
  let Ok(proto) = v8::Local::<v8::Object>::try_from(proto) else {
    return;
  };
  let name = v8::String::new(scope, "hasRegExpGroups").unwrap();
  let Some(function) = v8::Function::builder(has_regexp_groups_getter)
    .length(0)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
  else {
    return;
  };
  function.set_name(name);
  let get: v8::Local<v8::Value> = function.into();
  let set: v8::Local<v8::Value> = v8::undefined(scope).into();
  let mut descriptor = v8::PropertyDescriptor::new_from_get_set(get, set);
  descriptor.set_enumerable(true);
  descriptor.set_configurable(true);
  let _ = proto.define_property(scope, name.into(), &descriptor);
}

fn has_regexp_groups_getter(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let object = args.this();
  let Some(host) =
    (unsafe { yuzuko_core::op::host_ref::<UrlPattern>(scope, object) })
  else {
    yuzuko_core::op::OpException::throw(
      OpError::type_("illegal invocation"),
      scope,
    );
    return;
  };
  ret.set_bool(host.get().inner.has_regexp_groups());
}

fn parts_of(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  component: &str,
) -> Result<Vec<urlpattern::parser::Part>, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| {
    OpError::type_("URLPattern.compareComponent requires a URLPattern")
  })?;
  let wrapped =
    unsafe { yuzuko_core::op::host_ref::<UrlPattern>(scope, object) }
      .ok_or_else(|| {
        OpError::type_("URLPattern.compareComponent requires a URLPattern")
      })?;
  component_parts(&wrapped.get().inner, component)
}

fn component_text<'a>(
  pattern: &'a CompiledPattern,
  component: &str,
) -> Result<&'a str, OpError> {
  Ok(match component {
    "protocol" => pattern.protocol(),
    "username" => pattern.username(),
    "password" => pattern.password(),
    "hostname" => pattern.hostname(),
    "port" => pattern.port(),
    "pathname" => pattern.pathname(),
    "search" => pattern.search(),
    "hash" => pattern.hash(),
    _ => {
      return Err(OpError::type_("The URL pattern component is invalid"));
    }
  })
}

fn protocol_is_standard(pattern: &CompiledPattern) -> bool {
  ["ftp", "file", "http", "https", "ws", "wss"]
    .into_iter()
    .any(|scheme| pattern.protocol.matcher.matches(scheme).is_some())
}

fn component_options(
  pattern: &CompiledPattern,
  component: &str,
) -> urlpattern::parser::Options {
  match component {
    "hostname" => urlpattern::parser::Options::hostname(),
    "pathname" => {
      let non_opaque = pattern.protocol().is_empty()
        || protocol_is_standard(pattern)
        || pattern.pathname().starts_with('/');
      if non_opaque {
        urlpattern::parser::Options::pathname()
      } else {
        urlpattern::parser::Options::default()
      }
    }
    _ => urlpattern::parser::Options::default(),
  }
}

fn component_parts(
  pattern: &CompiledPattern,
  component: &str,
) -> Result<Vec<urlpattern::parser::Part>, OpError> {
  let text = component_text(pattern, component)?;
  let options = component_options(pattern, component);
  urlpattern::parser::parse_pattern_string(text, &options, |value| {
    Ok(value.to_string())
  })
  .map_err(|error| OpError::type_(error.to_string()))
}

fn type_rank(kind: &urlpattern::parser::PartType) -> u8 {
  match kind {
    urlpattern::parser::PartType::FullWildcard => 0,
    urlpattern::parser::PartType::SegmentWildcard => 1,
    urlpattern::parser::PartType::Regexp => 2,
    urlpattern::parser::PartType::FixedText => 3,
  }
}

fn modifier_rank(modifier: &urlpattern::parser::PartModifier) -> u8 {
  match modifier {
    urlpattern::parser::PartModifier::ZeroOrMore => 0,
    urlpattern::parser::PartModifier::Optional => 1,
    urlpattern::parser::PartModifier::OneOrMore => 2,
    urlpattern::parser::PartModifier::None => 3,
  }
}

fn compare_part(
  left: &urlpattern::parser::Part,
  right: &urlpattern::parser::Part,
) -> i32 {
  let left_key = (
    type_rank(&left.kind),
    modifier_rank(&left.modifier),
    left.prefix.as_str(),
    left.value.as_str(),
    left.suffix.as_str(),
  );
  let right_key = (
    type_rank(&right.kind),
    modifier_rank(&right.modifier),
    right.prefix.as_str(),
    right.value.as_str(),
    right.suffix.as_str(),
  );
  match left_key.cmp(&right_key) {
    std::cmp::Ordering::Less => -1,
    std::cmp::Ordering::Equal => 0,
    std::cmp::Ordering::Greater => 1,
  }
}

fn empty_fixed() -> urlpattern::parser::Part {
  urlpattern::parser::Part {
    kind: urlpattern::parser::PartType::FixedText,
    value: String::new(),
    modifier: urlpattern::parser::PartModifier::None,
    name: String::new(),
    prefix: String::new(),
    suffix: String::new(),
  }
}

fn compare_parts(
  left: &[urlpattern::parser::Part],
  right: &[urlpattern::parser::Part],
) -> i32 {
  let shared = left.len().min(right.len());
  for index in 0..shared {
    let result = compare_part(&left[index], &right[index]);
    if result != 0 {
      return result;
    }
  }
  if left.len() == right.len() {
    return 0;
  }
  let empty = empty_fixed();
  if left.len() < right.len() {
    compare_part(&empty, &right[shared])
  } else {
    compare_part(&left[shared], &empty)
  }
}

fn read_groups(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<std::collections::HashMap<String, String>, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The URL pattern groups are invalid"))?;
  let names = object
    .get_own_property_names(scope, v8::GetPropertyNamesArgs::default())
    .ok_or_else(|| OpError::type_("The URL pattern groups are invalid"))?;
  let mut groups = std::collections::HashMap::new();
  for index in 0..names.length() {
    let key = names
      .get_index(scope, index)
      .ok_or_else(|| OpError::type_("The URL pattern groups are invalid"))?;
    let group_value = object
      .get(scope, key)
      .ok_or_else(|| OpError::type_("The URL pattern groups are invalid"))?;
    let name = key
      .to_string(scope)
      .map(|text| text.to_rust_string_lossy(scope))
      .unwrap_or_default();
    let text = group_value
      .to_string(scope)
      .map(|text| text.to_rust_string_lossy(scope))
      .ok_or_else(|| OpError::type_("The URL pattern groups are invalid"))?;
    groups.insert(name, text);
  }
  Ok(groups)
}

fn generate_component(
  pattern: &CompiledPattern,
  component: &str,
  groups: &std::collections::HashMap<String, String>,
) -> Result<String, OpError> {
  let parts = component_parts(pattern, component)?;
  let standard = protocol_is_standard(pattern);
  let mut result = String::new();
  for part in parts {
    if part.modifier != urlpattern::parser::PartModifier::None {
      return Err(OpError::type_("URL pattern modifiers cannot be generated"));
    }
    match part.kind {
      urlpattern::parser::PartType::FixedText => {
        result.push_str(&part.value);
      }
      urlpattern::parser::PartType::SegmentWildcard => {
        if part
          .name
          .chars()
          .next()
          .is_some_and(|char| char.is_ascii_digit())
        {
          return Err(OpError::type_(
            "A numeric URL pattern group cannot be generated",
          ));
        }
        let Some(value) = groups.get(&part.name) else {
          return Err(OpError::type_(format!(
            "No input found for `{}`",
            part.name
          )));
        };
        let encoded = encode_group(component, standard, value)?;
        if let Some(delimiter) = component_delimiter(component, standard)
          && encoded.contains(delimiter)
        {
          return Err(OpError::type_(format!(
            "The value `{encoded}` contains the delimiter `{delimiter}`"
          )));
        }
        result.push_str(&part.prefix);
        result.push_str(&encoded);
        result.push_str(&part.suffix);
      }
      urlpattern::parser::PartType::FullWildcard
      | urlpattern::parser::PartType::Regexp => {
        return Err(OpError::type_(
          "URL pattern wildcards and regular expressions cannot be generated",
        ));
      }
    }
  }
  Ok(result)
}

fn component_delimiter(
  component: &str,
  standard_pathname: bool,
) -> Option<char> {
  match component {
    "pathname" if standard_pathname => Some('/'),
    "hostname" => Some('.'),
    _ => None,
  }
}

fn encode_group(
  component: &str,
  standard_pathname: bool,
  value: &str,
) -> Result<String, OpError> {
  let encoded = match component {
    "protocol" => canonicalize_protocol(value),
    "username" => canonicalize_username(value),
    "password" => canonicalize_password(value),
    "hostname" => canonicalize_hostname(value),
    "port" => canonicalize_port(value),
    "pathname" if standard_pathname => canonicalize_pathname(value),
    "pathname" => canonicalize_opaque_pathname(value),
    "search" => canonicalize_search(value),
    "hash" => canonicalize_hash(value),
    _ => return Err(OpError::type_("The URL pattern component is invalid")),
  };
  encoded.map_err(|_| OpError::type_("The URL pattern group is invalid"))
}

fn canonicalize_protocol(value: &str) -> Result<String, ()> {
  if value.is_empty() {
    return Ok(String::new());
  }
  url::Url::parse(&format!("{value}://dummy.test"))
    .map(|url| url.scheme().to_owned())
    .map_err(|_| ())
}

fn canonicalize_username(value: &str) -> Result<String, ()> {
  if value.is_empty() {
    return Ok(String::new());
  }
  let mut url = url::Url::parse("http://dummy.test").unwrap();
  url.set_username(value).map_err(|_| ())?;
  Ok(url.username().to_string())
}

fn canonicalize_password(value: &str) -> Result<String, ()> {
  if value.is_empty() {
    return Ok(String::new());
  }
  let mut url = url::Url::parse("http://dummy.test").unwrap();
  url.set_password(Some(value)).map_err(|_| ())?;
  Ok(url.password().unwrap_or("").to_string())
}

fn canonicalize_hostname(value: &str) -> Result<String, ()> {
  if value.is_empty() {
    return Ok(String::new());
  }
  let mut url = url::Url::parse("http://dummy.test").unwrap();
  url::quirks::set_hostname(&mut url, value).map_err(|_| ())?;
  Ok(url::quirks::hostname(&url).to_string())
}

fn canonicalize_port(value: &str) -> Result<String, ()> {
  if value.is_empty() {
    return Ok(String::new());
  }
  let mut url = url::Url::parse("http://dummy.test").unwrap();
  url::quirks::set_port(&mut url, value).map_err(|_| ())?;
  Ok(url::quirks::port(&url).to_string())
}

fn canonicalize_pathname(value: &str) -> Result<String, ()> {
  if value.is_empty() {
    return Ok(String::new());
  }
  let leading_slash = value.starts_with('/');
  let modified = if leading_slash {
    value.to_string()
  } else {
    format!("/-{value}")
  };
  let mut url = url::Url::parse("http://dummy.test").unwrap();
  url.set_path(&modified);
  let mut pathname = url::quirks::pathname(&url).to_string();
  if !leading_slash && pathname.starts_with("/-") {
    pathname = pathname[2..].to_string();
  }
  Ok(pathname)
}

fn canonicalize_opaque_pathname(value: &str) -> Result<String, ()> {
  if value.is_empty() {
    return Ok(String::new());
  }
  let mut url = url::Url::parse("data:dummy,test").unwrap();
  url.set_path(value);
  Ok(url::quirks::pathname(&url).to_string())
}

fn canonicalize_search(value: &str) -> Result<String, ()> {
  if value.is_empty() {
    return Ok(String::new());
  }
  let mut url = url::Url::parse("http://dummy.test").unwrap();
  url.set_query(Some(value));
  Ok(url.query().unwrap_or("").to_string())
}

fn canonicalize_hash(value: &str) -> Result<String, ()> {
  if value.is_empty() {
    return Ok(String::new());
  }
  let mut url = url::Url::parse("http://dummy.test").unwrap();
  url.set_fragment(Some(value));
  Ok(url.fragment().unwrap_or("").to_string())
}

thread_local! {
  static REGEXP_SCOPE: Cell<*mut ()> = const { Cell::new(std::ptr::null_mut()) };
}

struct RegexpScopeGuard(*mut ());

impl Drop for RegexpScopeGuard {
  fn drop(&mut self) {
    REGEXP_SCOPE.with(|cell| cell.set(self.0));
  }
}

fn enter_regexp_scope(scope: &mut v8::PinScope) -> RegexpScopeGuard {
  let previous =
    REGEXP_SCOPE.with(|cell| cell.replace(std::ptr::from_mut(scope).cast()));
  RegexpScopeGuard(previous)
}

fn with_current_regexp_scope<R>(
  body: impl FnOnce(&mut v8::PinScope) -> R,
) -> Option<R> {
  REGEXP_SCOPE.with(|cell| {
    let pointer = cell.get();
    if pointer.is_null() {
      return None;
    }
    // The URLPattern op published this HandleScope for the synchronous
    // `RegExp` trait calls. Those methods cannot take the scope, and the
    // guard keeps the pointer valid until the op returns.
    let scope = unsafe { &mut *pointer.cast::<v8::PinScope>() };
    Some(body(scope))
  })
}

#[derive(Debug)]
struct EcmaRegExp {
  pattern: String,
  flags: String,
}

impl urlpattern::regexp::RegExp for EcmaRegExp {
  fn syntax() -> urlpattern::RegexSyntax {
    urlpattern::RegexSyntax::EcmaScript
  }

  fn parse(pattern: &str, flags: &str, force_eval: bool) -> Result<Self, ()> {
    if force_eval && !regexp_compiles(pattern, flags) {
      return Err(());
    }
    Ok(Self {
      pattern: pattern.to_owned(),
      flags: flags.to_owned(),
    })
  }

  fn matches<'a>(&self, text: &'a str) -> Option<Vec<Option<&'a str>>> {
    regexp_captures(&self.pattern, &self.flags, text)
  }

  fn pattern_string(&self) -> &str {
    &self.pattern
  }
}

fn regexp_flags(flags: &str) -> v8::RegExpCreationFlags {
  // The URL Pattern standard compiles with the `v` flag. `d` exposes
  // capture indexes so an unmatched group stays undefined.
  let mut result = v8::RegExpCreationFlags::UNICODE_SETS
    | v8::RegExpCreationFlags::HAS_INDICES;
  if flags.contains('i') {
    result |= v8::RegExpCreationFlags::IGNORE_CASE;
  }
  if flags.contains('m') {
    result |= v8::RegExpCreationFlags::MULTILINE;
  }
  if flags.contains('s') {
    result |= v8::RegExpCreationFlags::DOT_ALL;
  }
  result
}

fn regexp_compiles(pattern: &str, flags: &str) -> bool {
  with_current_regexp_scope(|scope| {
    v8::tc_scope!(let scope, scope);
    let Some(source) = v8::String::new(scope, pattern) else {
      if scope.has_caught() {
        scope.reset();
      }
      return false;
    };
    let created = v8::RegExp::new(scope, source, regexp_flags(flags));
    let compiled = created.is_some() && !scope.has_caught();
    if scope.has_caught() {
      scope.reset();
    }
    compiled
  })
  .unwrap_or(false)
}

fn regexp_captures<'a>(
  pattern: &str,
  flags: &str,
  text: &'a str,
) -> Option<Vec<Option<&'a str>>> {
  let ranges = with_current_regexp_scope(|scope| {
    capture_ranges(scope, pattern, flags, text)
  })??;
  Some(
    ranges
      .into_iter()
      .map(|range| {
        range.and_then(|(start, end)| {
          let end = end.min(text.len());
          let start = start.min(end);
          text.get(start..end)
        })
      })
      .collect(),
  )
}

fn capture_ranges(
  scope: &mut v8::PinScope,
  pattern: &str,
  flags: &str,
  text: &str,
) -> Option<Vec<Option<(usize, usize)>>> {
  v8::tc_scope!(let scope, scope);
  let source = v8::String::new(scope, pattern)?;
  let subject = v8::String::new(scope, text)?;
  let regexp = v8::RegExp::new(scope, source, regexp_flags(flags));
  if scope.has_caught() {
    scope.reset();
    return None;
  }
  let result = regexp?.exec(scope, subject);
  if scope.has_caught() {
    scope.reset();
    return None;
  }
  let array = v8::Local::<v8::Array>::try_from(result?).ok()?;
  let indices_key = v8::String::new(scope, "indices")?;
  let indices = array.get(scope, indices_key.into())?;
  let indices = v8::Local::<v8::Array>::try_from(indices).ok()?;
  let mut ranges = Vec::new();
  for index in 1..array.length() {
    let value = array.get_index(scope, index)?;
    if value.is_null_or_undefined() {
      ranges.push(None);
      continue;
    }
    let pair = indices.get_index(scope, index)?;
    if pair.is_null_or_undefined() {
      ranges.push(None);
      continue;
    }
    let Ok(pair) = v8::Local::<v8::Array>::try_from(pair) else {
      ranges.push(None);
      continue;
    };
    let start = pair.get_index(scope, 0)?.uint32_value(scope)? as usize;
    let end = pair.get_index(scope, 1)?.uint32_value(scope)? as usize;
    ranges.push(Some((
      utf16_index_to_byte(text, start),
      utf16_index_to_byte(text, end),
    )));
  }
  Some(ranges)
}

fn utf16_index_to_byte(text: &str, index: usize) -> usize {
  let mut units = 0;
  for (byte, scalar) in text.char_indices() {
    if units >= index {
      return byte;
    }
    units += scalar.len_utf16();
  }
  text.len()
}

#[cfg(test)]
mod tests {
  use super::utf16_index_to_byte;

  #[test]
  fn utf16_indexes_follow_scalar_values() {
    let text = "a😀b";
    assert_eq!(utf16_index_to_byte(text, 0), 0);
    assert_eq!(utf16_index_to_byte(text, 1), 1);
    assert_eq!(utf16_index_to_byte(text, 3), 5);
    assert_eq!(utf16_index_to_byte(text, 4), text.len());
  }
}
