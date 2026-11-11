//! System DNS lookup shared by `connectTcp`, `Datagram.send`, `resolveDns`,
//! and `connectTls`.
//!
//! The host string is not rewritten. A trailing DNS root dot stays, and
//! `localhost` is not turned into `127.0.0.1`. The caller has already checked
//! the name. [`lookup`] checks resolved restricted addresses again.
//! [`records`] does not: `resolveDns` is allowed by the host grant alone.

use std::net::{SocketAddr, ToSocketAddrs};

use crate::op::{OpError, OpException, ToV8};

pub(crate) const UNRESOLVED: &str = "The host could not be resolved.";

pub(crate) enum LookupError {
  Resolve,
  Denied { name: String, message: String },
}

/// Addresses from the system resolver, after the restricted-address check.
///
/// An empty result is [`LookupError::Resolve`]. A denial of every address is
/// [`LookupError::Denied`].
pub(crate) fn lookup(
  host: &str,
  port: u16,
) -> Result<Vec<SocketAddr>, LookupError> {
  let found = system_addrs(host, port)?;
  match crate::permissions::retain_resolved_addresses(found) {
    Ok(addrs) if addrs.is_empty() => Err(LookupError::Resolve),
    Ok(addrs) => Ok(addrs),
    Err(OpError::Dom { name, message }) => {
      Err(LookupError::Denied { name, message })
    }
    Err(_) => Err(LookupError::Resolve),
  }
}

/// Address strings of one family. No records of that family is an empty
/// vector. A name that does not resolve is [`LookupError::Resolve`].
pub(crate) fn records(
  host: &str,
  v6: bool,
) -> Result<Vec<String>, LookupError> {
  // Port 9 only satisfies `to_socket_addrs`. It is not a permission check.
  let addrs = system_addrs(host, 9)?;
  let mut out = Vec::new();
  for addr in addrs {
    if addr.is_ipv6() != v6 {
      continue;
    }
    let text = addr.ip().to_string();
    if !out.iter().any(|have| have == &text) {
      out.push(text);
    }
  }
  Ok(out)
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  let yuzuko = crate::namespace(scope, "Yuzuko");
  define_fn(scope, yuzuko, "resolveDns", 2, resolve_dns);
}

fn system_addrs(host: &str, port: u16) -> Result<Vec<SocketAddr>, LookupError> {
  let query = query_host(host);
  let dial = format!("{query}:{port}");
  let found = crate::process::with_env(|| dial.to_socket_addrs())
    .map_err(|_| LookupError::Resolve)?
    .collect::<Vec<_>>();
  if found.is_empty() {
    Err(LookupError::Resolve)
  } else {
    Ok(found)
  }
}

fn query_host(host: &str) -> String {
  if host.contains(':') && !host.starts_with('[') {
    format!("[{host}]")
  } else {
    host.to_string()
  }
}

fn resolve_dns(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  if args.length() < 2 {
    crate::js::throw_type(scope, "Not enough arguments");
    return;
  }
  let host_value = args.get(0);
  if !host_value.is_string() {
    crate::js::throw_type(scope, "The host must be a string.");
    return;
  }
  let Some(host) = host_value.to_string(scope) else {
    crate::js::throw_type(scope, "The host must be a string.");
    return;
  };
  let host = host.to_rust_string_lossy(scope);
  if host.is_empty() {
    crate::js::throw_type(scope, "The host must not be empty.");
    return;
  }
  let type_value = args.get(1);
  if !type_value.is_string() {
    crate::js::throw_type(scope, "The record type must be \"A\" or \"AAAA\".");
    return;
  }
  let Some(record) = type_value.to_string(scope) else {
    crate::js::throw_type(scope, "The record type must be \"A\" or \"AAAA\".");
    return;
  };
  let record = record.to_rust_string_lossy(scope);
  let v6 = match record.as_str() {
    "A" => false,
    "AAAA" => true,
    _ => {
      crate::js::throw_type(
        scope,
        "The record type must be \"A\" or \"AAAA\".",
      );
      return;
    }
  };
  if let Err(error) = crate::permissions::check_net_host(&host) {
    error.throw(scope);
    return;
  }
  let host = host.trim().to_string();
  match settle(scope, move || records(&host, v6)) {
    Ok(value) => value.to_v8(scope, &mut ret),
    Err(error) => crate::op::OpException::throw(error, scope),
  }
}

fn settle(
  scope: &mut v8::PinScope,
  work: impl FnOnce() -> Result<Vec<String>, LookupError> + Send + 'static,
) -> Result<crate::op::Js, OpError> {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Err(OpError::type_("The socket operation failed."));
  };
  let promise = resolver.get_promise(scope);
  let promise_value: v8::Local<v8::Value> = promise.into();
  let promise_global = v8::Global::new(scope, promise_value);
  let resolver_global = v8::Global::new(scope, resolver);
  crate::jobs::spawn(work, move |scope, result| {
    let resolver = v8::Local::new(scope, &resolver_global);
    let outcome = match result {
      Ok(value) => value,
      Err(()) => Err(LookupError::Resolve),
    };
    match outcome {
      Ok(addrs) => {
        if let Some(value) = string_array(scope, &addrs) {
          let _ = resolver.resolve(scope, value);
        }
      }
      Err(LookupError::Resolve) => {
        let error = error_value(scope, UNRESOLVED);
        let _ = resolver.reject(scope, error);
      }
      Err(LookupError::Denied { name, message }) => {
        let error = crate::errors::denied_value(scope, &name, &message);
        let _ = resolver.reject(scope, error);
      }
    }
  });
  Ok(crate::op::Js(promise_global))
}

fn string_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  addrs: &[String],
) -> Option<v8::Local<'s, v8::Value>> {
  let array = v8::Array::new(scope, addrs.len() as i32);
  for (index, addr) in addrs.iter().enumerate() {
    let text = v8::String::new(scope, addr)?;
    array.set_index(scope, index as u32, text.into())?;
  }
  Some(array.into())
}

fn error_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  message: &str,
) -> v8::Local<'s, v8::Value> {
  let Some(text) = v8::String::new(scope, message) else {
    return v8::undefined(scope).into();
  };
  v8::Exception::error(scope, text)
}

fn define_fn(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  length: i32,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::builder(callback)
    .length(length)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap_or_else(|| panic!("create function {name}"));
  let key = v8::String::new(scope, name).unwrap();
  function.set_name(key);
  object.set(scope, key.into(), function.into());
}
