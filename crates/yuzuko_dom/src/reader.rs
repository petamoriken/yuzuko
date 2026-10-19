//! `FileReader`. Reads are macrotasks so `abort` can run after `loadstart`.

use std::cell::{Cell, RefCell};

use yuzuko_core::op::{Js, OpError, host_ref, traced, traced_local};
use yuzuko_macros::op;

use crate::target::EventTarget;

const EMPTY: i32 = 0;
const LOADING: i32 = 1;
const DONE: i32 = 2;

const STEP_START: i32 = 1;
const STEP_PROGRESS: i32 = 2;
const STEP_LOAD: i32 = 3;
const STEP_END: i32 = 4;

enum Kind {
  Text,
  Binary,
  DataUrl,
  Buffer,
}

struct Pending {
  bytes: Vec<u8>,
  mime: String,
  encoding: Option<String>,
  kind: Kind,
}

#[repr(C)]
pub struct FileReader {
  base: EventTarget,
  state: Cell<i32>,
  generation: Cell<u32>,
  result: RefCell<Option<v8::TracedReference<v8::Value>>>,
  error: RefCell<Option<v8::TracedReference<v8::Value>>>,
  on_load_start: RefCell<Option<v8::TracedReference<v8::Value>>>,
  on_progress: RefCell<Option<v8::TracedReference<v8::Value>>>,
  on_load: RefCell<Option<v8::TracedReference<v8::Value>>>,
  on_abort: RefCell<Option<v8::TracedReference<v8::Value>>>,
  on_error: RefCell<Option<v8::TracedReference<v8::Value>>>,
  on_load_end: RefCell<Option<v8::TracedReference<v8::Value>>>,
  pending: RefCell<Option<Pending>>,
}

unsafe impl v8::cppgc::GarbageCollected for FileReader {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    trace_slot(visitor, &self.result);
    trace_slot(visitor, &self.error);
    trace_slot(visitor, &self.on_load_start);
    trace_slot(visitor, &self.on_progress);
    trace_slot(visitor, &self.on_load);
    trace_slot(visitor, &self.on_abort);
    trace_slot(visitor, &self.on_error);
    trace_slot(visitor, &self.on_load_end);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"FileReader"
  }
}

fn trace_slot(
  visitor: &mut v8::cppgc::Visitor,
  slot: &RefCell<Option<v8::TracedReference<v8::Value>>>,
) {
  if let Ok(slot) = slot.try_borrow() {
    visitor.trace(&*slot);
  }
}

#[op(inherit = EventTarget)]
impl FileReader {
  #[constructor]
  fn new() -> FileReader {
    FileReader {
      base: EventTarget::new(),
      state: Cell::new(EMPTY),
      generation: Cell::new(0),
      result: RefCell::new(None),
      error: RefCell::new(None),
      on_load_start: RefCell::new(None),
      on_progress: RefCell::new(None),
      on_load: RefCell::new(None),
      on_abort: RefCell::new(None),
      on_error: RefCell::new(None),
      on_load_end: RefCell::new(None),
      pending: RefCell::new(None),
    }
  }

  #[getter]
  fn ready_state(&self) -> i32 {
    self.state.get()
  }

  #[getter]
  fn result(&self, scope: &mut v8::PinScope) -> Js {
    stored(scope, &self.result)
  }

  #[getter]
  fn error(&self, scope: &mut v8::PinScope) -> Js {
    stored(scope, &self.error)
  }

  #[getter]
  #[rename("onloadstart")]
  fn on_load_start(&self, scope: &mut v8::PinScope) -> Js {
    stored(scope, &self.on_load_start)
  }

  #[setter]
  #[rename("onloadstart")]
  fn on_load_start(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) {
    assign_handler(&self.base, &self.on_load_start, scope, "loadstart", value);
  }

  #[getter]
  #[rename("onprogress")]
  fn on_progress(&self, scope: &mut v8::PinScope) -> Js {
    stored(scope, &self.on_progress)
  }

  #[setter]
  #[rename("onprogress")]
  fn on_progress(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    assign_handler(&self.base, &self.on_progress, scope, "progress", value);
  }

  #[getter]
  #[rename("onload")]
  fn on_load(&self, scope: &mut v8::PinScope) -> Js {
    stored(scope, &self.on_load)
  }

  #[setter]
  #[rename("onload")]
  fn on_load(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    assign_handler(&self.base, &self.on_load, scope, "load", value);
  }

  #[getter]
  #[rename("onabort")]
  fn on_abort(&self, scope: &mut v8::PinScope) -> Js {
    stored(scope, &self.on_abort)
  }

  #[setter]
  #[rename("onabort")]
  fn on_abort(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    assign_handler(&self.base, &self.on_abort, scope, "abort", value);
  }

  #[getter]
  #[rename("onerror")]
  fn on_error(&self, scope: &mut v8::PinScope) -> Js {
    stored(scope, &self.on_error)
  }

  #[setter]
  #[rename("onerror")]
  fn on_error(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    assign_handler(&self.base, &self.on_error, scope, "error", value);
  }

  #[getter]
  #[rename("onloadend")]
  fn on_load_end(&self, scope: &mut v8::PinScope) -> Js {
    stored(scope, &self.on_load_end)
  }

  #[setter]
  #[rename("onloadend")]
  fn on_load_end(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    assign_handler(&self.base, &self.on_load_end, scope, "loadend", value);
  }

  #[rename("readAsArrayBuffer")]
  fn read_as_array_buffer(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    blob: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.begin(this, scope, blob, Kind::Buffer, None)
  }

  #[rename("readAsBinaryString")]
  fn read_as_binary_string(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    blob: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.begin(this, scope, blob, Kind::Binary, None)
  }

  #[rename("readAsDataURL")]
  fn read_as_data_url(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    blob: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    self.begin(this, scope, blob, Kind::DataUrl, None)
  }

  #[rename("readAsText")]
  fn read_as_text(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    blob: v8::Local<v8::Value>,
    encoding: Option<String>,
  ) -> Result<(), OpError> {
    self.begin(this, scope, blob, Kind::Text, encoding)
  }

  fn abort(
    &self,
    #[this] this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
  ) {
    if self.state.get() != LOADING {
      *self.result.borrow_mut() = None;
      return;
    }
    self.generation.set(self.generation.get().wrapping_add(1));
    self.state.set(DONE);
    *self.result.borrow_mut() = None;
    *self.pending.borrow_mut() = None;
    if let Some(error) = yuzuko_core::op::new_dom_exception(
      scope,
      "AbortError",
      "The operation was aborted.",
    ) {
      let error: v8::Local<v8::Value> = error.into();
      *self.error.borrow_mut() = Some(traced(scope, error));
    }
    let generation = self.generation.get();
    crate::dispatch_trusted_event(scope, this, "abort");
    // A new read started from the abort listener owns the following loadend.
    if self.generation.get() == generation && self.state.get() == DONE {
      crate::dispatch_trusted_event(scope, this, "loadend");
    }
  }
}

impl FileReader {
  fn begin(
    &self,
    this: v8::Local<v8::Object>,
    scope: &mut v8::PinScope,
    blob: v8::Local<v8::Value>,
    kind: Kind,
    encoding: Option<String>,
  ) -> Result<(), OpError> {
    if self.state.get() == LOADING {
      return Err(OpError::dom(
        "InvalidStateError",
        "FileReader is already loading.",
      ));
    }
    let (bytes, mime) = yuzuko_winter::snapshot_blob(scope, blob)?;
    let generation = self.generation.get().wrapping_add(1);
    self.generation.set(generation);
    self.state.set(LOADING);
    *self.result.borrow_mut() = None;
    *self.error.borrow_mut() = None;
    let empty = bytes.is_empty();
    *self.pending.borrow_mut() = Some(Pending {
      bytes,
      mime,
      encoding,
      kind,
    });
    queue_step(scope, this, STEP_START, generation);
    if !empty {
      queue_step(scope, this, STEP_PROGRESS, generation);
    }
    queue_step(scope, this, STEP_LOAD, generation);
    queue_step(scope, this, STEP_END, generation);
    Ok(())
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  FileReader::install(scope);
  let constructor = FileReader::template(scope)
    .get_function(scope)
    .expect("FileReader");
  let constructor = v8::Local::<v8::Object>::try_from(
    v8::Local::<v8::Value>::from(constructor),
  )
  .expect("FileReader constructor");
  let prototype = constructor
    .get(scope, v8::String::new(scope, "prototype").unwrap().into())
    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    .expect("FileReader prototype");
  for (name, value) in [("EMPTY", EMPTY), ("LOADING", LOADING), ("DONE", DONE)]
  {
    define_constant(scope, constructor, name, value);
    define_constant(scope, prototype, name, value);
  }
}

fn define_constant(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: i32,
) {
  let key = v8::String::new(scope, name).unwrap();
  let value = v8::Integer::new(scope, value);
  let _ = object.define_own_property(
    scope,
    key.into(),
    value.into(),
    v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE,
  );
}

fn stored(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Value>>>,
) -> Js {
  match slot
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference))
  {
    Some(value) => Js(v8::Global::new(scope, value)),
    None => Js::null(scope),
  }
}

fn assign_handler(
  target: &EventTarget,
  slot: &RefCell<Option<v8::TracedReference<v8::Value>>>,
  scope: &mut v8::PinScope,
  type_name: &str,
  value: v8::Local<v8::Value>,
) {
  let previous = slot
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference));
  if let Some(previous) = previous {
    target.remove_callback(scope, type_name, previous, false);
  }
  if value.is_function() {
    target.add_callback(scope, type_name, value, false, false, false);
    *slot.borrow_mut() = Some(traced(scope, value));
  } else {
    *slot.borrow_mut() = None;
  }
}

fn queue_step(
  scope: &mut v8::PinScope,
  reader: v8::Local<v8::Object>,
  step: i32,
  generation: u32,
) {
  let function = v8::Function::new(scope, reader_step).unwrap();
  let step = v8::Integer::new(scope, step);
  let generation = v8::Integer::new(scope, generation as i32);
  yuzuko_core::schedule_callback(
    scope,
    function,
    0.0,
    &[reader.into(), step.into(), generation.into()],
  );
}

fn reader_step(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Ok(object) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
    return;
  };
  let Some(reader) = (unsafe { host_ref::<FileReader>(scope, object) }) else {
    return;
  };
  let step = args.get(1).int32_value(scope).unwrap_or(0);
  let generation = args.get(2).uint32_value(scope).unwrap_or(0);
  let reader = reader.get();
  if reader.generation.get() != generation {
    return;
  }
  let loading = reader.state.get() == LOADING;
  match step {
    STEP_START if loading => {
      crate::dispatch_trusted_event(scope, object, "loadstart");
    }
    STEP_PROGRESS if loading => {
      crate::dispatch_trusted_event(scope, object, "progress");
    }
    STEP_LOAD if loading => finish(scope, object, reader),
    STEP_END if reader.state.get() == DONE => {
      crate::dispatch_trusted_event(scope, object, "loadend");
    }
    _ => {}
  }
}

fn finish(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  reader: &FileReader,
) {
  let pending = reader.pending.borrow_mut().take();
  let Some(pending) = pending else {
    return;
  };
  let value = result_value(scope, &pending);
  *reader.result.borrow_mut() = Some(traced(scope, value));
  reader.state.set(DONE);
  crate::dispatch_trusted_event(scope, object, "load");
}

fn result_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  pending: &Pending,
) -> v8::Local<'s, v8::Value> {
  match pending.kind {
    Kind::Text => {
      let text = yuzuko_winter::decode_blob_text(
        &pending.bytes,
        &pending.mime,
        pending.encoding.as_deref(),
      );
      v8::String::new(scope, &text)
        .unwrap_or_else(|| v8::String::empty(scope))
        .into()
    }
    Kind::Binary => {
      let text = yuzuko_winter::binary_string(&pending.bytes);
      v8::String::new(scope, &text)
        .unwrap_or_else(|| v8::String::empty(scope))
        .into()
    }
    Kind::DataUrl => {
      let text = yuzuko_winter::data_url(&pending.bytes, &pending.mime);
      v8::String::new(scope, &text)
        .unwrap_or_else(|| v8::String::empty(scope))
        .into()
    }
    Kind::Buffer => {
      let store =
        v8::ArrayBuffer::new_backing_store_from_vec(pending.bytes.clone())
          .make_shared();
      v8::ArrayBuffer::with_backing_store(scope, &store).into()
    }
  }
}
