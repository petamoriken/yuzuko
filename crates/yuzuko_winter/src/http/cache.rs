//! In-memory Cache Storage. A stored request keeps its URL and method and
//! gets a fresh `AbortSignal`. The original signal is not retained.

use std::cell::RefCell;
use std::collections::HashMap;

use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_macros::op;

thread_local! {
  static CACHES: RefCell<HashMap<String, Vec<String>>> =
    RefCell::new(HashMap::new());
}

pub(crate) fn clear() {
  CACHES.with(|slot| slot.borrow_mut().clear());
}

pub struct CacheStorage;

unsafe impl v8::cppgc::GarbageCollected for CacheStorage {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"CacheStorage"
  }
}

pub struct Cache {
  name: String,
}

unsafe impl v8::cppgc::GarbageCollected for Cache {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Cache"
  }
}

#[op(name = "CacheStorage")]
impl CacheStorage {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[promise]
  fn open(&self, scope: &mut v8::PinScope, name: String) -> Js {
    let _ = self;
    CACHES.with(|slot| {
      slot.borrow_mut().entry(name.clone()).or_default();
    });
    let cache =
      unsafe { yuzuko_core::op::wrap_instance(scope, Cache { name }) };
    resolve(scope, cache.into())
  }

  #[promise]
  fn delete(&self, scope: &mut v8::PinScope, name: String) -> Js {
    let _ = self;
    let removed = CACHES.with(|slot| slot.borrow_mut().remove(&name).is_some());
    resolve(scope, v8::Boolean::new(scope, removed).into())
  }
}

#[op]
impl Cache {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[promise]
  fn put(
    &self,
    scope: &mut v8::PinScope,
    request: v8::Local<v8::Value>,
    response: v8::Local<v8::Value>,
  ) -> Js {
    if response.is_null_or_undefined() {
      return reject(scope, "Cache.put requires a Response");
    }
    let (method, url) =
      match crate::http::request::request_method_url(scope, request) {
        Ok(record) => record,
        Err(OpError::Type(message) | OpError::Range(message)) => {
          return reject(scope, &message);
        }
        Err(OpError::Dom { message, .. }) => return reject(scope, &message),
        Err(OpError::Thrown(_)) => {
          return reject(scope, "Cache.put requires a Request");
        }
      };
    if method != "GET" {
      return reject(scope, "Cache.put only stores GET requests");
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
      return reject(scope, "Cache.put requires an HTTP URL");
    }
    CACHES.with(|slot| {
      let mut caches = slot.borrow_mut();
      let entries = caches.entry(self.name.clone()).or_default();
      if !entries.iter().any(|stored| stored == &url) {
        entries.push(url);
      }
    });
    resolve(scope, v8::undefined(scope).into())
  }

  #[promise]
  fn keys(&self, scope: &mut v8::PinScope) -> Js {
    let urls = CACHES
      .with(|slot| slot.borrow().get(&self.name).cloned().unwrap_or_default());
    let array = v8::Array::new(scope, urls.len() as i32);
    for (index, url) in urls.iter().enumerate() {
      let Ok(request) = new_get_request(scope, url) else {
        return reject(scope, "The cached request could not be created");
      };
      array.set_index(scope, index as u32, request.into());
    }
    resolve(scope, array.into())
  }
}

pub fn install(scope: &mut v8::PinScope) {
  Cache::install(scope);
  CacheStorage::install(scope);
  let caches = unsafe { yuzuko_core::op::wrap_instance(scope, CacheStorage) };
  let getter = v8::Function::builder(caches_attribute)
    .data(caches.into())
    .length(0)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap();
  let getter_name = v8::String::new(scope, "get caches").unwrap();
  getter.set_name(getter_name);
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "caches").unwrap();
  let get: v8::Local<v8::Value> = getter.into();
  let set: v8::Local<v8::Value> = v8::undefined(scope).into();
  let mut descriptor = v8::PropertyDescriptor::new_from_get_set(get, set);
  descriptor.set_enumerable(true);
  descriptor.set_configurable(true);
  let _ = global.define_property(scope, key.into(), &descriptor);
}

fn caches_attribute(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let global = scope.get_current_context().global(scope);
  let this = args.this();
  if this.is_null_or_undefined() || this.strict_equals(global.into()) {
    ret.set(args.data());
    return;
  }
  yuzuko_core::op::OpException::throw(
    OpError::type_("illegal invocation"),
    scope,
  );
}

fn new_get_request<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  url: &str,
) -> Result<v8::Local<'s, v8::Object>, ()> {
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "Request").unwrap();
  let Some(ctor) = global.get(scope, key.into()) else {
    return Err(());
  };
  let Ok(ctor) = v8::Local::<v8::Function>::try_from(ctor) else {
    return Err(());
  };
  let url = v8::String::new(scope, url).unwrap();
  ctor.new_instance(scope, &[url.into()]).ok_or(())
}

fn resolve(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  resolver.resolve(scope, value);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

fn reject(scope: &mut v8::PinScope, message: &str) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let message = v8::String::new(scope, message).unwrap();
  let error = v8::Exception::type_error(scope, message);
  resolver.reject(scope, error);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}
