use yuzuko_core::op::OpError;
use yuzuko_macros::op;

pub(crate) use yuzuko_core::base64::{
  decode_forgiving, decode_url, encode, encode_url,
};

pub fn install(scope: &mut v8::PinScope) {
  let global = scope.get_current_context().global(scope);
  set_fn(scope, global, "btoa", btoa);
  set_fn(scope, global, "atob", atob);
}

fn set_fn(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::new(scope, callback).unwrap();
  let key = v8::String::new(scope, name).unwrap();
  object.set(scope, key.into(), function.into());
}

#[op]
fn btoa(input: String) -> Result<String, OpError> {
  encode_latin1(&input).ok_or_else(|| {
    OpError::dom(
      "InvalidCharacterError",
      "btoa failed: the string contains characters outside of the Latin1 range",
    )
  })
}

#[op]
fn atob(input: String) -> Result<String, OpError> {
  decode_forgiving(&input)
    .map(|decoded| decoded.iter().copied().map(char::from).collect())
    .map_err(|_| {
      OpError::dom(
        "InvalidCharacterError",
        "atob failed: the string is not correctly encoded",
      )
    })
}

fn encode_latin1(input: &str) -> Option<String> {
  let mut bytes = Vec::with_capacity(input.len());
  for ch in input.chars() {
    let code = u32::from(ch);
    if code > 0xFF {
      return None;
    }
    bytes.push(code as u8);
  }
  Some(encode(&bytes))
}
