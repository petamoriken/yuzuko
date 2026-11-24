//! `performance` is an `EventTarget`. User timing records marks and measures.
//!
//! `queue a PerformanceEntry` already appends to the timeline buffer. `mark`
//! and `measure` call it once. An empty observer buffer does not stop later
//! observers in the same task.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::time::Duration;

use yuzuko_core::op::{Js, OpError, host_ref, traced, traced_local};
use yuzuko_macros::op;

use crate::report::report_error;
use crate::target::EventTarget;
use crate::util::{self, key};

macro_rules! pending_exception {
  ($scope:expr) => {{
    let exception = $scope
      .exception()
      .unwrap_or_else(|| v8::undefined($scope).into());
    v8::Global::new($scope, exception)
  }};
}

const MODE_UNDEFINED: u8 = 0;
const MODE_SINGLE: u8 = 1;
const MODE_MULTIPLE: u8 = 2;

#[repr(C)]
pub struct Performance {
  base: EventTarget,
}

unsafe impl v8::cppgc::GarbageCollected for Performance {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"Performance"
  }
}

#[op(inherit = EventTarget, inspect = ["timeOrigin"])]
impl Performance {
  #[getter]
  fn time_origin(&self) -> f64 {
    yuzuko_core::time_origin_ms()
  }

  fn now(&self) -> f64 {
    yuzuko_core::performance_now_ms()
  }

  #[rename("toJSON")]
  fn to_json(&self, scope: &mut v8::PinScope) -> Js {
    let object = v8::Object::new(scope);
    let origin = v8::Number::new(scope, yuzuko_core::time_origin_ms());
    let _ = object.create_data_property(
      scope,
      key(scope, "timeOrigin").into(),
      origin.into(),
    );
    js_value(scope, object.into())
  }

  #[length(1)]
  fn mark(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Js, OpError> {
    let _ = self;
    let mark = PerformanceMark::build(scope, args)?;
    let name = mark.base.name.clone();
    let start_time = mark.base.start_time;
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, mark) };
    queue_entry(scope, object, &name, "mark", start_time);
    Ok(js_value(scope, object.into()))
  }

  fn clear_marks(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    let _ = self;
    clear_type(scope, args, "mark")
  }

  #[length(1)]
  fn measure(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Js, OpError> {
    let _ = self;
    let measure = PerformanceMeasure::build(scope, args)?;
    let name = measure.base.name.clone();
    let start_time = measure.base.start_time;
    let object = unsafe { yuzuko_core::op::wrap_instance(scope, measure) };
    queue_entry(scope, object, &name, "measure", start_time);
    Ok(js_value(scope, object.into()))
  }

  fn clear_measures(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    let _ = self;
    clear_type(scope, args, "measure")
  }

  fn get_entries(&self, scope: &mut v8::PinScope) -> Js {
    let _ = self;
    timeline_entries(scope, None, None)
  }

  #[length(1)]
  fn get_entries_by_type(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Js, OpError> {
    let _ = self;
    if args.length() < 1 {
      return Err(OpError::type_("The entry type is missing."));
    }
    let entry_type = util::try_string(scope, args.get(0))?;
    Ok(timeline_entries(scope, Some(entry_type.as_str()), None))
  }

  #[length(1)]
  fn get_entries_by_name(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Js, OpError> {
    let _ = self;
    if args.length() < 1 {
      return Err(OpError::type_("The entry name is missing."));
    }
    let name = util::try_string(scope, args.get(0))?;
    let entry_type = optional_string(scope, args, 1)?;
    Ok(timeline_entries(
      scope,
      entry_type.as_deref(),
      Some(name.as_str()),
    ))
  }
}

#[repr(C)]
struct PerformanceEntry {
  id: u64,
  navigation_id: u64,
  name: String,
  entry_type: String,
  start_time: f64,
  duration: f64,
}

unsafe impl v8::cppgc::GarbageCollected for PerformanceEntry {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"PerformanceEntry"
  }
}

#[op(inspect = [
  "name",
  "entryType",
  "startTime",
  "duration",
  "id",
  "navigationId"
])]
impl PerformanceEntry {
  #[getter]
  fn name(&self) -> String {
    self.name.clone()
  }

  #[getter]
  fn entry_type(&self) -> String {
    self.entry_type.clone()
  }

  #[getter]
  fn start_time(&self) -> f64 {
    self.start_time
  }

  #[getter]
  fn duration(&self) -> f64 {
    self.duration
  }

  #[getter]
  fn id(&self) -> f64 {
    self.id as f64
  }

  #[getter]
  fn navigation_id(&self) -> f64 {
    self.navigation_id as f64
  }

  #[rename("toJSON")]
  fn to_json(&self, scope: &mut v8::PinScope) -> Js {
    entry_json(scope, self, None)
  }
}

#[repr(C)]
struct PerformanceMark {
  base: PerformanceEntry,
  detail: RefCell<Option<v8::TracedReference<v8::Value>>>,
}

unsafe impl v8::cppgc::GarbageCollected for PerformanceMark {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(detail) = self.detail.try_borrow() {
      visitor.trace(&*detail);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"PerformanceMark"
  }
}

#[op(inherit = PerformanceEntry, inspect_after = ["detail"])]
impl PerformanceMark {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    // The constructor does not record the entry. `performance.mark` does.
    Self::build(scope, args)
  }

  #[getter]
  fn detail(&self, scope: &mut v8::PinScope) -> Js {
    detail_value(scope, &self.detail)
  }

  #[rename("toJSON")]
  fn to_json(&self, scope: &mut v8::PinScope) -> Js {
    let detail = detail_local(scope, &self.detail);
    entry_json(scope, &self.base, Some(detail))
  }
}

#[repr(C)]
struct PerformanceMeasure {
  base: PerformanceEntry,
  detail: RefCell<Option<v8::TracedReference<v8::Value>>>,
}

unsafe impl v8::cppgc::GarbageCollected for PerformanceMeasure {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
    if let Ok(detail) = self.detail.try_borrow() {
      visitor.trace(&*detail);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"PerformanceMeasure"
  }
}

#[op(inherit = PerformanceEntry, inspect_after = ["detail"])]
impl PerformanceMeasure {
  #[getter]
  fn detail(&self, scope: &mut v8::PinScope) -> Js {
    detail_value(scope, &self.detail)
  }

  #[rename("toJSON")]
  fn to_json(&self, scope: &mut v8::PinScope) -> Js {
    let detail = detail_local(scope, &self.detail);
    entry_json(scope, &self.base, Some(detail))
  }
}

struct Observed {
  object: v8::TracedReference<v8::Object>,
  name: String,
  entry_type: String,
  start_time: f64,
}

struct PerformanceObserverEntryList {
  entries: Vec<Observed>,
}

unsafe impl v8::cppgc::GarbageCollected for PerformanceObserverEntryList {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    for entry in &self.entries {
      visitor.trace(&entry.object);
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"PerformanceObserverEntryList"
  }
}

#[op]
impl PerformanceObserverEntryList {
  fn get_entries(&self, scope: &mut v8::PinScope) -> Js {
    list_entries(scope, &self.entries, None, None)
  }

  #[length(1)]
  fn get_entries_by_type(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Js, OpError> {
    if args.length() < 1 {
      return Err(OpError::type_("The entry type is missing."));
    }
    let entry_type = util::try_string(scope, args.get(0))?;
    Ok(list_entries(
      scope,
      &self.entries,
      Some(entry_type.as_str()),
      None,
    ))
  }

  #[length(1)]
  fn get_entries_by_name(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Js, OpError> {
    if args.length() < 1 {
      return Err(OpError::type_("The entry name is missing."));
    }
    let name = util::try_string(scope, args.get(0))?;
    let entry_type = optional_string(scope, args, 1)?;
    Ok(list_entries(
      scope,
      &self.entries,
      entry_type.as_deref(),
      Some(name.as_str()),
    ))
  }
}

struct SingleOption {
  entry_type: String,
  buffered: bool,
}

enum ObserverOptions {
  Empty,
  Single(Vec<SingleOption>),
  Multiple(Vec<String>),
}

struct PerformanceObserver {
  callback: RefCell<Option<v8::TracedReference<v8::Function>>>,
  mode: Cell<u8>,
  options: RefCell<ObserverOptions>,
  buffer: RefCell<Vec<Observed>>,
  dropped: Cell<bool>,
}

unsafe impl v8::cppgc::GarbageCollected for PerformanceObserver {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(callback) = self.callback.try_borrow() {
      visitor.trace(&*callback);
    }
    if let Ok(buffer) = self.buffer.try_borrow() {
      for entry in buffer.iter() {
        visitor.trace(&entry.object);
      }
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"PerformanceObserver"
  }
}

#[op]
impl PerformanceObserver {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    if args.length() < 1 || !args.get(0).is_function() {
      return Err(OpError::type_("The callback is not a function."));
    }
    let function = v8::Local::<v8::Function>::try_from(args.get(0))
      .map_err(|_| OpError::type_("The callback is not a function."))?;
    Ok(Self {
      callback: RefCell::new(Some(traced(scope, function))),
      mode: Cell::new(MODE_UNDEFINED),
      options: RefCell::new(ObserverOptions::Empty),
      buffer: RefCell::new(Vec::new()),
      dropped: Cell::new(false),
    })
  }

  fn observe(
    &self,
    scope: &mut v8::PinScope,
    #[this] this: v8::Local<v8::Object>,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    let options = observe_options(scope, args)?;
    let entry_types = match &options.entry_types {
      Some(value) => Some(entry_type_sequence(scope, *value)?),
      None => None,
    };
    let type_name = match &options.type_name {
      Some(value) => Some(util::try_string(scope, *value)?),
      None => None,
    };
    let buffered = options.buffered;
    if entry_types.is_none() && type_name.is_none() {
      return Err(OpError::type_(
        "The observe options need type or entryTypes.",
      ));
    }
    if entry_types.is_some()
      && (type_name.is_some() || options.buffered_present)
    {
      return Err(OpError::type_(
        "The observe options cannot set entryTypes with another option.",
      ));
    }
    let mode = self.mode.get();
    if mode == MODE_UNDEFINED {
      self.mode.set(if entry_types.is_some() {
        MODE_MULTIPLE
      } else {
        MODE_SINGLE
      });
    } else if (mode == MODE_SINGLE && entry_types.is_some())
      || (mode == MODE_MULTIPLE && type_name.is_some())
    {
      return Err(OpError::dom(
        "InvalidModificationError",
        "PerformanceObserver cannot change its observe mode.",
      ));
    }
    self.dropped.set(true);
    if let Some(entry_types) = entry_types {
      let supported = entry_types
        .into_iter()
        .filter(|entry_type| is_supported(entry_type))
        .collect::<Vec<_>>();
      if supported.is_empty() {
        return Ok(());
      }
      *self.options.borrow_mut() = ObserverOptions::Multiple(supported);
      register_observer(scope, this);
      return Ok(());
    }
    let Some(entry_type) = type_name else {
      return Ok(());
    };
    if !is_supported(&entry_type) {
      return Ok(());
    }
    {
      let mut stored = self.options.borrow_mut();
      match &mut *stored {
        ObserverOptions::Single(list) => {
          if let Some(existing) =
            list.iter_mut().find(|item| item.entry_type == entry_type)
          {
            existing.buffered = buffered;
          } else {
            list.push(SingleOption {
              entry_type: entry_type.clone(),
              buffered,
            });
          }
        }
        _ => {
          *stored = ObserverOptions::Single(vec![SingleOption {
            entry_type: entry_type.clone(),
            buffered,
          }]);
        }
      }
    }
    register_observer(scope, this);
    if buffered {
      copy_buffered(scope, self, &entry_type);
      queue_observer_task();
    }
    Ok(())
  }

  fn disconnect(
    &self,
    scope: &mut v8::PinScope,
    #[this] this: v8::Local<v8::Object>,
  ) {
    TIMELINE.with(|slot| {
      slot.borrow_mut().observers.retain(|global| {
        !v8::Local::new(scope, global).strict_equals(this.into())
      });
    });
    self.buffer.borrow_mut().clear();
    *self.options.borrow_mut() = ObserverOptions::Empty;
  }

  fn take_records(&self, scope: &mut v8::PinScope) -> Js {
    let entries = std::mem::take(&mut *self.buffer.borrow_mut());
    observed_array(scope, &entries, false)
  }
}

impl PerformanceMark {
  fn build(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let name = required_string(scope, args, 0, "The mark name is missing.")?;
    let options = dictionary_arg(scope, args, 1)?;
    let start_time = mark_start_time(scope, options)?;
    let detail = match options {
      Some(options) => member_detail(scope, options)?,
      None => None,
    };
    Ok(Self {
      base: new_entry(name, "mark", start_time, 0.0),
      detail: RefCell::new(detail),
    })
  }
}

impl PerformanceMeasure {
  fn build(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let name = required_string(scope, args, 0, "The measure name is missing.")?;
    let input = measure_input(scope, args)?;
    let end_mark = given_string(scope, args, 2)?;
    let members = match &input {
      MeasureInput::Options(object) => {
        let object = v8::Local::new(scope, object);
        read_measure_members(scope, object)?
      }
      MeasureInput::Empty | MeasureInput::Name(_) => MeasureMembers::default(),
    };
    let is_options = !matches!(input, MeasureInput::Name(_));
    if is_options
      && members.any_present()
      && (end_mark.is_some()
        || members.start_and_end_missing()
        || members.all_three())
    {
      return Err(OpError::type_("The measure options are invalid."));
    }
    let end_time = if let Some(end_mark) = end_mark.as_deref() {
      name_to_timestamp(end_mark)?
    } else if is_options && let Some(end) = members.end.as_ref() {
      mark_ref_timestamp(end)?
    } else if is_options
      && let (Some(start), Some(duration)) =
        (members.start.as_ref(), members.duration)
    {
      let start = mark_ref_timestamp(start)?;
      let duration = timestamp_value(duration)?;
      start + duration
    } else {
      yuzuko_core::performance_now_ms()
    };
    let start_time = if is_options && let Some(start) = members.start.as_ref() {
      mark_ref_timestamp(start)?
    } else if is_options
      && let (Some(duration), Some(end)) =
        (members.duration, members.end.as_ref())
    {
      let duration = timestamp_value(duration)?;
      let end = mark_ref_timestamp(end)?;
      end - duration
    } else if let MeasureInput::Name(start) = &input {
      name_to_timestamp(start)?
    } else {
      0.0
    };
    let detail = if is_options {
      match members.detail {
        Some(value) => clone_detail(scope, v8::Local::new(scope, value))?,
        None => None,
      }
    } else {
      None
    };
    Ok(Self {
      base: new_entry(name, "measure", start_time, end_time - start_time),
      detail: RefCell::new(detail),
    })
  }
}

impl PerformanceObserver {
  fn wants(&self, entry_type: &str) -> bool {
    match &*self.options.borrow() {
      ObserverOptions::Empty => false,
      ObserverOptions::Multiple(types) => {
        types.iter().any(|item| item == entry_type)
      }
      ObserverOptions::Single(options) => {
        options.iter().any(|item| item.entry_type == entry_type)
      }
    }
  }
}

struct ObserveArgs<'s> {
  entry_types: Option<v8::Local<'s, v8::Value>>,
  type_name: Option<v8::Local<'s, v8::Value>>,
  buffered: bool,
  buffered_present: bool,
}

enum MeasureInput {
  Empty,
  Name(String),
  Options(v8::Global<v8::Object>),
}

enum MarkRef {
  Name(String),
  Time(f64),
}

#[derive(Default)]
struct MeasureMembers {
  start: Option<MarkRef>,
  end: Option<MarkRef>,
  duration: Option<f64>,
  detail: Option<v8::Global<v8::Value>>,
}

impl MeasureMembers {
  fn any_present(&self) -> bool {
    self.start.is_some()
      || self.end.is_some()
      || self.duration.is_some()
      || self.detail.is_some()
  }

  fn start_and_end_missing(&self) -> bool {
    self.start.is_none() && self.end.is_none()
  }

  fn all_three(&self) -> bool {
    self.start.is_some() && self.end.is_some() && self.duration.is_some()
  }
}

#[derive(Clone)]
struct StoredEntry {
  object: v8::Global<v8::Object>,
  name: String,
  entry_type: String,
  start_time: f64,
}

struct Timeline {
  buffers: Vec<(String, Vec<StoredEntry>)>,
  next_id: u64,
  observers: Vec<v8::Global<v8::Object>>,
  queued: bool,
  generation: u64,
  supported: Option<v8::Global<v8::Array>>,
}

impl Timeline {
  fn new() -> Self {
    Self {
      buffers: Vec::new(),
      next_id: 0,
      observers: Vec::new(),
      queued: false,
      generation: 0,
      supported: None,
    }
  }
}

thread_local! {
  static TIMELINE: RefCell<Timeline> = RefCell::new(Timeline::new());
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  PerformanceEntry::install(scope);
  PerformanceMark::install(scope);
  PerformanceMeasure::install(scope);
  PerformanceObserverEntryList::install(scope);
  PerformanceObserver::install(scope);
  install_supported_entry_types(scope);
  Performance::install(scope);
  let object = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      Performance {
        base: EventTarget::new(),
      },
    )
  };
  let global = scope.get_current_context().global(scope);
  global.set(scope, key(scope, "performance").into(), object.into());
}

pub(crate) fn clear() {
  TIMELINE.with(|slot| {
    let generation = slot.borrow().generation.wrapping_add(1);
    *slot.borrow_mut() = Timeline {
      generation,
      ..Timeline::new()
    };
  });
}

fn install_supported_entry_types(scope: &mut v8::PinScope) {
  let constructor = PerformanceObserver::template(scope)
    .get_function(scope)
    .expect("PerformanceObserver");
  let getter = v8::Function::builder(supported_entry_types)
    .length(0)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap();
  let getter_name = v8::String::new(scope, "get supportedEntryTypes").unwrap();
  getter.set_name(getter_name);
  let object: v8::Local<v8::Object> = constructor.into();
  let key = v8::String::new(scope, "supportedEntryTypes").unwrap();
  let get: v8::Local<v8::Value> = getter.into();
  let set: v8::Local<v8::Value> = v8::undefined(scope).into();
  let mut descriptor = v8::PropertyDescriptor::new_from_get_set(get, set);
  descriptor.set_enumerable(true);
  descriptor.set_configurable(true);
  let _ = object.define_property(scope, key.into(), &descriptor);
}

fn supported_entry_types(
  scope: &mut v8::PinScope,
  _args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let existing = TIMELINE.with(|slot| slot.borrow().supported.clone());
  if let Some(existing) = existing {
    let array = v8::Local::<v8::Array>::new(scope, existing);
    ret.set(array.into());
    return;
  }
  let array = v8::Array::new(scope, 2);
  let mark = v8::String::new(scope, "mark").unwrap();
  let measure = v8::String::new(scope, "measure").unwrap();
  array.set_index(scope, 0, mark.into());
  array.set_index(scope, 1, measure.into());
  let _ = array.set_integrity_level(scope, v8::IntegrityLevel::Frozen);
  TIMELINE.with(|slot| {
    slot.borrow_mut().supported = Some(v8::Global::new(scope, array));
  });
  ret.set(array.into());
}

fn new_entry(
  name: String,
  entry_type: &str,
  start_time: f64,
  duration: f64,
) -> PerformanceEntry {
  PerformanceEntry {
    id: next_entry_id(),
    navigation_id: 0,
    name,
    entry_type: entry_type.to_string(),
    start_time,
    duration,
  }
}

fn next_entry_id() -> u64 {
  TIMELINE.with(|slot| {
    let mut timeline = slot.borrow_mut();
    timeline.next_id += 1;
    timeline.next_id
  })
}

fn queue_entry(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  entry_type: &str,
  start_time: f64,
) {
  let observers = TIMELINE.with(|slot| slot.borrow().observers.clone());
  let mut interested = Vec::new();
  for global in &observers {
    let local = v8::Local::new(scope, global);
    let wants = unsafe { host_ref::<PerformanceObserver>(scope, local) }
      .is_some_and(|host| host.get().wants(entry_type));
    if wants {
      interested.push(global.clone());
    }
  }
  for global in interested {
    let local = v8::Local::new(scope, &global);
    let reference = traced(scope, object);
    let Some(host) = (unsafe { host_ref::<PerformanceObserver>(scope, local) })
    else {
      continue;
    };
    host.get().buffer.borrow_mut().push(Observed {
      object: reference,
      name: name.to_string(),
      entry_type: entry_type.to_string(),
      start_time,
    });
  }
  let stored = StoredEntry {
    object: v8::Global::new(scope, object),
    name: name.to_string(),
    entry_type: entry_type.to_string(),
    start_time,
  };
  TIMELINE.with(|slot| {
    let mut timeline = slot.borrow_mut();
    if let Some((_, entries)) = timeline
      .buffers
      .iter_mut()
      .find(|(kind, _)| kind == entry_type)
    {
      entries.push(stored);
    } else {
      timeline
        .buffers
        .push((entry_type.to_string(), vec![stored]));
    }
  });
  queue_observer_task();
}

fn queue_observer_task() {
  let generation = TIMELINE.with(|slot| {
    let mut timeline = slot.borrow_mut();
    if timeline.queued {
      return None;
    }
    timeline.queued = true;
    Some(timeline.generation)
  });
  let Some(generation) = generation else {
    return;
  };
  yuzuko_core::schedule_host(Duration::ZERO, move |scope| {
    deliver_observers(scope, generation);
  });
}

fn deliver_observers(scope: &mut v8::PinScope, generation: u64) {
  if TIMELINE.with(|slot| slot.borrow().generation) != generation {
    return;
  }
  TIMELINE.with(|slot| slot.borrow_mut().queued = false);
  let observers = TIMELINE.with(|slot| slot.borrow().observers.clone());
  for global in observers {
    if TIMELINE.with(|slot| slot.borrow().generation) != generation {
      return;
    }
    let object = v8::Local::new(scope, &global);
    deliver_one(scope, object);
  }
}

fn deliver_one(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
  let prepared = {
    let Some(host) =
      (unsafe { host_ref::<PerformanceObserver>(scope, object) })
    else {
      return;
    };
    let observer = host.get();
    if observer.buffer.borrow().is_empty() {
      return;
    }
    let entries = std::mem::take(&mut *observer.buffer.borrow_mut());
    let dropped = observer.dropped.get();
    if dropped {
      observer.dropped.set(false);
    }
    let function = observer
      .callback
      .borrow()
      .as_ref()
      .and_then(|reference| traced_local(scope, reference));
    (entries, dropped, function)
  };
  let (entries, dropped, function) = prepared;
  let Some(function) = function else {
    return;
  };
  let list = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      PerformanceObserverEntryList { entries },
    )
  };
  let options = v8::Object::new(scope);
  if dropped {
    let count = v8::Number::new(scope, 0.0);
    let _ = options.create_data_property(
      scope,
      key(scope, "droppedEntriesCount").into(),
      count.into(),
    );
  }
  let receiver: v8::Local<v8::Value> = object.into();
  let list_value: v8::Local<v8::Value> = list.into();
  let options_value: v8::Local<v8::Value> = options.into();
  let captured = {
    v8::tc_scope!(let tc, scope);
    match function.call(tc, receiver, &[list_value, receiver, options_value]) {
      Some(_) => Ok(()),
      None => Err(
        tc.exception()
          .map(|exception| v8::Global::new(tc, exception)),
      ),
    }
  };
  if let Err(Some(exception)) = captured {
    report_error(scope, v8::Local::new(scope, exception));
  }
}

fn register_observer(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
  TIMELINE.with(|slot| {
    let mut timeline = slot.borrow_mut();
    let present = timeline
      .observers
      .iter()
      .any(|global| v8::Local::new(scope, global).strict_equals(object.into()));
    if !present {
      timeline.observers.push(v8::Global::new(scope, object));
    }
  });
}

fn copy_buffered(
  scope: &mut v8::PinScope,
  observer: &PerformanceObserver,
  entry_type: &str,
) {
  let stored = TIMELINE.with(|slot| {
    slot
      .borrow()
      .buffers
      .iter()
      .find(|(kind, _)| kind == entry_type)
      .map(|(_, entries)| entries.clone())
      .unwrap_or_default()
  });
  let mut extra = Vec::with_capacity(stored.len());
  for entry in stored {
    let local = v8::Local::new(scope, &entry.object);
    extra.push(Observed {
      object: traced(scope, local),
      name: entry.name,
      entry_type: entry.entry_type,
      start_time: entry.start_time,
    });
  }
  observer.buffer.borrow_mut().extend(extra);
}

fn clear_type(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  entry_type: &str,
) -> Result<(), OpError> {
  let name = if args.length() < 1 || args.get(0).is_undefined() {
    None
  } else {
    Some(util::try_string(scope, args.get(0))?)
  };
  TIMELINE.with(|slot| {
    let mut timeline = slot.borrow_mut();
    let Some((_, entries)) = timeline
      .buffers
      .iter_mut()
      .find(|(kind, _)| kind == entry_type)
    else {
      return;
    };
    match name {
      Some(name) => entries.retain(|entry| entry.name != name),
      None => entries.clear(),
    }
  });
  Ok(())
}

fn timeline_entries(
  scope: &mut v8::PinScope,
  entry_type: Option<&str>,
  name: Option<&str>,
) -> Js {
  let mut entries = TIMELINE.with(|slot| {
    let timeline = slot.borrow();
    let mut entries = Vec::new();
    for (kind, stored) in &timeline.buffers {
      if entry_type.is_some_and(|expected| expected != kind) {
        continue;
      }
      for entry in stored {
        if name.is_some_and(|expected| expected != entry.name) {
          continue;
        }
        entries.push(entry.clone());
      }
    }
    entries
  });
  sort_by_start(&mut entries);
  let objects = entries
    .iter()
    .map(|entry| v8::Local::new(scope, &entry.object))
    .collect::<Vec<_>>();
  object_array(scope, &objects)
}

fn list_entries(
  scope: &mut v8::PinScope,
  entries: &[Observed],
  entry_type: Option<&str>,
  name: Option<&str>,
) -> Js {
  observed_filtered(scope, entries, true, entry_type, name)
}

fn observed_array(
  scope: &mut v8::PinScope,
  entries: &[Observed],
  sort: bool,
) -> Js {
  observed_filtered(scope, entries, sort, None, None)
}

fn observed_filtered(
  scope: &mut v8::PinScope,
  entries: &[Observed],
  sort: bool,
  entry_type: Option<&str>,
  name: Option<&str>,
) -> Js {
  let mut listed = Vec::new();
  for entry in entries {
    if entry_type.is_some_and(|expected| expected != entry.entry_type) {
      continue;
    }
    if name.is_some_and(|expected| expected != entry.name) {
      continue;
    }
    let Some(local) = traced_local(scope, &entry.object) else {
      continue;
    };
    listed.push((v8::Global::new(scope, local), entry.start_time));
  }
  if sort {
    listed.sort_by(|left, right| compare_time(left.1, right.1));
  }
  let objects = listed
    .iter()
    .map(|(object, _)| v8::Local::new(scope, object))
    .collect::<Vec<_>>();
  object_array(scope, &objects)
}

fn sort_by_start(entries: &mut [StoredEntry]) {
  entries
    .sort_by(|left, right| compare_time(left.start_time, right.start_time));
}

fn compare_time(left: f64, right: f64) -> Ordering {
  left.partial_cmp(&right).unwrap_or(Ordering::Equal)
}

fn latest_mark_start(name: &str) -> Option<f64> {
  TIMELINE.with(|slot| {
    let timeline = slot.borrow();
    let (_, entries) =
      timeline.buffers.iter().find(|(kind, _)| kind == "mark")?;
    entries
      .iter()
      .rev()
      .find(|entry| entry.name == name)
      .map(|entry| entry.start_time)
  })
}

fn is_timing_name(name: &str) -> bool {
  matches!(
    name,
    "navigationStart"
      | "unloadEventStart"
      | "unloadEventEnd"
      | "redirectStart"
      | "redirectEnd"
      | "fetchStart"
      | "domainLookupStart"
      | "domainLookupEnd"
      | "connectStart"
      | "connectEnd"
      | "secureConnectionStart"
      | "requestStart"
      | "responseStart"
      | "responseEnd"
      | "domLoading"
      | "domInteractive"
      | "domContentLoadedEventStart"
      | "domContentLoadedEventEnd"
      | "domComplete"
      | "loadEventStart"
      | "loadEventEnd"
  )
}

fn is_supported(entry_type: &str) -> bool {
  entry_type == "mark" || entry_type == "measure"
}

fn name_to_timestamp(name: &str) -> Result<f64, OpError> {
  if is_timing_name(name) {
    return Err(OpError::type_(
      "The mark name is a PerformanceTiming attribute.",
    ));
  }
  latest_mark_start(name).ok_or_else(|| {
    OpError::dom(
      "SyntaxError",
      format!("The mark \"{name}\" does not exist."),
    )
  })
}

fn mark_ref_timestamp(mark: &MarkRef) -> Result<f64, OpError> {
  match mark {
    MarkRef::Time(time) => timestamp_value(*time),
    MarkRef::Name(name) => name_to_timestamp(name),
  }
}

fn timestamp_value(time: f64) -> Result<f64, OpError> {
  if time < 0.0 {
    Err(OpError::type_("The timestamp is negative."))
  } else {
    Ok(time)
  }
}

fn mark_start_time(
  scope: &mut v8::PinScope,
  options: Option<v8::Local<v8::Object>>,
) -> Result<f64, OpError> {
  let Some(options) = options else {
    return Ok(yuzuko_core::performance_now_ms());
  };
  let Some(value) = object_member(scope, options, "startTime")? else {
    return Ok(yuzuko_core::performance_now_ms());
  };
  timestamp_value(try_number(scope, value)?)
}

fn member_detail(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Object>,
) -> Result<Option<v8::TracedReference<v8::Value>>, OpError> {
  let Some(value) = object_member(scope, options, "detail")? else {
    return Ok(None);
  };
  clone_detail(scope, value)
}

fn clone_detail(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Option<v8::TracedReference<v8::Value>>, OpError> {
  if value.is_null() {
    return Ok(None);
  }
  match yuzuko_core::clone_value(scope, value) {
    Ok(cloned) => Ok(Some(traced(scope, cloned))),
    Err(Some(thrown)) => Err(OpError::Thrown(Box::new(thrown))),
    Err(None) => Err(OpError::dom(
      "DataCloneError",
      "The detail could not be cloned.",
    )),
  }
}

fn measure_input(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<MeasureInput, OpError> {
  if args.length() < 2 {
    return Ok(MeasureInput::Empty);
  }
  let value = args.get(1);
  if value.is_null() || value.is_undefined() {
    return Ok(MeasureInput::Empty);
  }
  if value.is_object() {
    let object = v8::Local::<v8::Object>::try_from(value)
      .map_err(|_| OpError::type_("The measure options are not an object."))?;
    return Ok(MeasureInput::Options(v8::Global::new(scope, object)));
  }
  Ok(MeasureInput::Name(util::try_string(scope, value)?))
}

fn read_measure_members(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> Result<MeasureMembers, OpError> {
  let start = match object_member(scope, object, "start")? {
    Some(value) => Some(mark_ref(scope, value)?),
    None => None,
  };
  let end = match object_member(scope, object, "end")? {
    Some(value) => Some(mark_ref(scope, value)?),
    None => None,
  };
  let duration = match object_member(scope, object, "duration")? {
    Some(value) => Some(try_number(scope, value)?),
    None => None,
  };
  let detail = object_member(scope, object, "detail")?
    .map(|value| v8::Global::new(scope, value));
  Ok(MeasureMembers {
    start,
    end,
    duration,
    detail,
  })
}

fn mark_ref(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<MarkRef, OpError> {
  if value.is_number() {
    return Ok(MarkRef::Time(try_number(scope, value)?));
  }
  Ok(MarkRef::Name(util::try_string(scope, value)?))
}

fn observe_options<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  args: &v8::FunctionCallbackArguments,
) -> Result<ObserveArgs<'s>, OpError> {
  if args.length() < 1 {
    return Ok(ObserveArgs {
      entry_types: None,
      type_name: None,
      buffered: false,
      buffered_present: false,
    });
  }
  let value = args.get(0);
  if value.is_null() || value.is_undefined() {
    return Ok(ObserveArgs {
      entry_types: None,
      type_name: None,
      buffered: false,
      buffered_present: false,
    });
  }
  if !value.is_object() {
    return Err(OpError::type_("The observe options are not an object."));
  }
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The observe options are not an object."))?;
  let entry_types = object_member(scope, object, "entryTypes")?;
  let type_name = object_member(scope, object, "type")?;
  let buffered_value = object_member(scope, object, "buffered")?;
  let buffered = buffered_value
    .map(|value| value.boolean_value(scope))
    .unwrap_or(false);
  Ok(ObserveArgs {
    entry_types,
    type_name,
    buffered,
    buffered_present: buffered_value.is_some(),
  })
}

fn entry_type_sequence(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<String>, OpError> {
  if !value.is_object() {
    return Err(OpError::type_("entryTypes is not a sequence."));
  }
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("entryTypes is not a sequence."))?;
  let symbol = v8::Symbol::get_iterator(scope);
  let method = call_get(scope, object, symbol.into())?;
  let method = v8::Local::<v8::Function>::try_from(method)
    .map_err(|_| OpError::type_("entryTypes is not a sequence."))?;
  let iterator = call_function(scope, method, object.into(), &[])?;
  let iterator = v8::Local::<v8::Object>::try_from(iterator)
    .map_err(|_| OpError::type_("entryTypes is not a sequence."))?;
  let mut types = Vec::new();
  loop {
    let next = call_get(scope, iterator, key(scope, "next").into())?;
    let next = v8::Local::<v8::Function>::try_from(next)
      .map_err(|_| OpError::type_("entryTypes is not a sequence."))?;
    let step = call_function(scope, next, iterator.into(), &[])?;
    let step = v8::Local::<v8::Object>::try_from(step)
      .map_err(|_| OpError::type_("entryTypes is not a sequence."))?;
    let done = call_get(scope, step, key(scope, "done").into())?;
    if done.boolean_value(scope) {
      return Ok(types);
    }
    let item = call_get(scope, step, key(scope, "value").into())?;
    types.push(util::try_string(scope, item)?);
  }
}

fn dictionary_arg<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  args: &v8::FunctionCallbackArguments,
  index: i32,
) -> Result<Option<v8::Local<'s, v8::Object>>, OpError> {
  if args.length() <= index {
    return Ok(None);
  }
  let value = args.get(index);
  if value.is_null() || value.is_undefined() {
    return Ok(None);
  }
  if !value.is_object() {
    return Err(OpError::type_("The mark options are not an object."));
  }
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The mark options are not an object."))?;
  Ok(Some(yuzuko_core::op::root(scope, object)))
}

fn required_string(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  index: i32,
  message: &str,
) -> Result<String, OpError> {
  if args.length() <= index {
    return Err(OpError::type_(message));
  }
  util::try_string(scope, args.get(index))
}

fn given_string(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  index: i32,
) -> Result<Option<String>, OpError> {
  if args.length() <= index {
    return Ok(None);
  }
  let value = args.get(index);
  if value.is_undefined() {
    return Ok(None);
  }
  Ok(Some(util::try_string(scope, value)?))
}

fn optional_string(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  index: i32,
) -> Result<Option<String>, OpError> {
  given_string(scope, args, index)
}

fn object_member<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, OpError> {
  let property = key(scope, name);
  let captured = {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, property.into()) {
      None => Err(pending_exception!(tc)),
      Some(value) if value.is_undefined() => Ok(None),
      Some(value) => Ok(Some(v8::Global::new(tc, value))),
    }
  };
  match captured {
    Ok(None) => Ok(None),
    Ok(Some(value)) => Ok(Some(v8::Local::new(scope, value))),
    Err(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
  }
}

fn try_number(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<f64, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match value.number_value(tc) {
      Some(number) => Ok(number),
      None => Err(pending_exception!(tc)),
    }
  };
  match captured {
    Ok(number) => Ok(number),
    Err(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
  }
}

fn call_get<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<v8::Object>,
  property: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match object.get(tc, property) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(pending_exception!(tc)),
    }
  };
  match captured {
    Ok(value) => Ok(v8::Local::new(scope, value)),
    Err(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
  }
}

fn call_function<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  function: v8::Local<v8::Function>,
  receiver: v8::Local<v8::Value>,
  args: &[v8::Local<v8::Value>],
) -> Result<v8::Local<'s, v8::Value>, OpError> {
  let captured = {
    v8::tc_scope!(let tc, scope);
    match function.call(tc, receiver, args) {
      Some(value) => Ok(v8::Global::new(tc, value)),
      None => Err(pending_exception!(tc)),
    }
  };
  match captured {
    Ok(value) => Ok(v8::Local::new(scope, value)),
    Err(exception) => {
      Err(OpError::thrown(scope, v8::Local::new(scope, exception)))
    }
  }
}

fn detail_value(
  scope: &mut v8::PinScope,
  detail: &RefCell<Option<v8::TracedReference<v8::Value>>>,
) -> Js {
  let value = detail_local(scope, detail);
  js_value(scope, value)
}

fn detail_local<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  detail: &RefCell<Option<v8::TracedReference<v8::Value>>>,
) -> v8::Local<'s, v8::Value> {
  detail
    .borrow()
    .as_ref()
    .and_then(|reference| traced_local(scope, reference))
    .unwrap_or_else(|| v8::null(scope).into())
}

fn entry_json(
  scope: &mut v8::PinScope,
  entry: &PerformanceEntry,
  detail: Option<v8::Local<v8::Value>>,
) -> Js {
  let object = v8::Object::new(scope);
  let id = v8::Number::new(scope, entry.id as f64);
  let _ =
    object.create_data_property(scope, key(scope, "id").into(), id.into());
  let name = v8::String::new(scope, &entry.name).unwrap();
  let _ =
    object.create_data_property(scope, key(scope, "name").into(), name.into());
  let entry_type = v8::String::new(scope, &entry.entry_type).unwrap();
  let _ = object.create_data_property(
    scope,
    key(scope, "entryType").into(),
    entry_type.into(),
  );
  let start_time = v8::Number::new(scope, entry.start_time);
  let _ = object.create_data_property(
    scope,
    key(scope, "startTime").into(),
    start_time.into(),
  );
  let duration = v8::Number::new(scope, entry.duration);
  let _ = object.create_data_property(
    scope,
    key(scope, "duration").into(),
    duration.into(),
  );
  let navigation_id = v8::Number::new(scope, entry.navigation_id as f64);
  let _ = object.create_data_property(
    scope,
    key(scope, "navigationId").into(),
    navigation_id.into(),
  );
  if let Some(detail) = detail {
    let _ =
      object.create_data_property(scope, key(scope, "detail").into(), detail);
  }
  js_value(scope, object.into())
}

fn object_array(
  scope: &mut v8::PinScope,
  objects: &[v8::Local<v8::Object>],
) -> Js {
  let array = v8::Array::new(scope, objects.len() as i32);
  for (index, object) in objects.iter().enumerate() {
    array.set_index(scope, index as u32, (*object).into());
  }
  js_value(scope, array.into())
}

fn js_value(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Js {
  Js(v8::Global::new(scope, value))
}
