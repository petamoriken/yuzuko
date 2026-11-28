//! In-memory Cache Storage. A stored request keeps its URL and method and
//! gets a fresh `AbortSignal`. The original signal is not retained.
//!
//! One GET `http` or `https` URL is one entry. A later `put` of that URL
//! replaces the body in place. The store is dropped when the realm drops.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use yuzuko_core::op::Js;
use yuzuko_core::op::OpError;
use yuzuko_core::op::OpException;
use yuzuko_macros::op;

use crate::http::fetch::{CachedBody, CachedResponse};
use crate::util::resolve;

thread_local! {
  static STORAGE: RefCell<Storage> = RefCell::new(Storage::empty());
  static JOBS: RefCell<HashMap<u64, Job>> = RefCell::new(HashMap::new());
  static NEXT_JOB: Cell<u64> = const { Cell::new(1) };
}

struct Storage {
  generation: u64,
  next_cache: u64,
  names: Vec<String>,
  caches: HashMap<String, NamedCache>,
}

struct NamedCache {
  generation: u64,
  urls: Vec<String>,
  entries: HashMap<String, StoredResponse>,
  next_seq: u64,
  seqs: HashMap<String, u64>,
}

#[derive(Clone)]
struct StoredResponse {
  status: u16,
  status_text: String,
  response_url: String,
  redirected: bool,
  kind: String,
  headers: Vec<(String, String)>,
  body: CacheBody,
}

#[derive(Clone)]
enum CacheBody {
  Null,
  Bytes(Vec<u8>),
}

struct PutJob {
  storage_generation: u64,
  name: String,
  cache_generation: u64,
  url: String,
  seq: u64,
  status: u16,
  status_text: String,
  response_url: String,
  redirected: bool,
  kind: String,
  headers: Vec<(String, String)>,
  resolver: v8::Global<v8::PromiseResolver>,
}

struct AddJob {
  storage_generation: u64,
  name: String,
  cache_generation: u64,
  current_url: String,
  rest: Vec<QueuedRequest>,
  done: Vec<ReadyRecord>,
  waiting: Option<PendingRecord>,
  resolver: v8::Global<v8::PromiseResolver>,
}

struct QueuedRequest {
  object: v8::Global<v8::Object>,
  url: String,
}

struct ReadyRecord {
  url: String,
  stored: StoredResponse,
}

struct PendingRecord {
  url: String,
  status: u16,
  status_text: String,
  response_url: String,
  redirected: bool,
  kind: String,
  headers: Vec<(String, String)>,
}

enum Job {
  Put(PutJob),
  Add(AddJob),
}

struct Query {
  ignore_search: bool,
  ignore_method: bool,
  cache_name: Option<String>,
}

impl Storage {
  fn empty() -> Self {
    Self {
      generation: 0,
      next_cache: 0,
      names: Vec::new(),
      caches: HashMap::new(),
    }
  }
}

impl Query {
  fn fresh() -> Self {
    Self {
      ignore_search: false,
      ignore_method: false,
      cache_name: None,
    }
  }
}

pub(crate) fn clear() {
  JOBS.with(|slot| slot.borrow_mut().clear());
  STORAGE.with(|slot| {
    let mut storage = slot.borrow_mut();
    storage.generation = storage.generation.saturating_add(1);
    storage.names.clear();
    storage.caches.clear();
  });
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
  generation: u64,
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
    let generation = STORAGE.with(|slot| {
      let mut storage = slot.borrow_mut();
      if let Some(cache) = storage.caches.get(&name) {
        return cache.generation;
      }
      storage.next_cache = storage.next_cache.saturating_add(1);
      let generation = storage.next_cache;
      storage.names.push(name.clone());
      storage.caches.insert(
        name.clone(),
        NamedCache {
          generation,
          urls: Vec::new(),
          entries: HashMap::new(),
          next_seq: 0,
          seqs: HashMap::new(),
        },
      );
      generation
    });
    let cache = unsafe {
      yuzuko_core::op::wrap_instance(scope, Cache { name, generation })
    };
    resolve(scope, cache.into())
  }

  #[promise]
  fn delete(&self, scope: &mut v8::PinScope, name: String) -> Js {
    let _ = self;
    let removed = STORAGE.with(|slot| {
      let mut storage = slot.borrow_mut();
      let had = storage.caches.remove(&name).is_some();
      storage.names.retain(|found| found != &name);
      had
    });
    resolve(scope, v8::Boolean::new(scope, removed).into())
  }

  #[promise]
  #[length(1)]
  fn has(&self, scope: &mut v8::PinScope, name: String) -> Js {
    let _ = self;
    let found = STORAGE.with(|slot| slot.borrow().caches.contains_key(&name));
    resolve(scope, v8::Boolean::new(scope, found).into())
  }

  #[promise]
  fn keys(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    let names = STORAGE.with(|slot| slot.borrow().names.clone());
    let array = v8::Array::new(scope, names.len() as i32);
    for (index, name) in names.iter().enumerate() {
      let text = v8::String::new(scope, name).unwrap();
      yuzuko_core::define_element(
        scope,
        array.into(),
        index as u32,
        text.into(),
      );
    }
    resolve(scope, array.into())
  }

  #[promise]
  #[length(1)]
  #[rename("match")]
  fn match_(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Js {
    let _ = self;
    match storage_match(scope, args) {
      Ok(value) => resolve(scope, value),
      Err(error) => reject_op(scope, error),
    }
  }
}

#[op]
impl Cache {
  #[constructor]
  fn new() -> Result<Self, OpError> {
    Err(OpError::type_("Illegal constructor"))
  }

  #[promise]
  #[length(2)]
  fn put(
    &self,
    scope: &mut v8::PinScope,
    request: v8::Local<v8::Value>,
    response: v8::Local<v8::Value>,
  ) -> Js {
    match put_request(self, scope, request, response) {
      Ok(outcome) => outcome,
      Err(error) => reject_op(scope, error),
    }
  }

  #[promise]
  #[length(1)]
  #[rename("match")]
  fn match_(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Js {
    match self.query_one(scope, args) {
      Ok(value) => resolve(scope, value),
      Err(error) => reject_op(scope, error),
    }
  }

  #[promise]
  #[rename("matchAll")]
  fn match_all(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Js {
    match self.query_all(scope, args) {
      Ok(value) => resolve(scope, value),
      Err(error) => reject_op(scope, error),
    }
  }

  #[promise]
  fn add(&self, scope: &mut v8::PinScope, request: v8::Local<v8::Value>) -> Js {
    match self.add_requests(scope, std::slice::from_ref(&request)) {
      Ok(outcome) => outcome,
      Err(error) => reject_op(scope, error),
    }
  }

  #[promise]
  #[length(1)]
  #[rename("addAll")]
  fn add_all(
    &self,
    scope: &mut v8::PinScope,
    requests: v8::Local<v8::Value>,
  ) -> Js {
    if !requests.is_array() {
      return reject(scope, "Cache.addAll requires an array");
    }
    let Ok(array) = v8::Local::<v8::Array>::try_from(requests) else {
      return reject(scope, "Cache.addAll requires an array");
    };
    let mut values = Vec::with_capacity(array.length() as usize);
    for index in 0..array.length() {
      let Some(value) = array.get_index(scope, index) else {
        return reject(scope, "Cache.addAll requires an array");
      };
      values.push(value);
    }
    match self.add_requests(scope, &values) {
      Ok(outcome) => outcome,
      Err(error) => reject_op(scope, error),
    }
  }

  #[promise]
  #[length(1)]
  fn delete(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Js {
    match self.delete_matches(scope, args) {
      Ok(removed) => resolve(scope, v8::Boolean::new(scope, removed).into()),
      Err(error) => reject_op(scope, error),
    }
  }

  #[promise]
  fn keys(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Js {
    match self.request_keys(scope, args) {
      Ok(value) => resolve(scope, value),
      Err(error) => reject_op(scope, error),
    }
  }
}

impl Cache {
  fn require_live(&self) -> Result<(), OpError> {
    if cache_alive(&self.name, self.generation) {
      Ok(())
    } else {
      Err(deleted_error())
    }
  }

  fn query_one<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<v8::Local<'s, v8::Value>, OpError> {
    self.require_live()?;
    let (method, url, query) = read_request_query(scope, args, false)?;
    let found =
      query_cache(&self.name, Some(self.generation), &method, &url, &query);
    match found.into_iter().next() {
      Some(stored) => response_value(scope, &stored),
      None => Ok(v8::undefined(scope).into()),
    }
  }

  fn query_all<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<v8::Local<'s, v8::Value>, OpError> {
    self.require_live()?;
    let stored =
      listed_responses(scope, args, &self.name, Some(self.generation))?;
    let mut values = Vec::with_capacity(stored.len());
    for item in &stored {
      values.push(response_value(scope, item)?);
    }
    Ok(array_of(scope, values).into())
  }

  fn request_keys<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<v8::Local<'s, v8::Value>, OpError> {
    self.require_live()?;
    let urls = listed_urls(scope, args, &self.name, Some(self.generation))?;
    let array = v8::Array::new(scope, urls.len() as i32);
    for (index, url) in urls.iter().enumerate() {
      let request = new_get_request(scope, url).map_err(|_| {
        OpError::type_("The cached request could not be created")
      })?;
      yuzuko_core::define_element(
        scope,
        array.into(),
        index as u32,
        request.into(),
      );
    }
    Ok(array.into())
  }

  fn delete_matches(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<bool, OpError> {
    self.require_live()?;
    let (method, url, query) = read_request_query(scope, args, false)?;
    let keys =
      matched_keys(&self.name, Some(self.generation), &method, &url, &query);
    Ok(delete_keys(&self.name, self.generation, &keys))
  }

  fn add_requests(
    &self,
    scope: &mut v8::PinScope,
    values: &[v8::Local<v8::Value>],
  ) -> Result<Js, OpError> {
    self.require_live()?;
    let mut queued = Vec::with_capacity(values.len());
    for value in values {
      queued.push(validate_add_request(scope, *value)?);
    }
    if queued.is_empty() {
      return Ok(resolve(scope, v8::undefined(scope).into()));
    }
    let (resolver, promise) = promise_pair(scope);
    let storage_generation = current_storage_generation();
    let id = alloc_job();
    JOBS.with(|slot| {
      slot.borrow_mut().insert(
        id,
        Job::Add(AddJob {
          storage_generation,
          name: self.name.clone(),
          cache_generation: self.generation,
          current_url: String::new(),
          rest: queued,
          done: Vec::new(),
          waiting: None,
          resolver,
        }),
      );
    });
    pump_add(scope, id);
    Ok(promise)
  }
}

fn put_request(
  cache: &Cache,
  scope: &mut v8::PinScope,
  request: v8::Local<v8::Value>,
  response: v8::Local<v8::Value>,
) -> Result<Js, OpError> {
  if response.is_null_or_undefined() {
    return Err(OpError::type_("Cache.put requires a Response"));
  }
  cache.require_live()?;
  let request = coerce_request(scope, request)?;
  let (method, url) =
    crate::http::request::request_method_url(scope, request.into())?;
  if method != "GET" {
    return Err(OpError::type_("Cache.put only stores GET requests"));
  }
  if !http_url(&url) {
    return Err(OpError::type_("Cache.put requires an HTTP URL"));
  }
  let snapshot = crate::http::fetch::snapshot_response(
    scope,
    response,
    "Cache.put requires a Response",
  )?;
  cache.require_live()?;
  let key = cache_key(&url);
  let reserved = reserve_seq(&cache.name, cache.generation, &key)?;
  let stored = stored_from(&snapshot);
  match snapshot.body {
    CachedBody::Null => {
      write_entry(
        &cache.name,
        cache.generation,
        &key,
        reserved,
        stored_with_body(stored, CacheBody::Null),
      )?;
      Ok(resolve(scope, v8::undefined(scope).into()))
    }
    CachedBody::Bytes(bytes) => {
      write_entry(
        &cache.name,
        cache.generation,
        &key,
        reserved,
        stored_with_body(stored, CacheBody::Bytes(bytes)),
      )?;
      Ok(resolve(scope, v8::undefined(scope).into()))
    }
    CachedBody::Pending(body) => {
      let (resolver, promise) = promise_pair(scope);
      let id = alloc_job();
      JOBS.with(|slot| {
        slot.borrow_mut().insert(
          id,
          Job::Put(PutJob {
            storage_generation: reserved.storage_generation,
            name: cache.name.clone(),
            cache_generation: cache.generation,
            url: key,
            seq: reserved.seq,
            status: stored.status,
            status_text: stored.status_text,
            response_url: stored.response_url,
            redirected: stored.redirected,
            kind: stored.kind,
            headers: stored.headers,
            resolver,
          }),
        );
      });
      let local = v8::Local::new(scope, &body);
      watch(scope, local, id, put_body_ok, put_body_err);
      Ok(promise)
    }
  }
}

struct Reserved {
  storage_generation: u64,
  seq: u64,
}

fn stored_from(snapshot: &CachedResponse) -> StoredResponse {
  StoredResponse {
    status: snapshot.status,
    status_text: snapshot.status_text.clone(),
    response_url: snapshot.url.clone(),
    redirected: snapshot.redirected,
    kind: snapshot.kind.clone(),
    headers: snapshot.headers.clone(),
    body: CacheBody::Null,
  }
}

fn stored_with_body(
  mut stored: StoredResponse,
  body: CacheBody,
) -> StoredResponse {
  stored.body = body;
  stored
}

fn reserve_seq(
  name: &str,
  generation: u64,
  url: &str,
) -> Result<Reserved, OpError> {
  STORAGE.with(|slot| {
    let mut storage = slot.borrow_mut();
    let storage_generation = storage.generation;
    let Some(cache) = storage.caches.get_mut(name) else {
      return Err(deleted_error());
    };
    if cache.generation != generation {
      return Err(deleted_error());
    }
    cache.next_seq = cache.next_seq.saturating_add(1);
    let seq = cache.next_seq;
    cache.seqs.insert(url.to_string(), seq);
    Ok(Reserved {
      storage_generation,
      seq,
    })
  })
}

fn write_entry(
  name: &str,
  generation: u64,
  url: &str,
  reserved: Reserved,
  stored: StoredResponse,
) -> Result<(), OpError> {
  STORAGE.with(|slot| {
    let mut storage = slot.borrow_mut();
    if storage.generation != reserved.storage_generation {
      return Ok(());
    }
    let Some(cache) = storage.caches.get_mut(name) else {
      return Err(deleted_error());
    };
    if cache.generation != generation {
      return Err(deleted_error());
    }
    if cache.seqs.get(url).copied() != Some(reserved.seq) {
      return Ok(());
    }
    if !cache.urls.iter().any(|item| item == url) {
      cache.urls.push(url.to_string());
    }
    cache.entries.insert(url.to_string(), stored);
    Ok(())
  })
}

fn put_body_ok(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some(Job::Put(job)) = take_job(job_id(scope, args.data())) else {
    return;
  };
  if !same_storage(job.storage_generation) {
    return;
  }
  let bytes = match crate::bytes::buffer_source(scope, args.get(0)) {
    Ok(bytes) => bytes.unwrap_or_default(),
    Err(error) => {
      reject_resolver(scope, job.resolver, error);
      return;
    }
  };
  finish_put(scope, job, CacheBody::Bytes(bytes));
}

fn put_body_err(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some(Job::Put(job)) = take_job(job_id(scope, args.data())) else {
    return;
  };
  if !same_storage(job.storage_generation) {
    return;
  }
  if !cache_alive(&job.name, job.cache_generation) {
    reject_resolver(scope, job.resolver, deleted_error());
    return;
  }
  let resolver = v8::Local::new(scope, &job.resolver);
  resolver.reject(scope, args.get(0));
}

fn finish_put(scope: &mut v8::PinScope, job: PutJob, body: CacheBody) {
  if !same_storage(job.storage_generation) {
    return;
  }
  if !cache_alive(&job.name, job.cache_generation) {
    reject_resolver(scope, job.resolver, deleted_error());
    return;
  }
  let current = STORAGE.with(|slot| {
    slot
      .borrow()
      .caches
      .get(&job.name)
      .and_then(|cache| cache.seqs.get(&job.url).copied())
  });
  if current == Some(job.seq) {
    let stored = StoredResponse {
      status: job.status,
      status_text: job.status_text,
      response_url: job.response_url,
      redirected: job.redirected,
      kind: job.kind,
      headers: job.headers,
      body,
    };
    let _ =
      write_known(&job.name, job.cache_generation, &job.url, job.seq, stored);
  }
  resolve_resolver(scope, job.resolver);
}

fn write_known(
  name: &str,
  generation: u64,
  url: &str,
  seq: u64,
  stored: StoredResponse,
) -> Result<(), OpError> {
  STORAGE.with(|slot| {
    let mut storage = slot.borrow_mut();
    let Some(cache) = storage.caches.get_mut(name) else {
      return Err(deleted_error());
    };
    if cache.generation != generation {
      return Err(deleted_error());
    }
    if cache.seqs.get(url).copied() != Some(seq) {
      return Ok(());
    }
    if !cache.urls.iter().any(|item| item == url) {
      cache.urls.push(url.to_string());
    }
    cache.entries.insert(url.to_string(), stored);
    Ok(())
  })
}

fn validate_add_request(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<QueuedRequest, OpError> {
  let request = coerce_request(scope, value)?;
  let (method, url) =
    crate::http::request::request_method_url(scope, request.into())?;
  if method != "GET" {
    return Err(OpError::type_("Cache.put only stores GET requests"));
  }
  if !http_url(&url) {
    return Err(OpError::type_("Cache.put requires an HTTP URL"));
  }
  Ok(QueuedRequest {
    object: v8::Global::new(scope, request),
    url: cache_key(&url),
  })
}

fn pump_add(scope: &mut v8::PinScope, id: u64) {
  let Some(Job::Add(mut job)) = take_job(id) else {
    return;
  };
  if !same_storage(job.storage_generation) {
    return;
  }
  if !cache_alive(&job.name, job.cache_generation) {
    reject_resolver(scope, job.resolver, deleted_error());
    return;
  }
  if job.rest.is_empty() {
    commit_add(&job);
    resolve_resolver(scope, job.resolver);
    return;
  }
  let request = job.rest.remove(0);
  job.current_url = request.url;
  let fetch_target = request.object;
  JOBS.with(|slot| {
    slot.borrow_mut().insert(id, Job::Add(job));
  });
  let local = v8::Local::new(scope, &fetch_target);
  match call_fetch(scope, local.into()) {
    Ok(promise) => watch(scope, promise, id, add_fulfilled, add_rejected),
    Err(error) => fail_add(scope, id, error),
  }
}

fn add_fulfilled(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let id = job_id(scope, args.data());
  let value = args.get(0);
  let Some(header) = peek_add(id) else {
    return;
  };
  if !same_storage(header.storage_generation) {
    let _ = take_job(id);
    return;
  }
  if !cache_alive(&header.name, header.cache_generation) {
    fail_add(scope, id, deleted_error());
    return;
  }
  if !crate::http::fetch::is_response(scope, value) {
    fail_add(scope, id, OpError::type_("Cache.add requires a Response"));
    return;
  }
  let status = match crate::http::fetch::response_status(
    scope,
    value,
    "Cache.add requires a Response",
  ) {
    Ok(status) => status,
    Err(error) => {
      fail_add(scope, id, error);
      return;
    }
  };
  if !(200..300).contains(&status) {
    fail_add(scope, id, OpError::type_("The response is not ok"));
    return;
  }
  let snapshot = match crate::http::fetch::snapshot_response(
    scope,
    value,
    "Cache.add requires a Response",
  ) {
    Ok(snapshot) => snapshot,
    Err(error) => {
      fail_add(scope, id, error);
      return;
    }
  };
  let Some(Job::Add(mut job)) = take_job(id) else {
    return;
  };
  if !same_storage(job.storage_generation) {
    return;
  }
  if !cache_alive(&job.name, job.cache_generation) {
    reject_resolver(scope, job.resolver, deleted_error());
    return;
  }
  let url = job.current_url.clone();
  let pending = matches!(snapshot.body, CachedBody::Pending(_));
  if pending {
    let CachedBody::Pending(promise) = snapshot.body else {
      return;
    };
    job.waiting = Some(PendingRecord {
      url,
      status: snapshot.status,
      status_text: snapshot.status_text,
      response_url: snapshot.url,
      redirected: snapshot.redirected,
      kind: snapshot.kind,
      headers: snapshot.headers,
    });
    JOBS.with(|slot| {
      slot.borrow_mut().insert(id, Job::Add(job));
    });
    let local = v8::Local::new(scope, &promise);
    watch(scope, local, id, add_body_ok, add_body_err);
    return;
  }
  let body = match snapshot.body {
    CachedBody::Bytes(bytes) => CacheBody::Bytes(bytes),
    CachedBody::Null => CacheBody::Null,
    CachedBody::Pending(_) => return,
  };
  job.done.push(ReadyRecord {
    url,
    stored: StoredResponse {
      status: snapshot.status,
      status_text: snapshot.status_text,
      response_url: snapshot.url,
      redirected: snapshot.redirected,
      kind: snapshot.kind,
      headers: snapshot.headers,
      body,
    },
  });
  JOBS.with(|slot| {
    slot.borrow_mut().insert(id, Job::Add(job));
  });
  pump_add(scope, id);
}

fn add_rejected(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let id = job_id(scope, args.data());
  let Some(Job::Add(job)) = take_job(id) else {
    return;
  };
  if !same_storage(job.storage_generation) {
    return;
  }
  if !cache_alive(&job.name, job.cache_generation) {
    reject_resolver(scope, job.resolver, deleted_error());
    return;
  }
  let resolver = v8::Local::new(scope, &job.resolver);
  resolver.reject(scope, args.get(0));
}

fn add_body_ok(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let id = job_id(scope, args.data());
  let Some(Job::Add(mut job)) = take_job(id) else {
    return;
  };
  if !same_storage(job.storage_generation) {
    return;
  }
  if !cache_alive(&job.name, job.cache_generation) {
    reject_resolver(scope, job.resolver, deleted_error());
    return;
  }
  let Some(pending) = job.waiting.take() else {
    return;
  };
  let bytes = match crate::bytes::buffer_source(scope, args.get(0)) {
    Ok(bytes) => bytes.unwrap_or_default(),
    Err(error) => {
      reject_resolver(scope, job.resolver, error);
      return;
    }
  };
  job.done.push(ReadyRecord {
    url: pending.url,
    stored: StoredResponse {
      status: pending.status,
      status_text: pending.status_text,
      response_url: pending.response_url,
      redirected: pending.redirected,
      kind: pending.kind,
      headers: pending.headers,
      body: CacheBody::Bytes(bytes),
    },
  });
  JOBS.with(|slot| {
    slot.borrow_mut().insert(id, Job::Add(job));
  });
  pump_add(scope, id);
}

fn add_body_err(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  add_rejected(scope, args, _ret);
}

fn fail_add(scope: &mut v8::PinScope, id: u64, error: OpError) {
  let Some(Job::Add(job)) = take_job(id) else {
    return;
  };
  if !same_storage(job.storage_generation) {
    return;
  }
  reject_resolver(scope, job.resolver, error);
}

struct AddHeader {
  storage_generation: u64,
  name: String,
  cache_generation: u64,
}

fn peek_add(id: u64) -> Option<AddHeader> {
  JOBS.with(|slot| {
    let jobs = slot.borrow();
    let Job::Add(job) = jobs.get(&id)? else {
      return None;
    };
    Some(AddHeader {
      storage_generation: job.storage_generation,
      name: job.name.clone(),
      cache_generation: job.cache_generation,
    })
  })
}

fn commit_add(job: &AddJob) {
  STORAGE.with(|slot| {
    let mut storage = slot.borrow_mut();
    if storage.generation != job.storage_generation {
      return;
    }
    let Some(cache) = storage.caches.get_mut(&job.name) else {
      return;
    };
    if cache.generation != job.cache_generation {
      return;
    }
    for record in &job.done {
      cache.next_seq = cache.next_seq.saturating_add(1);
      let seq = cache.next_seq;
      cache.seqs.insert(record.url.clone(), seq);
      if !cache.urls.iter().any(|item| item == &record.url) {
        cache.urls.push(record.url.clone());
      }
      cache
        .entries
        .insert(record.url.clone(), record.stored.clone());
    }
  });
}

fn storage_match<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  args: &v8::FunctionCallbackArguments,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let (method, url, query) = read_request_query(scope, args, true)?;
  let names = if let Some(name) = query.cache_name.clone() {
    let present = STORAGE.with(|slot| slot.borrow().caches.contains_key(&name));
    if !present {
      return Ok(v8::undefined(scope).into());
    }
    vec![name]
  } else {
    STORAGE.with(|slot| slot.borrow().names.clone())
  };
  for name in names {
    let found = query_cache(&name, None, &method, &url, &query);
    if let Some(stored) = found.into_iter().next() {
      return response_value(scope, &stored);
    }
  }
  Ok(v8::undefined(scope).into())
}

fn listed_responses(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  name: &str,
  generation: Option<u64>,
) -> Result<Vec<StoredResponse>, OpError> {
  if args.length() == 0 || args.get(0).is_undefined() {
    return Ok(all_entries(name, generation));
  }
  let (method, url, query) = read_request_query(scope, args, false)?;
  Ok(query_cache(name, generation, &method, &url, &query))
}

fn listed_urls(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  name: &str,
  generation: Option<u64>,
) -> Result<Vec<String>, OpError> {
  if args.length() == 0 || args.get(0).is_undefined() {
    return Ok(all_keys(name, generation));
  }
  let (method, url, query) = read_request_query(scope, args, false)?;
  Ok(matched_keys(name, generation, &method, &url, &query))
}

fn all_entries(name: &str, generation: Option<u64>) -> Vec<StoredResponse> {
  STORAGE.with(|slot| {
    let storage = slot.borrow();
    let Some(cache) = storage.caches.get(name) else {
      return Vec::new();
    };
    if generation.is_some_and(|generation| cache.generation != generation) {
      return Vec::new();
    }
    cache
      .urls
      .iter()
      .filter_map(|url| cache.entries.get(url).cloned())
      .collect()
  })
}

fn all_keys(name: &str, generation: Option<u64>) -> Vec<String> {
  STORAGE.with(|slot| {
    let storage = slot.borrow();
    let Some(cache) = storage.caches.get(name) else {
      return Vec::new();
    };
    if generation.is_some_and(|generation| cache.generation != generation) {
      return Vec::new();
    }
    cache.urls.clone()
  })
}

fn query_cache(
  name: &str,
  generation: Option<u64>,
  method: &str,
  url: &str,
  query: &Query,
) -> Vec<StoredResponse> {
  STORAGE.with(|slot| {
    let storage = slot.borrow();
    let Some(cache) = storage.caches.get(name) else {
      return Vec::new();
    };
    if generation.is_some_and(|generation| cache.generation != generation) {
      return Vec::new();
    }
    cache
      .urls
      .iter()
      .filter(|key| request_matches(key, method, url, query))
      .filter_map(|key| cache.entries.get(key).cloned())
      .collect()
  })
}

fn matched_keys(
  name: &str,
  generation: Option<u64>,
  method: &str,
  url: &str,
  query: &Query,
) -> Vec<String> {
  STORAGE.with(|slot| {
    let storage = slot.borrow();
    let Some(cache) = storage.caches.get(name) else {
      return Vec::new();
    };
    if generation.is_some_and(|generation| cache.generation != generation) {
      return Vec::new();
    }
    cache
      .urls
      .iter()
      .filter(|key| request_matches(key, method, url, query))
      .cloned()
      .collect()
  })
}

fn delete_keys(name: &str, generation: u64, keys: &[String]) -> bool {
  STORAGE.with(|slot| {
    let mut storage = slot.borrow_mut();
    let Some(cache) = storage.caches.get_mut(name) else {
      return false;
    };
    if cache.generation != generation {
      return false;
    }
    let mut removed = false;
    for url in keys {
      if cache.entries.remove(url).is_some() {
        removed = true;
        cache.seqs.remove(url);
        cache.urls.retain(|item| item != url);
      }
    }
    removed
  })
}

fn request_matches(
  stored: &str,
  method: &str,
  url: &str,
  query: &Query,
) -> bool {
  if !query.ignore_method && method != "GET" {
    return false;
  }
  if query.ignore_search {
    without_search(stored) == without_search(&cache_key(url))
  } else {
    stored == cache_key(url)
  }
}

fn read_request_query(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  cache_name: bool,
) -> Result<(String, String, Query), OpError> {
  let request = coerce_request(scope, args.get(0))?;
  let (method, url) =
    crate::http::request::request_method_url(scope, request.into())?;
  let options = if args.length() > 1 {
    args.get(1)
  } else {
    v8::undefined(scope).into()
  };
  let query = read_query(scope, options, cache_name)?;
  Ok((method, url, query))
}

fn read_query(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  cache_name: bool,
) -> Result<Query, OpError> {
  if value.is_undefined() {
    return Ok(Query::fresh());
  }
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("Cache options are not an object"))?;
  let ignore_search = dict_bool(scope, object, "ignoreSearch")?;
  let ignore_method = dict_bool(scope, object, "ignoreMethod")?;
  let _ignore_vary = dict_bool(scope, object, "ignoreVary")?;
  let cache_name = if cache_name {
    dict_string(scope, object, "cacheName")?
  } else {
    None
  };
  Ok(Query {
    ignore_search,
    ignore_method,
    cache_name,
  })
}

fn dict_bool(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<bool, OpError> {
  match dict_get(scope, object, name)? {
    Some(value) => Ok(value.boolean_value(scope)),
    None => Ok(false),
  }
}

fn dict_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<String>, OpError> {
  let Some(value) = dict_get(scope, object, name)? else {
    return Ok(None);
  };
  let text = {
    v8::tc_scope!(let tc, scope);
    match value.to_string(tc) {
      Some(text) => Ok(text.to_rust_string_lossy(tc)),
      None => Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown))),
    }
  };
  match text {
    Ok(text) => Ok(Some(text)),
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::type_("The property access failed")),
  }
}

fn dict_get<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, OpError> {
  let found = {
    v8::tc_scope!(let tc, scope);
    let key = v8::String::new(tc, name).unwrap();
    match object.get(tc, key.into()) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown))),
    }
  };
  match found {
    Ok(value) => {
      let value = v8::Local::new(scope, &value);
      if value.is_undefined() {
        Ok(None)
      } else {
        Ok(Some(value))
      }
    }
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::type_("The property access failed")),
  }
}

fn coerce_request<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  if crate::http::request::is_request(scope, value) {
    let object = v8::Local::<v8::Object>::try_from(value)
      .map_err(|_| OpError::type_("Cache.put requires a Request"))?;
    return Ok(yuzuko_core::op::root(scope, object));
  }
  let ctor = yuzuko_core::intrinsics::intrinsic_function(scope, "Request")
    .ok_or_else(|| OpError::type_("Cache.put requires a Request"))?;
  let created = {
    v8::tc_scope!(let tc, scope);
    match ctor.new_instance(tc, &[value]) {
      Some(object) => Ok(v8::Global::new(tc, object)),
      None => Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown))),
    }
  };
  match created {
    Ok(object) => Ok(v8::Local::new(scope, &object)),
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::type_("Cache.put requires a Request")),
  }
}

fn call_fetch<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  request: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Promise>, OpError> {
  let function = yuzuko_core::intrinsics::intrinsic_function(scope, "fetch")
    .ok_or_else(|| OpError::type_("fetch is not a function"))?;
  let called = {
    v8::tc_scope!(let tc, scope);
    let receiver = tc.get_current_context().global(tc);
    match function.call(tc, receiver.into(), &[request]) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(tc.exception().map(|thrown| v8::Global::new(tc, thrown))),
    }
  };
  let value = match called {
    Ok(value) => v8::Local::new(scope, &value),
    Err(Some(thrown)) => return Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => return Err(OpError::type_("fetch failed")),
  };
  v8::Local::<v8::Promise>::try_from(value)
    .map_err(|_| OpError::type_("fetch did not return a promise"))
}

fn response_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  stored: &StoredResponse,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let body = match &stored.body {
    CacheBody::Null => crate::http::fetch::CachedBody::Null,
    CacheBody::Bytes(bytes) => {
      crate::http::fetch::CachedBody::Bytes(bytes.clone())
    }
  };
  let object = crate::http::fetch::response_from_cache(
    scope,
    crate::http::fetch::CachedResponse {
      status: stored.status,
      status_text: stored.status_text.clone(),
      url: stored.response_url.clone(),
      redirected: stored.redirected,
      kind: stored.kind.clone(),
      headers: stored.headers.clone(),
      body,
    },
  )?;
  Ok(object.into())
}

fn cache_alive(name: &str, generation: u64) -> bool {
  STORAGE.with(|slot| {
    slot
      .borrow()
      .caches
      .get(name)
      .is_some_and(|cache| cache.generation == generation)
  })
}

fn current_storage_generation() -> u64 {
  STORAGE.with(|slot| slot.borrow().generation)
}

fn same_storage(generation: u64) -> bool {
  current_storage_generation() == generation
}

fn deleted_error() -> OpError {
  OpError::dom("InvalidStateError", "The cache is deleted")
}

fn cache_key(url: &str) -> String {
  let Ok(mut parsed) = url::Url::parse(url) else {
    return url.to_string();
  };
  parsed.set_fragment(None);
  parsed.to_string()
}

fn without_search(url: &str) -> String {
  let Ok(mut parsed) = url::Url::parse(url) else {
    return url.to_string();
  };
  parsed.set_fragment(None);
  parsed.set_query(None);
  parsed.to_string()
}

fn http_url(url: &str) -> bool {
  url::Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

fn alloc_job() -> u64 {
  NEXT_JOB.with(|slot| {
    let id = slot.get();
    slot.set(id.saturating_add(1));
    id
  })
}

fn job_id(scope: &mut v8::PinScope, data: v8::Local<v8::Value>) -> u64 {
  data.number_value(scope).unwrap_or(0.0) as u64
}

fn take_job(id: u64) -> Option<Job> {
  JOBS.with(|slot| slot.borrow_mut().remove(&id))
}

fn watch(
  scope: &mut v8::PinScope,
  promise: v8::Local<v8::Promise>,
  id: u64,
  on_ok: impl v8::MapFnTo<v8::FunctionCallback>,
  on_err: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let data: v8::Local<v8::Value> = v8::Number::new(scope, id as f64).into();
  let Some(fulfilled) = v8::Function::builder(on_ok).data(data).build(scope)
  else {
    return;
  };
  let Some(rejected) = v8::Function::builder(on_err).data(data).build(scope)
  else {
    return;
  };
  let _ = promise.then2(scope, fulfilled, rejected);
}

fn promise_pair(
  scope: &mut v8::PinScope,
) -> (v8::Global<v8::PromiseResolver>, Js) {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  let value: v8::Local<v8::Value> = promise.into();
  (
    v8::Global::new(scope, resolver),
    Js(v8::Global::new(scope, value)),
  )
}

fn resolve_resolver(
  scope: &mut v8::PinScope,
  resolver: v8::Global<v8::PromiseResolver>,
) {
  let resolver = v8::Local::new(scope, &resolver);
  resolver.resolve(scope, v8::undefined(scope).into());
}

fn reject_resolver(
  scope: &mut v8::PinScope,
  resolver: v8::Global<v8::PromiseResolver>,
  error: OpError,
) {
  let captured = {
    v8::tc_scope!(let tc, scope);
    error.throw(tc);
    tc.exception().map(|value| v8::Global::new(tc, value))
  };
  let resolver = v8::Local::new(scope, &resolver);
  if let Some(error) = captured {
    let error = v8::Local::new(scope, &error);
    resolver.reject(scope, error);
  }
}

fn array_of<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  values: Vec<v8::Local<v8::Value>>,
) -> v8::Local<'s, v8::Array> {
  let array = v8::Array::new(scope, values.len() as i32);
  for (index, value) in values.iter().enumerate() {
    yuzuko_core::define_element(scope, array.into(), index as u32, *value);
  }
  array
}

pub fn install(scope: &mut v8::PinScope) {
  Cache::install(scope);
  CacheStorage::install(scope);
  let caches = unsafe { yuzuko_core::op::wrap_instance(scope, CacheStorage) };
  let getter = v8::Function::builder(crate::util::global_attribute)
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
  let url = v8::String::new(scope, url).ok_or(())?;
  ctor.new_instance(scope, &[url.into()]).ok_or(())
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

fn reject_op(scope: &mut v8::PinScope, error: OpError) -> Js {
  let captured = {
    v8::tc_scope!(let tc, scope);
    error.throw(tc);
    tc.exception().map(|value| v8::Global::new(tc, value))
  };
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  if let Some(error) = captured {
    let error = v8::Local::new(scope, &error);
    resolver.reject(scope, error);
  } else {
    let message = v8::String::new(scope, "Cache failed").unwrap();
    let error = v8::Exception::error(scope, message);
    resolver.reject(scope, error);
  }
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn realm_cleanup_releases_pending_jobs() {
    // Initialize the process-wide V8 platform before creating a test isolate.
    yuzuko_core::run_scripts(&[]).unwrap();
    let mut isolate = v8::Isolate::new(Default::default());
    v8::scope!(let scope, &mut isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let (resolver, _) = promise_pair(scope);
    JOBS.with(|slot| {
      slot.borrow_mut().insert(
        1,
        Job::Add(AddJob {
          storage_generation: 0,
          name: String::new(),
          cache_generation: 0,
          current_url: String::new(),
          rest: Vec::new(),
          done: Vec::new(),
          waiting: None,
          resolver,
        }),
      );
    });
    clear();
    JOBS.with(|slot| assert!(slot.borrow().is_empty()));
  }
}
