//! `EventSource` for server-sent events.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use yuzuko_core::op::OpError;
use yuzuko_macros::op;
use yuzuko_winter::{BodyNote, SseItem, SseParser, perform};

use super::common::{self, define_constant, dispatch_simple};
use crate::message::trusted_message;
use crate::target::{self, EventTarget};
use crate::util::{self, key};

const CONNECTING: i32 = 0;
const OPEN: i32 = 1;
const CLOSED: i32 = 2;

static NEXT: AtomicU64 = AtomicU64::new(1);

struct SourceQueue {
  notes: Mutex<Vec<Tagged>>,
  wake: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl SourceQueue {
  fn new() -> Self {
    Self {
      notes: Mutex::new(Vec::new()),
      wake: Mutex::new(None),
    }
  }

  fn push(&self, note: Tagged) {
    self.notes.lock().expect("event source").push(note);
    let wake = self.wake.lock().expect("event source wake").clone();
    if let Some(wake) = wake {
      wake();
    }
  }

  fn listen(&self, wake: Arc<dyn Fn() + Send + Sync>) {
    *self.wake.lock().expect("event source wake") = Some(Arc::clone(&wake));
    if !self.notes.lock().expect("event source").is_empty() {
      wake();
    }
  }

  fn take(&self) -> Vec<Tagged> {
    std::mem::take(&mut *self.notes.lock().expect("event source"))
  }
}

struct Tagged {
  generation: u64,
  note: SourceNote,
}

struct Live {
  object: v8::Global<v8::Object>,
  notes: Arc<SourceQueue>,
}

thread_local! {
  static LIVE: RefCell<Vec<(u64, Live)>> = const { RefCell::new(Vec::new()) };
}

enum SourceNote {
  Open {
    origin: String,
  },
  Event {
    name: String,
    data: String,
    id: String,
  },
  Retry(u64),
  /// The stream ended after it opened. Reconnect.
  Down,
  /// The status or MIME type failed. Stay closed.
  Fatal,
}

#[repr(C)]
pub struct EventSource {
  base: EventTarget,
  id: u64,
  url: String,
  credentials: bool,
  ready: Cell<i32>,
  origin: RefCell<String>,
  last_id: RefCell<String>,
  retry_ms: Cell<u64>,
  generation: Cell<u64>,
  closed: Arc<AtomicBool>,
}

unsafe impl v8::cppgc::GarbageCollected for EventSource {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"EventSource"
  }
}

#[op(
  inherit = EventTarget,
  inspect = [
    "readyState",
    "url",
    "withCredentials",
    "onopen",
    "onmessage",
    "onerror",
  ]
)]
impl EventSource {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    if args.length() < 1 {
      return Err(OpError::type_("EventSource requires a URL"));
    }
    let input = yuzuko_core::rust_string(scope, args.get(0));
    let url = resolve_http(&input)?;
    yuzuko_core::authorize_url(&url)?;
    let credentials = with_credentials(scope, args)?;
    Ok(Self {
      base: EventTarget::new(),
      id: NEXT.fetch_add(1, Ordering::Relaxed),
      url: url.to_string(),
      credentials,
      ready: Cell::new(CONNECTING),
      origin: RefCell::new(String::new()),
      last_id: RefCell::new(String::new()),
      retry_ms: Cell::new(3000),
      generation: Cell::new(1),
      closed: Arc::new(AtomicBool::new(false)),
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.id, object);
    begin(
      self.id,
      self.url.clone(),
      String::new(),
      self.closed.clone(),
      self.generation.get(),
    );
    arm(self.id);
  }

  #[getter]
  fn url(&self) -> String {
    self.url.clone()
  }

  #[getter]
  fn with_credentials(&self) -> bool {
    self.credentials
  }

  #[getter]
  fn ready_state(&self) -> i32 {
    self.ready.get()
  }

  fn close(&self) {
    self.ready.set(CLOSED);
    self.generation.set(self.generation.get().saturating_add(1));
    self.closed.store(true, Ordering::Relaxed);
    if let Some(notes) = live_notes(self.id) {
      notes.push(Tagged {
        generation: self.generation.get(),
        note: SourceNote::Down,
      });
    }
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  EventSource::install(scope);
  let constructor = EventSource::template(scope)
    .get_function(scope)
    .expect("EventSource");
  crate::target::define_prototype_event_handlers(
    scope,
    constructor,
    &["open", "message", "error"],
  );
  let constructor = v8::Local::<v8::Object>::try_from(
    v8::Local::<v8::Value>::from(constructor),
  )
  .expect("EventSource constructor");
  let prototype = constructor
    .get(scope, key(scope, "prototype").into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    .expect("EventSource prototype");
  for (name, value) in [
    ("CONNECTING", CONNECTING),
    ("OPEN", OPEN),
    ("CLOSED", CLOSED),
  ] {
    define_constant(scope, constructor, name, value);
    define_constant(scope, prototype, name, value);
  }
}

pub(crate) fn clear() {
  LIVE.with(|slot| slot.borrow_mut().clear());
}

/// The host is checked again on every connection, including a reconnect.
fn stream_permitted(url: &str) -> bool {
  let Ok(url) = url::Url::parse(url) else {
    return false;
  };
  yuzuko_core::authorize_url(&url).is_ok()
}

fn begin(
  id: u64,
  url: String,
  last_id: String,
  closed: Arc<AtomicBool>,
  generation: u64,
) {
  let notes = notes_for(id);
  let done = Arc::clone(&closed);
  let queued = Arc::clone(&notes);
  let started = std::thread::Builder::new().spawn(move || {
    run_stream(url, last_id, done, generation, queued);
  });
  // Without a thread the connection cannot be established. Fail it.
  if started.is_err() {
    notes.push(Tagged {
      generation,
      note: SourceNote::Fatal,
    });
  }
}

fn run_stream(
  url: String,
  last_id: String,
  closed: Arc<AtomicBool>,
  generation: u64,
  notes: Arc<SourceQueue>,
) {
  if closed.load(Ordering::Relaxed) {
    return;
  }
  if !stream_permitted(&url) {
    notes.push(Tagged {
      generation,
      note: SourceNote::Down,
    });
    return;
  }
  let mut headers = vec![
    ("Accept".to_string(), "text/event-stream".to_string()),
    ("Cache-Control".to_string(), "no-cache".to_string()),
  ];
  if !last_id.is_empty() {
    headers.push(("Last-Event-ID".to_string(), last_id.clone()));
  }
  let response = perform(
    "GET".to_string(),
    url.clone(),
    headers,
    Vec::new(),
    "follow".to_string(),
    Arc::clone(&closed),
  );
  let Ok(response) = response else {
    notes.push(Tagged {
      generation,
      note: SourceNote::Down,
    });
    return;
  };
  if response.status != 200 || !is_event_stream(&response.headers) {
    notes.push(Tagged {
      generation,
      note: SourceNote::Fatal,
    });
    return;
  }
  let origin = origin_of(&response.url);
  notes.push(Tagged {
    generation,
    note: SourceNote::Open { origin },
  });
  let mut parser = SseParser::default();
  parser.set_last_id(last_id);
  loop {
    if closed.load(Ordering::Relaxed) {
      response.body.stop();
      return;
    }
    match response.body.wait_note(Duration::from_millis(200)) {
      Some(BodyNote::Chunk(bytes)) => match parser.push(&bytes) {
        Ok(items) => emit_items(&notes, generation, items),
        Err(yuzuko_winter::SseError) => {
          notes.push(Tagged {
            generation,
            note: SourceNote::Down,
          });
          return;
        }
      },
      Some(BodyNote::End) => {
        if let Ok(items) = parser.finish() {
          emit_items(&notes, generation, items);
        }
        notes.push(Tagged {
          generation,
          note: SourceNote::Down,
        });
        return;
      }
      Some(BodyNote::Failed(_)) => {
        notes.push(Tagged {
          generation,
          note: SourceNote::Down,
        });
        return;
      }
      None => {}
    }
  }
}

fn emit_items(notes: &SourceQueue, generation: u64, items: Vec<SseItem>) {
  for item in items {
    let note = match item {
      SseItem::Retry(ms) => SourceNote::Retry(ms),
      SseItem::Event(event) => SourceNote::Event {
        name: event.name,
        data: event.data,
        id: event.id,
      },
    };
    notes.push(Tagged { generation, note });
  }
}

fn is_event_stream(headers: &[(String, String)]) -> bool {
  headers.iter().any(|(name, value)| {
    if !name.eq_ignore_ascii_case("content-type") {
      return false;
    }
    value
      .split(';')
      .next()
      .unwrap_or("")
      .trim()
      .eq_ignore_ascii_case("text/event-stream")
  })
}

fn origin_of(url: &str) -> String {
  url::Url::parse(url)
    .map(|url| url.origin().ascii_serialization())
    .unwrap_or_else(|_| "null".to_string())
}

fn resolve_http(input: &str) -> Result<url::Url, OpError> {
  let url = if let Ok(url) = url::Url::parse(input) {
    url
  } else if let Some(base) = yuzuko_core::entry_url() {
    let base =
      url::Url::parse(&base).map_err(|_| syntax("The URL is invalid"))?;
    base.join(input).map_err(|_| syntax("The URL is invalid"))?
  } else {
    return Err(syntax("The URL is invalid"));
  };
  if url.scheme() != "http" && url.scheme() != "https" {
    return Err(syntax("The URL scheme is not http or https"));
  }
  Ok(url)
}

fn syntax(message: &str) -> OpError {
  OpError::dom("SyntaxError", message)
}

fn with_credentials(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<bool, OpError> {
  if args.length() < 2 || args.get(1).is_null() || args.get(1).is_undefined() {
    return Ok(false);
  }
  if !args.get(1).is_object() {
    return Err(OpError::type_("EventSource options must be a dictionary"));
  }
  Ok(util::dict_bool(scope, Some(args.get(1)), "withCredentials"))
}

fn remember(scope: &mut v8::PinScope, id: u64, object: v8::Local<v8::Object>) {
  LIVE.with(|slot| {
    slot.borrow_mut().push((
      id,
      Live {
        object: v8::Global::new(scope, object),
        notes: Arc::new(SourceQueue::new()),
      },
    ));
  });
}

fn notes_for(id: u64) -> Arc<SourceQueue> {
  LIVE.with(|slot| {
    slot
      .borrow()
      .iter()
      .find(|(stored, _)| *stored == id)
      .map(|(_, live)| Arc::clone(&live.notes))
      .expect("event source")
  })
}

fn arm(id: u64) {
  let Some(notes) = live_notes(id) else {
    return;
  };
  notes.listen(common::one_shot_wake("event source wake", move |scope| {
    pump(scope, id)
  }));
}

fn live_notes(id: u64) -> Option<Arc<SourceQueue>> {
  LIVE.with(|slot| {
    slot
      .borrow()
      .iter()
      .find(|(stored, _)| *stored == id)
      .map(|(_, live)| Arc::clone(&live.notes))
  })
}

fn pump(scope: &mut v8::PinScope, id: u64) {
  let Some(object) = live_object(scope, id) else {
    return;
  };
  let Some(notes) = live_notes(id) else {
    return;
  };
  let notes = notes.take();
  let Some(source) =
    (unsafe { yuzuko_core::op::host_ref::<EventSource>(scope, object) })
  else {
    return;
  };
  let source = source.get();
  for tagged in notes {
    if tagged.generation != source.generation.get() {
      continue;
    }
    if source.ready.get() == CLOSED {
      break;
    }
    let note = tagged.note;
    match note {
      SourceNote::Open { origin } => {
        *source.origin.borrow_mut() = origin;
        source.ready.set(OPEN);
        dispatch_simple(scope, object, "open");
      }
      SourceNote::Retry(ms) => source.retry_ms.set(ms),
      SourceNote::Event {
        name,
        data,
        id: event_id,
      } => {
        if !event_id.contains('\0') {
          *source.last_id.borrow_mut() = event_id.clone();
        }
        let text = v8::String::new(scope, &data).unwrap();
        let message = trusted_message(
          scope,
          &name,
          text.into(),
          source.origin.borrow().clone(),
          event_id,
        );
        let _ = target::dispatch(scope, object, message);
      }
      SourceNote::Down => {
        if source.ready.get() == CLOSED {
          break;
        }
        source.ready.set(CONNECTING);
        dispatch_simple(scope, object, "error");
        schedule_reconnect(id, source.retry_ms.get());
      }
      SourceNote::Fatal => {
        if source.ready.get() == CLOSED {
          break;
        }
        source.ready.set(CLOSED);
        source.closed.store(true, Ordering::Relaxed);
        dispatch_simple(scope, object, "error");
      }
    }
  }
  if source.ready.get() != CLOSED {
    arm(id);
  }
}

fn schedule_reconnect(id: u64, delay: u64) {
  let delay = Duration::from_millis(delay.max(1));
  yuzuko_core::schedule_host(delay, move |scope| {
    let Some(object) = live_object(scope, id) else {
      return;
    };
    let Some(source) =
      (unsafe { yuzuko_core::op::host_ref::<EventSource>(scope, object) })
    else {
      return;
    };
    let source = source.get();
    if source.ready.get() == CLOSED || source.closed.load(Ordering::Relaxed) {
      return;
    }
    let generation = source.generation.get().saturating_add(1);
    source.generation.set(generation);
    begin(
      id,
      source.url.clone(),
      source.last_id.borrow().clone(),
      Arc::clone(&source.closed),
      generation,
    );
  });
}

fn live_object<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  id: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  LIVE.with(|slot| {
    slot
      .borrow()
      .iter()
      .find(|(stored, _)| *stored == id)
      .map(|(_, live)| v8::Local::new(scope, &live.object))
  })
}
