//! Web Audio classes installed on `globalThis`.

use std::cell::RefCell;
use std::collections::HashMap;

use yuzuko_core::op::{Js, New, OpError};
use yuzuko_dom::EventTarget;
use yuzuko_macros::op;

use crate::decode::{self, DecodeFailure};
use crate::engine::{
  self, DriveResult, EvalError, Fault, Kind, NodeShell, Play, RenderHint, Wave,
  WorkletParam,
};
use crate::util;

fn trace_event(target: &EventTarget, visitor: &mut v8::cppgc::Visitor) {
  v8::cppgc::GarbageCollected::trace(target, visitor);
}

fn op_fault(fault: Fault) -> OpError {
  fault.op()
}

fn as_u32(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u32, OpError> {
  value
    .uint32_value(scope)
    .ok_or_else(|| OpError::type_("The value is not an unsigned integer"))
}

fn promise_of(
  scope: &mut v8::PinScope,
  settle: impl FnOnce(&mut v8::PinScope, v8::Local<v8::PromiseResolver>),
) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  settle(scope, resolver);
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

fn resolve_undefined(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
) {
  let value: v8::Local<v8::Value> = v8::undefined(scope).into();
  resolver.resolve(scope, value);
}

pub fn require_context(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u64, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The context is not an AudioContext"))?;
  let Some(host) =
    (unsafe { yuzuko_core::op::host_ref::<BaseAudioContext>(scope, object) })
  else {
    return Err(OpError::type_("The context is not an AudioContext"));
  };
  let id = host.get().id;
  if engine::context_rate(id).is_none() {
    return Err(OpError::type_("The context is not an AudioContext"));
  }
  Ok(id)
}

fn node_id(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u64, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The destination is not an AudioNode"))?;
  let Some(host) =
    (unsafe { yuzuko_core::op::host_ref::<AudioNode>(scope, object) })
  else {
    return Err(OpError::type_("The destination is not an AudioNode"));
  };
  Ok(host.get().id)
}

fn parse_hint(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<RenderHint, OpError> {
  if value.is_null() {
    return Err(OpError::type_("renderSizeHint is null"));
  }
  if value.is_number() {
    let size = as_u32(scope, value)?;
    return Ok(RenderHint::Frames(size));
  }
  if value.is_string() {
    return match util::rust_string(scope, value).as_str() {
      "default" => Ok(RenderHint::Default),
      "hardware" => Ok(RenderHint::Hardware),
      _ => Err(OpError::type_("renderSizeHint is not supported")),
    };
  }
  Err(OpError::type_("renderSizeHint is invalid"))
}

fn check_latency(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<(), OpError> {
  if value.is_number() {
    let number = value
      .number_value(scope)
      .ok_or_else(|| OpError::type_("latencyHint is not a number"))?;
    if !number.is_finite() {
      return Err(OpError::type_("latencyHint is not finite"));
    }
    return Ok(());
  }
  if value.is_string() {
    return match util::rust_string(scope, value).as_str() {
      "interactive" | "balanced" | "playback" => Ok(()),
      _ => Err(OpError::type_("latencyHint is invalid")),
    };
  }
  Err(OpError::type_("latencyHint is invalid"))
}

struct NodeOpts {
  count: u32,
  mode: &'static str,
  interpretation: &'static str,
}

fn node_options(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  count: u32,
  mode: &'static str,
  interpretation: &'static str,
) -> Result<NodeOpts, OpError> {
  if value.is_null_or_undefined() {
    return Ok(NodeOpts {
      count,
      mode,
      interpretation,
    });
  }
  if !value.is_object() {
    return Err(OpError::type_("AudioNode options are not an object"));
  }
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("AudioNode options are not an object"))?;
  let mut count = count;
  let mut mode = mode;
  let mut interpretation = interpretation;
  if let Some(raw) = util::member(scope, object, "channelCount") {
    let next = as_u32(scope, raw)?;
    if next == 0 || next > 32 {
      return Err(OpError::dom(
        "NotSupportedError",
        "The channel count is not supported",
      ));
    }
    count = next;
  }
  if let Some(raw) = util::member(scope, object, "channelCountMode") {
    mode = match util::rust_string(scope, raw).as_str() {
      "max" => "max",
      "clamped-max" => "clamped-max",
      "explicit" => "explicit",
      _ => return Err(OpError::type_("The channel count mode is invalid")),
    };
  }
  if let Some(raw) = util::member(scope, object, "channelInterpretation") {
    interpretation = match util::rust_string(scope, raw).as_str() {
      "speakers" => "speakers",
      "discrete" => "discrete",
      _ => {
        return Err(OpError::type_("The channel interpretation is invalid"));
      }
    };
  }
  Ok(NodeOpts {
    count,
    mode,
    interpretation,
  })
}

fn option_number(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  name: &str,
) -> Result<Option<f64>, OpError> {
  if value.is_null_or_undefined() || !value.is_object() {
    return Ok(None);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return Ok(None);
  };
  let Some(raw) = util::member(scope, object, name) else {
    return Ok(None);
  };
  Ok(Some(util::finite_number(scope, raw)?))
}

fn shell(
  context: u64,
  opts: &NodeOpts,
  inputs: u32,
  outputs: u32,
) -> NodeShell {
  NodeShell {
    context,
    inputs,
    outputs,
    channel_count: opts.count,
    max_channels: 32,
    index_over_max: false,
    mode: opts.mode,
    interpretation: opts.interpretation,
  }
}

fn make_param(
  scope: &mut v8::PinScope,
  value: f64,
  default: f64,
  min: f64,
  max: f64,
  a_rate: bool,
) -> u64 {
  let id = engine::alloc();
  engine::insert_param(id, value, default, min, max, a_rate);
  let object =
    unsafe { yuzuko_core::op::wrap_instance(scope, AudioParam { id }) };
  engine::set_param_object(scope, id, object);
  id
}

fn param_js(scope: &mut v8::PinScope, id: u64) -> Js {
  engine::param_object(id)
    .map(|object| {
      let local: v8::Local<v8::Value> = v8::Local::new(scope, object).into();
      Js(v8::Global::new(scope, local))
    })
    .unwrap_or_else(|| Js::null(scope))
}

fn global_object(
  scope: &mut v8::PinScope,
  object: v8::Global<v8::Object>,
) -> Js {
  let local: v8::Local<v8::Value> = v8::Local::new(scope, object).into();
  Js(v8::Global::new(scope, local))
}

#[repr(C)]
pub struct AudioBuffer {
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for AudioBuffer {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioBuffer"
  }
}

#[op]
impl AudioBuffer {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<AudioBuffer, OpError> {
    let init = args.get(0);
    if !init.is_object() || init.is_null() {
      return Err(OpError::type_("AudioBuffer options are not an object"));
    }
    let object = v8::Local::<v8::Object>::try_from(init)
      .map_err(|_| OpError::type_("AudioBuffer options are not an object"))?;
    let Some(length) = util::member(scope, object, "length") else {
      return Err(OpError::type_("length is required"));
    };
    let Some(rate) = util::member(scope, object, "sampleRate") else {
      return Err(OpError::type_("sampleRate is required"));
    };
    let channels = util::member(scope, object, "numberOfChannels")
      .map(|value| as_u32(scope, value))
      .transpose()?
      .unwrap_or(1);
    build_buffer(
      engine::check_channels(channels).map_err(op_fault)?,
      engine::check_length(as_u32(scope, length)?).map_err(op_fault)?,
      engine::check_rate(util::finite_number(scope, rate)?)
        .map_err(op_fault)?,
    )
  }

  #[getter]
  fn sample_rate(&self) -> f64 {
    engine::buffer_view(self.id)
      .map(|view| view.rate)
      .unwrap_or(0.0)
  }

  #[getter]
  fn length(&self) -> u32 {
    engine::buffer_view(self.id)
      .map(|view| view.length)
      .unwrap_or(0)
  }

  #[getter]
  fn duration(&self) -> f64 {
    let Some(view) = engine::buffer_view(self.id) else {
      return 0.0;
    };
    if view.rate == 0.0 {
      0.0
    } else {
      f64::from(view.length) / view.rate
    }
  }

  #[getter]
  fn number_of_channels(&self) -> u32 {
    engine::buffer_view(self.id)
      .map(|view| view.channels)
      .unwrap_or(0)
  }

  #[length(1)]
  fn get_channel_data(
    &self,
    scope: &mut v8::PinScope,
    channel: u32,
  ) -> Result<Js, OpError> {
    engine::channel_view(scope, self.id, channel).map_err(op_fault)
  }

  #[length(2)]
  fn copy_from_channel(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    let destination = v8::Local::<v8::Float32Array>::try_from(args.get(0))
      .map_err(|_| OpError::type_("destination is not a Float32Array"))?;
    let channel = as_u32(scope, args.get(1))?;
    let start = if args.length() > 2 && !args.get(2).is_undefined() {
      as_u32(scope, args.get(2))?
    } else {
      0
    };
    engine::copy_from_channel(scope, self.id, channel, start, destination)
      .map_err(op_fault)
  }

  #[length(2)]
  fn copy_to_channel(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    let source = v8::Local::<v8::Float32Array>::try_from(args.get(0))
      .map_err(|_| OpError::type_("source is not a Float32Array"))?;
    let channel = as_u32(scope, args.get(1))?;
    let start = if args.length() > 2 && !args.get(2).is_undefined() {
      as_u32(scope, args.get(2))?
    } else {
      0
    };
    engine::copy_to_channel(scope, self.id, channel, start, source)
      .map_err(op_fault)
  }
}

fn build_buffer(
  channels: u32,
  length: u32,
  rate: f64,
) -> Result<AudioBuffer, OpError> {
  let id = engine::alloc();
  let data = vec![vec![0.0; length as usize]; channels as usize];
  engine::insert_buffer(id, rate, data);
  Ok(AudioBuffer { id })
}

fn buffer_from_pcm(pcm: decode::Pcm) -> Result<u64, OpError> {
  if pcm.channels.is_empty() {
    return Err(OpError::dom(
      "EncodingError",
      "The audio data could not be decoded",
    ));
  }
  let id = engine::alloc();
  engine::insert_buffer(id, pcm.sample_rate, pcm.channels);
  Ok(id)
}

#[repr(C)]
pub struct AudioParam {
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for AudioParam {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioParam"
  }
}

fn automation() -> Result<(), OpError> {
  Err(OpError::dom(
    "NotSupportedError",
    "AudioParam automation is not implemented",
  ))
}

#[op]
impl AudioParam {
  #[getter]
  fn value(&self) -> f64 {
    engine::param_view(self.id)
      .map(|param| param.value)
      .unwrap_or(0.0)
  }

  #[setter]
  fn value(&self, value: f64) -> Result<(), OpError> {
    engine::set_param_value(self.id, value).map_err(op_fault)
  }

  #[getter]
  fn default_value(&self) -> f64 {
    engine::param_view(self.id)
      .map(|param| param.default)
      .unwrap_or(0.0)
  }

  #[getter]
  fn min_value(&self) -> f64 {
    engine::param_view(self.id)
      .map(|param| param.min)
      .unwrap_or(0.0)
  }

  #[getter]
  fn max_value(&self) -> f64 {
    engine::param_view(self.id)
      .map(|param| param.max)
      .unwrap_or(0.0)
  }

  #[getter]
  fn automation_rate(&self) -> String {
    if engine::param_view(self.id).is_some_and(|param| param.a_rate) {
      "a-rate".to_string()
    } else {
      "k-rate".to_string()
    }
  }

  #[setter]
  fn automation_rate(&self, value: String) -> Result<(), OpError> {
    let a_rate = match value.as_str() {
      "a-rate" => true,
      "k-rate" => false,
      _ => return Err(OpError::type_("The automation rate is invalid")),
    };
    engine::set_param_rate(self.id, a_rate).map_err(op_fault)
  }

  #[length(2)]
  fn set_value_at_time(&self) -> Result<(), OpError> {
    automation()
  }

  #[length(2)]
  fn linear_ramp_to_value_at_time(&self) -> Result<(), OpError> {
    automation()
  }

  #[length(2)]
  fn exponential_ramp_to_value_at_time(&self) -> Result<(), OpError> {
    automation()
  }

  #[length(3)]
  fn set_target_at_time(&self) -> Result<(), OpError> {
    automation()
  }

  #[length(3)]
  fn set_value_curve_at_time(&self) -> Result<(), OpError> {
    automation()
  }

  #[length(1)]
  fn cancel_scheduled_values(&self) -> Result<(), OpError> {
    automation()
  }

  #[length(1)]
  fn cancel_and_hold_at_time(&self) -> Result<(), OpError> {
    automation()
  }
}

#[repr(C)]
pub struct AudioParamMap {
  node: u64,
}

unsafe impl v8::cppgc::GarbageCollected for AudioParamMap {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioParamMap"
  }
}

#[op]
impl AudioParamMap {
  #[getter]
  fn size(&self) -> u32 {
    engine::worklet_param_ids(self.node).len() as u32
  }

  #[length(1)]
  fn get(&self, scope: &mut v8::PinScope, name: String) -> Js {
    let params = engine::worklet_param_ids(self.node);
    if let Some((_, id)) = params.into_iter().find(|(found, _)| found == &name)
    {
      return param_js(scope, id);
    }
    let missing: v8::Local<v8::Value> = v8::undefined(scope).into();
    Js(v8::Global::new(scope, missing))
  }

  #[length(1)]
  fn has(&self, name: String) -> bool {
    engine::worklet_param_ids(self.node)
      .iter()
      .any(|(found, _)| found == &name)
  }
}

#[repr(C)]
pub struct AudioNode {
  pub base: EventTarget,
  pub id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for AudioNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    trace_event(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioNode"
  }
}

#[op(inherit = EventTarget)]
impl AudioNode {
  #[getter]
  fn number_of_inputs(&self) -> u32 {
    engine::node_info(self.id)
      .map(|info| info.inputs)
      .unwrap_or(0)
  }

  #[getter]
  fn number_of_outputs(&self) -> u32 {
    engine::node_info(self.id)
      .map(|info| info.outputs)
      .unwrap_or(0)
  }

  #[getter]
  fn channel_count(&self) -> u32 {
    engine::node_info(self.id)
      .map(|info| info.channel_count)
      .unwrap_or(0)
  }

  #[setter]
  fn channel_count(&self, value: u32) -> Result<(), OpError> {
    engine::set_channel_count(self.id, value).map_err(op_fault)
  }

  #[getter]
  fn channel_count_mode(&self) -> String {
    engine::node_info(self.id)
      .map(|info| info.mode.to_string())
      .unwrap_or_else(|| "max".to_string())
  }

  #[setter]
  fn channel_count_mode(&self, value: String) -> Result<(), OpError> {
    engine::set_channel_mode(self.id, &value).map_err(op_fault)
  }

  #[getter]
  fn channel_interpretation(&self) -> String {
    engine::node_info(self.id)
      .map(|info| info.interpretation.to_string())
      .unwrap_or_else(|| "speakers".to_string())
  }

  #[setter]
  fn channel_interpretation(&self, value: String) -> Result<(), OpError> {
    engine::set_interpretation(self.id, &value).map_err(op_fault)
  }

  #[length(1)]
  fn connect(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Js, OpError> {
    let destination = args.get(0);
    if v8::Local::<v8::Object>::try_from(destination).is_ok_and(|object| {
      unsafe { yuzuko_core::op::host_ref::<AudioParam>(scope, object) }
        .is_some()
    }) {
      return Err(OpError::dom(
        "NotSupportedError",
        "Connecting to an AudioParam is not implemented",
      ));
    }
    let dest = node_id(scope, destination)?;
    let output = if args.length() > 1 && !args.get(1).is_undefined() {
      as_u32(scope, args.get(1))?
    } else {
      0
    };
    let input = if args.length() > 2 && !args.get(2).is_undefined() {
      as_u32(scope, args.get(2))?
    } else {
      0
    };
    engine::connect_node(self.id, dest, output, input).map_err(op_fault)?;
    Ok(Js(v8::Global::new(scope, destination)))
  }

  fn disconnect(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    if args.length() == 0 || args.get(0).is_undefined() {
      return engine::disconnect_all(self.id).map_err(op_fault);
    }
    let value = args.get(0);
    if value.is_number() {
      return engine::disconnect_output(self.id, as_u32(scope, value)?)
        .map_err(op_fault);
    }
    let dest = node_id(scope, value)?;
    engine::disconnect_dest(self.id, dest).map_err(op_fault)
  }
}

#[repr(C)]
pub struct AudioDestinationNode {
  pub base: AudioNode,
}

unsafe impl v8::cppgc::GarbageCollected for AudioDestinationNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioDestinationNode"
  }
}

#[op(inherit = AudioNode)]
impl AudioDestinationNode {
  #[getter]
  fn max_channel_count(&self) -> u32 {
    engine::node_info(self.base.id)
      .map(|info| info.max_channels)
      .unwrap_or(2)
  }
}

fn make_destination(
  scope: &mut v8::PinScope,
  context: u64,
  channels: u32,
  realtime: bool,
) -> (u64, v8::Global<v8::Object>) {
  let id = engine::alloc();
  let max = if realtime { channels.max(2) } else { channels };
  engine::insert_node(
    id,
    NodeShell {
      context,
      inputs: 1,
      outputs: 0,
      channel_count: channels,
      max_channels: max,
      index_over_max: true,
      mode: "explicit",
      interpretation: "speakers",
    },
    Kind::Destination,
  );
  let object = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      AudioDestinationNode {
        base: AudioNode {
          base: EventTarget::new(),
          id,
        },
      },
    )
  };
  engine::set_node_object(scope, id, object);
  (id, v8::Global::new(scope, object))
}

#[repr(C)]
pub struct AudioScheduledSourceNode {
  pub base: AudioNode,
}

unsafe impl v8::cppgc::GarbageCollected for AudioScheduledSourceNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioScheduledSourceNode"
  }
}

#[op(inherit = AudioNode)]
impl AudioScheduledSourceNode {
  #[getter]
  fn onended(&self, scope: &mut v8::PinScope) -> Js {
    engine::handler(scope, self.base.id, "ended")
  }

  #[setter]
  fn onended(&self, scope: &mut v8::PinScope, value: v8::Local<v8::Value>) {
    engine::bind_handler(scope, &self.base.base, self.base.id, "ended", value);
  }

  fn start(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    start_source(scope, self.base.id, args)
  }

  fn stop(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    let when = optional_time(scope, self.base.id, args)?;
    engine::source_stop(self.base.id, when).map_err(op_fault)
  }
}

fn optional_time(
  scope: &mut v8::PinScope,
  node: u64,
  args: &v8::FunctionCallbackArguments,
) -> Result<f64, OpError> {
  if args.length() == 0 || args.get(0).is_undefined() {
    let ctx = engine::node_info(node)
      .map(|info| info.context)
      .unwrap_or(0);
    return Ok(engine::context_time(ctx).unwrap_or(0.0));
  }
  util::finite_number(scope, args.get(0))
}

fn start_source(
  scope: &mut v8::PinScope,
  id: u64,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), OpError> {
  let when = optional_time(scope, id, args)?;
  let offset = if args.length() > 1 && !args.get(1).is_undefined() {
    util::finite_number(scope, args.get(1))?
  } else {
    0.0
  };
  let duration = if args.length() > 2 && !args.get(2).is_undefined() {
    Some(util::finite_number(scope, args.get(2))?)
  } else {
    None
  };
  engine::source_start(scope, id, when, offset, duration).map_err(op_fault)
}

fn remember(scope: &mut v8::PinScope, id: u64, object: v8::Local<v8::Object>) {
  engine::set_node_object(scope, id, object);
}

#[repr(C)]
pub struct GainNode {
  pub base: AudioNode,
  gain: u64,
}

unsafe impl v8::cppgc::GarbageCollected for GainNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"GainNode"
  }
}

fn build_gain(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
) -> Result<GainNode, OpError> {
  let opts = node_options(scope, options, 2, "max", "speakers")?;
  let gain = option_number(scope, options, "gain")?.unwrap_or(1.0);
  let (min, max) = engine::float_limits();
  let param = make_param(scope, gain, 1.0, min, max, true);
  let id = engine::alloc();
  engine::insert_node(id, shell(context, &opts, 1, 1), Kind::Gain { param });
  Ok(GainNode {
    base: AudioNode {
      base: EventTarget::new(),
      id,
    },
    gain: param,
  })
}

#[op(inherit = AudioNode)]
impl GainNode {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<GainNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    build_gain(scope, context, options)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.id, object);
  }

  #[getter]
  fn gain(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.gain)
  }
}

#[repr(C)]
pub struct ConstantSourceNode {
  pub base: AudioScheduledSourceNode,
  offset: u64,
}

unsafe impl v8::cppgc::GarbageCollected for ConstantSourceNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ConstantSourceNode"
  }
}

fn build_constant(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
) -> Result<ConstantSourceNode, OpError> {
  let opts = node_options(scope, options, 2, "max", "speakers")?;
  let offset = option_number(scope, options, "offset")?.unwrap_or(1.0);
  let (min, max) = engine::float_limits();
  let param = make_param(scope, offset, 1.0, min, max, true);
  let id = engine::alloc();
  engine::insert_node(
    id,
    shell(context, &opts, 0, 1),
    Kind::Constant {
      offset: param,
      play: Play::idle(),
    },
  );
  Ok(ConstantSourceNode {
    base: AudioScheduledSourceNode {
      base: AudioNode {
        base: EventTarget::new(),
        id,
      },
    },
    offset: param,
  })
}

#[op(inherit = AudioScheduledSourceNode)]
impl ConstantSourceNode {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<ConstantSourceNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    build_constant(scope, context, options)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.base.id, object);
  }

  #[getter]
  fn offset(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.offset)
  }
}

#[repr(C)]
pub struct OscillatorNode {
  pub base: AudioScheduledSourceNode,
  frequency: u64,
  detune: u64,
}

unsafe impl v8::cppgc::GarbageCollected for OscillatorNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"OscillatorNode"
  }
}

fn build_oscillator(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
) -> Result<OscillatorNode, OpError> {
  let opts = node_options(scope, options, 2, "max", "speakers")?;
  let rate = engine::context_rate(context).unwrap_or(48000.0);
  let nyquist = rate / 2.0;
  let frequency = option_number(scope, options, "frequency")?.unwrap_or(440.0);
  let detune = option_number(scope, options, "detune")?.unwrap_or(0.0);
  let (min, max) = engine::float_limits();
  let frequency_id =
    make_param(scope, frequency, 440.0, -nyquist, nyquist, true);
  let detune_id = make_param(scope, detune, 0.0, min, max, true);
  let wave = oscillator_wave(scope, context, options)?;
  let id = engine::alloc();
  engine::insert_node(
    id,
    shell(context, &opts, 0, 1),
    Kind::Oscillator {
      frequency: frequency_id,
      detune: detune_id,
      wave,
      play: Play::idle(),
    },
  );
  Ok(OscillatorNode {
    base: AudioScheduledSourceNode {
      base: AudioNode {
        base: EventTarget::new(),
        id,
      },
    },
    frequency: frequency_id,
    detune: detune_id,
  })
}

fn oscillator_wave(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
) -> Result<Wave, OpError> {
  let object = if options.is_object() && !options.is_null() {
    v8::Local::<v8::Object>::try_from(options).ok()
  } else {
    None
  };
  if let Some(object) = object
    && let Some(raw) = util::member(scope, object, "periodicWave")
  {
    if raw.is_null() {
      return Err(OpError::type_("periodicWave is null"));
    }
    let wave = wave_id(scope, raw)?;
    let Some((wave_context, wave)) = engine::copy_wave(wave) else {
      return Err(OpError::type_("The periodic wave is invalid"));
    };
    if wave_context != context {
      return Err(OpError::dom(
        "InvalidAccessError",
        "The periodic wave belongs to a different context",
      ));
    }
    return Ok(wave);
  }
  let name = object
    .and_then(|object| util::member(scope, object, "type"))
    .map(|value| util::rust_string(scope, value))
    .unwrap_or_else(|| "sine".to_string());
  match name.as_str() {
    "sine" => Ok(Wave::Sine),
    "square" => Ok(Wave::Square),
    "sawtooth" => Ok(Wave::Sawtooth),
    "triangle" => Ok(Wave::Triangle),
    "custom" => Err(OpError::dom(
      "InvalidStateError",
      "A custom oscillator requires a PeriodicWave",
    )),
    _ => Err(OpError::type_("The oscillator type is invalid")),
  }
}

#[op(inherit = AudioScheduledSourceNode)]
impl OscillatorNode {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<OscillatorNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    build_oscillator(scope, context, options)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.base.id, object);
  }

  #[getter]
  #[rename("type")]
  fn type_(&self) -> String {
    engine::oscillator_type(self.base.base.id)
      .unwrap_or_else(|| "sine".to_string())
  }

  #[setter]
  #[rename("type")]
  fn type_(&self, value: String) -> Result<(), OpError> {
    engine::set_oscillator_type(self.base.base.id, &value).map_err(op_fault)
  }

  #[getter]
  fn frequency(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.frequency)
  }

  #[getter]
  fn detune(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.detune)
  }

  #[length(1)]
  fn set_periodic_wave(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let wave = wave_id(scope, value)?;
    engine::set_periodic_wave(self.base.base.id, wave).map_err(op_fault)
  }
}

#[repr(C)]
pub struct AudioBufferSourceNode {
  pub base: AudioScheduledSourceNode,
  playback_rate: u64,
  detune: u64,
}

unsafe impl v8::cppgc::GarbageCollected for AudioBufferSourceNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioBufferSourceNode"
  }
}

fn build_buffer_source(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
) -> Result<AudioBufferSourceNode, OpError> {
  let opts = node_options(scope, options, 2, "max", "speakers")?;
  let playback = option_number(scope, options, "playbackRate")?.unwrap_or(1.0);
  let detune = option_number(scope, options, "detune")?.unwrap_or(0.0);
  let (min, max) = engine::float_limits();
  let rate_id = make_param(scope, playback, 1.0, min, max, true);
  let detune_id = make_param(scope, detune, 0.0, min, max, true);
  let buffer = source_buffer_option(scope, options)?;
  let looped = option_bool(scope, options, "loop");
  let loop_start = option_number(scope, options, "loopStart")?.unwrap_or(0.0);
  let loop_end = option_number(scope, options, "loopEnd")?.unwrap_or(0.0);
  let id = engine::alloc();
  engine::insert_node(
    id,
    shell(context, &opts, 0, 1),
    Kind::Buffer {
      buffer,
      acquired: None,
      rate: rate_id,
      detune: detune_id,
      play: Play::idle(),
      looped,
      loop_start,
      loop_end,
    },
  );
  Ok(AudioBufferSourceNode {
    base: AudioScheduledSourceNode {
      base: AudioNode {
        base: EventTarget::new(),
        id,
      },
    },
    playback_rate: rate_id,
    detune: detune_id,
  })
}

fn option_bool(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
  name: &str,
) -> bool {
  if !value.is_object() || value.is_null() {
    return false;
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return false;
  };
  util::member(scope, object, name)
    .is_some_and(|value| value.boolean_value(scope))
}

fn source_buffer_option(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
) -> Result<Option<u64>, OpError> {
  if !options.is_object() || options.is_null() {
    return Ok(None);
  }
  let Ok(object) = v8::Local::<v8::Object>::try_from(options) else {
    return Ok(None);
  };
  let Some(raw) = util::member(scope, object, "buffer") else {
    return Ok(None);
  };
  if raw.is_null() {
    return Ok(None);
  }
  buffer_id(scope, raw).map(Some)
}

fn buffer_id(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u64, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The buffer is not an AudioBuffer"))?;
  let Some(host) =
    (unsafe { yuzuko_core::op::host_ref::<AudioBuffer>(scope, object) })
  else {
    return Err(OpError::type_("The buffer is not an AudioBuffer"));
  };
  Ok(host.get().id)
}

#[op(inherit = AudioScheduledSourceNode)]
impl AudioBufferSourceNode {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<AudioBufferSourceNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    build_buffer_source(scope, context, options)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.base.id, object);
  }

  #[getter]
  fn buffer(&self, scope: &mut v8::PinScope) -> Js {
    match engine::source_buffer(self.base.base.id) {
      Some(Some(id)) => {
        let object =
          unsafe { yuzuko_core::op::wrap_instance(scope, AudioBuffer { id }) };
        global_object(scope, v8::Global::new(scope, object))
      }
      _ => Js::null(scope),
    }
  }

  #[setter]
  fn buffer(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let buffer = if value.is_null() {
      None
    } else {
      Some(buffer_id(scope, value)?)
    };
    engine::set_source_buffer(scope, self.base.base.id, buffer)
      .map_err(op_fault)
  }

  #[getter]
  fn playback_rate(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.playback_rate)
  }

  #[getter]
  fn detune(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.detune)
  }

  #[getter]
  #[rename("loop")]
  fn loop_(&self) -> bool {
    engine::loop_state(self.base.base.id)
      .map(|(looped, _, _)| looped)
      .unwrap_or(false)
  }

  #[setter]
  #[rename("loop")]
  fn loop_(&self, value: bool) {
    engine::set_loop(self.base.base.id, value, None, None);
  }

  #[getter]
  fn loop_start(&self) -> f64 {
    engine::loop_state(self.base.base.id)
      .map(|(_, start, _)| start)
      .unwrap_or(0.0)
  }

  #[setter]
  fn loop_start(&self, value: f64) {
    let looped = engine::loop_state(self.base.base.id)
      .map(|(looped, _, _)| looped)
      .unwrap_or(false);
    engine::set_loop(self.base.base.id, looped, Some(value), None);
  }

  #[getter]
  fn loop_end(&self) -> f64 {
    engine::loop_state(self.base.base.id)
      .map(|(_, _, end)| end)
      .unwrap_or(0.0)
  }

  #[setter]
  fn loop_end(&self, value: f64) {
    let looped = engine::loop_state(self.base.base.id)
      .map(|(looped, _, _)| looped)
      .unwrap_or(false);
    engine::set_loop(self.base.base.id, looped, None, Some(value));
  }
}

#[repr(C)]
pub struct PeriodicWave {
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for PeriodicWave {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"PeriodicWave"
  }
}

fn wave_id(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<u64, OpError> {
  let object = v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("The periodic wave is invalid"))?;
  let Some(host) =
    (unsafe { yuzuko_core::op::host_ref::<PeriodicWave>(scope, object) })
  else {
    return Err(OpError::type_("The periodic wave is invalid"));
  };
  Ok(host.get().id)
}

fn build_wave(
  _scope: &mut v8::PinScope,
  context: u64,
  real: Vec<f32>,
  imag: Vec<f32>,
  disable_normalization: bool,
) -> Result<PeriodicWave, OpError> {
  let count = real.len().max(imag.len());
  if count < 2 {
    return Err(OpError::dom(
      "IndexSizeError",
      "PeriodicWave requires at least two coefficients",
    ));
  }
  let mut real = real;
  let mut imag = imag;
  real.resize(count, 0.0);
  imag.resize(count, 0.0);
  if !disable_normalization {
    let mut sum = 0.0f32;
    for index in 1..count {
      sum += real[index].abs() + imag[index].abs();
    }
    if sum > 0.0 {
      let scale = 1.0 / sum;
      for index in 0..count {
        real[index] *= scale;
        imag[index] *= scale;
      }
    }
  }
  let id = engine::alloc();
  engine::register_wave(id, context, real, imag);
  Ok(PeriodicWave { id })
}

#[op]
impl PeriodicWave {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<PeriodicWave, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    if !options.is_object() || options.is_null() {
      return Err(OpError::type_("PeriodicWave options are not an object"));
    }
    let object = v8::Local::<v8::Object>::try_from(options)
      .map_err(|_| OpError::type_("PeriodicWave options are not an object"))?;
    let real = util::member(scope, object, "real")
      .map(|value| util::read_f32_sequence(scope, value))
      .transpose()?
      .unwrap_or_default();
    let imag = util::member(scope, object, "imag")
      .map(|value| util::read_f32_sequence(scope, value))
      .transpose()?
      .unwrap_or_default();
    let disable = util::member(scope, object, "disableNormalization")
      .is_some_and(|value| value.boolean_value(scope));
    build_wave(scope, context, real, imag, disable)
  }
}

#[repr(C)]
pub struct BaseAudioContext {
  pub base: EventTarget,
  pub id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for BaseAudioContext {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    trace_event(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"BaseAudioContext"
  }
}

#[op(inherit = EventTarget)]
impl BaseAudioContext {
  #[getter]
  fn destination(&self, scope: &mut v8::PinScope) -> Js {
    engine::destination_object(self.id)
      .map(|object| global_object(scope, object))
      .unwrap_or_else(|| Js::null(scope))
  }

  #[getter]
  fn sample_rate(&self) -> f64 {
    engine::context_rate(self.id).unwrap_or(0.0)
  }

  #[getter]
  fn current_time(&self) -> f64 {
    engine::context_time(self.id).unwrap_or(0.0)
  }

  #[getter]
  fn render_quantum_size(&self) -> u32 {
    engine::context_quantum(self.id).unwrap_or(128)
  }

  #[getter]
  fn state(&self) -> String {
    engine::context_state(self.id)
      .unwrap_or("closed")
      .to_string()
  }

  #[getter]
  fn onstatechange(&self, scope: &mut v8::PinScope) -> Js {
    engine::handler(scope, self.id, "statechange")
  }

  #[setter]
  fn onstatechange(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) {
    engine::bind_handler(scope, &self.base, self.id, "statechange", value);
  }

  #[getter]
  fn audio_worklet(&self, scope: &mut v8::PinScope) -> Result<Js, OpError> {
    if let Some(object) = worklet_object(self.id) {
      return Ok(global_object(scope, object));
    }
    engine::ensure_worklet(scope, self.id).map_err(op_fault)?;
    let object = unsafe {
      yuzuko_core::op::wrap_instance(
        scope,
        AudioWorklet {
          base: EventTarget::new(),
          id: self.id,
        },
      )
    };
    let global = v8::Global::new(scope, object);
    store_worklet(self.id, global.clone());
    Ok(global_object(scope, global))
  }

  #[promise]
  #[length(1)]
  fn decode_audio_data(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Js, OpError> {
    let value = args.get(0);
    let buffer = v8::Local::<v8::ArrayBuffer>::try_from(value)
      .map_err(|_| OpError::type_("decodeAudioData requires an ArrayBuffer"))?;
    let success = callback_at(scope, args, 1);
    let failure = callback_at(scope, args, 2);
    if buffer.was_detached() {
      return Ok(settle_decode(
        scope,
        Ok(DecodeSettle::Clone),
        success,
        failure,
      ));
    }
    let length = buffer.byte_length();
    let mut bytes = vec![0u8; length];
    if let Some(data) = buffer.data()
      && length > 0
    {
      unsafe {
        std::ptr::copy_nonoverlapping(
          data.as_ptr().cast::<u8>(),
          bytes.as_mut_ptr(),
          length,
        );
      }
    }
    let _ = buffer.detach(None);
    let decoded = match decode::decode(&bytes) {
      Ok(pcm) => DecodeSettle::Buffer(buffer_from_pcm(pcm)?),
      Err(DecodeFailure::Panic) => DecodeSettle::Invalid,
      Err(DecodeFailure::Encoding) => DecodeSettle::Encoding,
    };
    Ok(settle_decode(scope, Ok(decoded), success, failure))
  }

  #[length(3)]
  fn create_buffer(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<New<AudioBuffer>, OpError> {
    let channels =
      engine::check_channels(as_u32(scope, args.get(0))?).map_err(op_fault)?;
    let length =
      engine::check_length(as_u32(scope, args.get(1))?).map_err(op_fault)?;
    let rate = engine::check_rate(util::finite_number(scope, args.get(2))?)
      .map_err(op_fault)?;
    Ok(New(build_buffer(channels, length, rate)?))
  }

  fn create_buffer_source(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<New<AudioBufferSourceNode>, OpError> {
    let options = if args.length() > 0 {
      args.get(0)
    } else {
      v8::undefined(scope).into()
    };
    Ok(New(build_buffer_source(scope, self.id, options)?))
  }

  fn create_constant_source(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<New<ConstantSourceNode>, OpError> {
    let options = if args.length() > 0 {
      args.get(0)
    } else {
      v8::undefined(scope).into()
    };
    Ok(New(build_constant(scope, self.id, options)?))
  }

  fn create_gain(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<New<GainNode>, OpError> {
    let options = if args.length() > 0 {
      args.get(0)
    } else {
      v8::undefined(scope).into()
    };
    Ok(New(build_gain(scope, self.id, options)?))
  }

  fn create_oscillator(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<New<OscillatorNode>, OpError> {
    let options = if args.length() > 0 {
      args.get(0)
    } else {
      v8::undefined(scope).into()
    };
    Ok(New(build_oscillator(scope, self.id, options)?))
  }

  #[length(2)]
  fn create_periodic_wave(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<New<PeriodicWave>, OpError> {
    let real = util::read_f32_sequence(scope, args.get(0))?;
    let imag = util::read_f32_sequence(scope, args.get(1))?;
    let disable = if args.length() > 2 && args.get(2).is_object() {
      let object = v8::Local::<v8::Object>::try_from(args.get(2)).ok();
      object
        .and_then(|object| util::member(scope, object, "disableNormalization"))
        .is_some_and(|value| value.boolean_value(scope))
    } else {
      false
    };
    Ok(New(build_wave(scope, self.id, real, imag, disable)?))
  }
}

enum DecodeSettle {
  Buffer(u64),
  Encoding,
  Invalid,
  Clone,
}

fn callback_at(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
  index: i32,
) -> Option<v8::Global<v8::Function>> {
  if args.length() <= index {
    return None;
  }
  v8::Local::<v8::Function>::try_from(args.get(index))
    .ok()
    .map(|function| v8::Global::new(scope, function))
}

fn call_callback(
  scope: &mut v8::PinScope,
  callback: &Option<v8::Global<v8::Function>>,
  value: v8::Local<v8::Value>,
) {
  let Some(callback) = callback else {
    return;
  };
  v8::tc_scope!(let scope, scope);
  let callback = v8::Local::new(scope, callback);
  let receiver: v8::Local<v8::Value> = v8::undefined(scope).into();
  let _ = callback.call(scope, receiver, &[value]);
}

fn settle_decode(
  scope: &mut v8::PinScope,
  decoded: Result<DecodeSettle, OpError>,
  success: Option<v8::Global<v8::Function>>,
  failure: Option<v8::Global<v8::Function>>,
) -> Js {
  let resolver = v8::PromiseResolver::new(scope).unwrap();
  let promise = resolver.get_promise(scope);
  match decoded {
    Ok(DecodeSettle::Buffer(id)) => {
      let object =
        unsafe { yuzuko_core::op::wrap_instance(scope, AudioBuffer { id }) };
      let value: v8::Local<v8::Value> = object.into();
      call_callback(scope, &success, value);
      resolver.resolve(scope, value);
    }
    other => {
      let (name, message) = match other {
        Ok(DecodeSettle::Clone) => {
          ("DataCloneError", "The ArrayBuffer is detached")
        }
        Ok(DecodeSettle::Invalid) => {
          ("InvalidStateError", "The audio decoder failed")
        }
        _ => ("EncodingError", "The audio data could not be decoded"),
      };
      let error = util::dom_exception(scope, name, message);
      call_callback(scope, &failure, error);
      resolver.reject(scope, error);
    }
  }
  let promise: v8::Local<v8::Value> = promise.into();
  Js(v8::Global::new(scope, promise))
}

#[repr(C)]
pub struct AudioContext {
  pub base: BaseAudioContext,
}

unsafe impl v8::cppgc::GarbageCollected for AudioContext {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioContext"
  }
}

#[op(inherit = BaseAudioContext)]
impl AudioContext {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<AudioContext, OpError> {
    let mut rate = 48000.0;
    let mut hint = RenderHint::Default;
    if args.length() > 0 && !args.get(0).is_null_or_undefined() {
      let init = args.get(0);
      if !init.is_object() {
        return Err(OpError::type_("AudioContext options are not an object"));
      }
      let object = v8::Local::<v8::Object>::try_from(init).map_err(|_| {
        OpError::type_("AudioContext options are not an object")
      })?;
      if let Some(value) = util::member(scope, object, "sampleRate") {
        rate = engine::check_rate(util::finite_number(scope, value)?)
          .map_err(op_fault)?;
      }
      if let Some(value) = util::member(scope, object, "renderSizeHint") {
        hint = parse_hint(scope, value)?;
      }
      if let Some(value) = util::member(scope, object, "latencyHint") {
        check_latency(scope, value)?;
      }
      if let Some(value) = util::member(scope, object, "sinkId") {
        let sink = util::rust_string(scope, value);
        if !sink.is_empty() {
          return Err(OpError::dom(
            "NotSupportedError",
            "Selecting an audio sink is not implemented",
          ));
        }
      }
    }
    let quantum = engine::quantum_for(rate, hint, true).map_err(op_fault)?;
    let id = engine::alloc();
    let (destination, destination_object) =
      make_destination(scope, id, 2, true);
    engine::register_context(
      id,
      rate,
      quantum,
      true,
      2,
      None,
      destination,
      destination_object,
    );
    Ok(AudioContext {
      base: BaseAudioContext {
        base: EventTarget::new(),
        id,
      },
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    engine::set_context_object(scope, self.base.id, object);
  }

  #[getter]
  fn base_latency(&self) -> f64 {
    engine::base_latency(self.base.id).unwrap_or(0.0)
  }

  #[getter]
  fn output_latency(&self) -> f64 {
    0.0
  }

  #[promise]
  fn suspend(&self, scope: &mut v8::PinScope) -> Js {
    promise_of(scope, |scope, resolver| {
      match engine::suspend_realtime(scope, self.base.id) {
        Ok(()) => resolve_undefined(scope, resolver),
        Err(fault) => fault.reject(scope, resolver),
      }
    })
  }

  #[promise]
  fn resume(&self, scope: &mut v8::PinScope) -> Js {
    promise_of(scope, |scope, resolver| {
      match engine::resume_realtime(scope, self.base.id) {
        Ok(()) => resolve_undefined(scope, resolver),
        Err(fault) => fault.reject(scope, resolver),
      }
    })
  }

  #[promise]
  fn close(&self, scope: &mut v8::PinScope) -> Js {
    promise_of(scope, |scope, resolver| {
      match engine::close_context(scope, self.base.id) {
        Ok(()) => resolve_undefined(scope, resolver),
        Err(fault) => fault.reject(scope, resolver),
      }
    })
  }
}

#[repr(C)]
pub struct OfflineAudioContext {
  pub base: BaseAudioContext,
}

unsafe impl v8::cppgc::GarbageCollected for OfflineAudioContext {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"OfflineAudioContext"
  }
}

#[op(inherit = BaseAudioContext)]
impl OfflineAudioContext {
  #[constructor]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<OfflineAudioContext, OpError> {
    if args.length() == 0 {
      return Err(OpError::type_(
        "OfflineAudioContext requires options or three arguments",
      ));
    }
    let first = args.get(0);
    let (channels, length, rate, hint) = if first.is_object()
      && !first.is_null()
    {
      let object = v8::Local::<v8::Object>::try_from(first).map_err(|_| {
        OpError::type_("OfflineAudioContext options are not an object")
      })?;
      let Some(rate) = util::member(scope, object, "sampleRate") else {
        return Err(OpError::type_("sampleRate is required"));
      };
      let rate = engine::check_rate(util::finite_number(scope, rate)?)
        .map_err(op_fault)?;
      let channels = util::member(scope, object, "numberOfChannels")
        .map(|value| as_u32(scope, value))
        .transpose()?
        .unwrap_or(1);
      let channels = engine::check_channels(channels).map_err(op_fault)?;
      let length = match util::member(scope, object, "length") {
        None => None,
        Some(value) if value.is_null() => None,
        Some(value) => {
          Some(engine::check_length(as_u32(scope, value)?).map_err(op_fault)?)
        }
      };
      let hint = util::member(scope, object, "renderSizeHint")
        .map(|value| parse_hint(scope, value))
        .transpose()?
        .unwrap_or(RenderHint::Default);
      (channels, length, rate, hint)
    } else {
      if args.length() != 3 {
        return Err(OpError::type_(
          "OfflineAudioContext requires options or three arguments",
        ));
      }
      let channels =
        engine::check_channels(as_u32(scope, first)?).map_err(op_fault)?;
      let length =
        engine::check_length(as_u32(scope, args.get(1))?).map_err(op_fault)?;
      let rate = engine::check_rate(util::finite_number(scope, args.get(2))?)
        .map_err(op_fault)?;
      (channels, Some(length), rate, RenderHint::Default)
    };
    let quantum = engine::quantum_for(rate, hint, false).map_err(op_fault)?;
    let id = engine::alloc();
    let (destination, destination_object) =
      make_destination(scope, id, channels, false);
    engine::register_context(
      id,
      rate,
      quantum,
      false,
      channels,
      length,
      destination,
      destination_object,
    );
    Ok(OfflineAudioContext {
      base: BaseAudioContext {
        base: EventTarget::new(),
        id,
      },
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    engine::set_context_object(scope, self.base.id, object);
  }

  #[getter]
  fn length(&self, scope: &mut v8::PinScope) -> Js {
    match engine::context_length(self.base.id) {
      Some(Some(length)) => {
        let number: v8::Local<v8::Value> =
          v8::Number::new(scope, f64::from(length)).into();
        Js(v8::Global::new(scope, number))
      }
      _ => Js::null(scope),
    }
  }

  #[promise]
  fn start_rendering(&self, scope: &mut v8::PinScope) -> Js {
    let id = self.base.id;
    promise_of(scope, move |scope, resolver| {
      match engine::start_offline(scope, id) {
        Ok(DriveResult::Done(buffer)) => {
          let object = unsafe {
            yuzuko_core::op::wrap_instance(scope, AudioBuffer { id: buffer })
          };
          resolver.resolve(scope, object.into());
        }
        Ok(DriveResult::Suspended) => {
          engine::set_rendering(id, v8::Global::new(scope, resolver));
        }
        Err(fault) => fault.reject(scope, resolver),
      }
    })
  }

  #[promise]
  #[length(1)]
  fn suspend(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Js {
    let id = self.base.id;
    let time = args.get(0);
    promise_of(scope, move |scope, resolver| {
      match util::finite_number(scope, time).map_err(|error| {
        Fault::Type(match error {
          OpError::Type(message) | OpError::Range(message) => message,
          OpError::Dom { message, .. } => message,
          OpError::Thrown(_) => "The suspend time is invalid".to_string(),
        })
      }) {
        Ok(time) => match engine::suspend_frame(id, time) {
          Ok(frame) => {
            engine::push_suspend(id, frame, v8::Global::new(scope, resolver));
          }
          Err(fault) => fault.reject(scope, resolver),
        },
        Err(fault) => fault.reject(scope, resolver),
      }
    })
  }

  #[promise]
  fn resume(&self, scope: &mut v8::PinScope) -> Js {
    let id = self.base.id;
    promise_of(scope, move |scope, resolver| {
      match engine::resume_offline(scope, id) {
        Ok(DriveResult::Done(buffer)) => {
          let object = unsafe {
            yuzuko_core::op::wrap_instance(scope, AudioBuffer { id: buffer })
          };
          if let Some(pending) = engine::take_rendering(id) {
            let pending = v8::Local::new(scope, pending);
            pending.resolve(scope, object.into());
          }
          resolve_undefined(scope, resolver);
        }
        Ok(DriveResult::Suspended) => resolve_undefined(scope, resolver),
        Err(fault) => fault.reject(scope, resolver),
      }
    })
  }
}

#[repr(C)]
pub struct AudioWorklet {
  pub base: EventTarget,
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for AudioWorklet {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    trace_event(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioWorklet"
  }
}

struct Job {
  resolver: v8::Global<v8::PromiseResolver>,
  context: u64,
}

thread_local! {
  static JOBS: RefCell<HashMap<u64, Job>> = RefCell::new(HashMap::new());
  static WORKLETS: RefCell<HashMap<u64, v8::Global<v8::Object>>> =
    RefCell::new(HashMap::new());
}

pub fn clear() {
  JOBS.with(|jobs| jobs.borrow_mut().clear());
  WORKLETS.with(|worklets| worklets.borrow_mut().clear());
}

fn store_worklet(id: u64, object: v8::Global<v8::Object>) {
  WORKLETS.with(|worklets| {
    worklets.borrow_mut().insert(id, object);
  });
}

fn worklet_object(id: u64) -> Option<v8::Global<v8::Object>> {
  WORKLETS.with(|worklets| worklets.borrow().get(&id).cloned())
}

#[op(inherit = EventTarget)]
impl AudioWorklet {
  #[getter]
  fn port(&self, scope: &mut v8::PinScope) -> Js {
    engine::worklet_port(self.id)
      .map(|port| global_object(scope, port))
      .unwrap_or_else(|| Js::null(scope))
  }

  #[promise]
  #[length(1)]
  fn add_module(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Js {
    let url = util::rust_string(scope, args.get(0));
    let context = self.id;
    promise_of(scope, move |scope, resolver| {
      let id = engine::alloc();
      JOBS.with(|jobs| {
        jobs.borrow_mut().insert(
          id,
          Job {
            resolver: v8::Global::new(scope, resolver),
            context,
          },
        );
      });
      match call_fetch(scope, &url) {
        Ok(fetch) => {
          let data = v8::Number::new(scope, id as f64);
          let ok = v8::Function::builder(module_fetched)
            .data(data.into())
            .build(scope)
            .unwrap();
          let err = v8::Function::builder(module_failed)
            .data(data.into())
            .build(scope)
            .unwrap();
          let _ = fetch.then2(scope, ok, err);
        }
        Err(()) => fail_job(scope, id),
      }
    })
  }
}

fn call_fetch<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  url: &str,
) -> Result<v8::Local<'s, v8::Promise>, ()> {
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, "fetch").ok_or(())?;
  let value = global.get(scope, key.into()).ok_or(())?;
  let function = v8::Local::<v8::Function>::try_from(value).map_err(|_| ())?;
  let argument = v8::String::new(scope, url).ok_or(())?;
  let result = function
    .call(scope, global.into(), &[argument.into()])
    .ok_or(())?;
  v8::Local::<v8::Promise>::try_from(result).map_err(|_| ())
}

fn module_fetched(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let id = args.data().number_value(scope).unwrap_or(0.0) as u64;
  let Ok(response) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
    fail_job(scope, id);
    return;
  };
  let ok = util::member(scope, response, "ok")
    .is_some_and(|value| value.boolean_value(scope));
  if !ok {
    fail_job(scope, id);
    return;
  }
  let Some(text) = util::member(scope, response, "text") else {
    fail_job(scope, id);
    return;
  };
  let Ok(function) = v8::Local::<v8::Function>::try_from(text) else {
    fail_job(scope, id);
    return;
  };
  let Some(result) = function.call(scope, response.into(), &[]) else {
    fail_job(scope, id);
    return;
  };
  let Ok(promise) = v8::Local::<v8::Promise>::try_from(result) else {
    fail_job(scope, id);
    return;
  };
  let data = args.data();
  let Some(ok) = v8::Function::builder(module_text).data(data).build(scope)
  else {
    fail_job(scope, id);
    return;
  };
  let Some(err) = v8::Function::builder(module_failed).data(data).build(scope)
  else {
    fail_job(scope, id);
    return;
  };
  let _ = promise.then2(scope, ok, err);
}

fn module_text(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let id = args.data().number_value(scope).unwrap_or(0.0) as u64;
  let Some(job) = take_job(id) else {
    return;
  };
  let source = util::rust_string(scope, args.get(0));
  let resolver = v8::Local::new(scope, &job.resolver);
  match engine::eval_module(scope, job.context, &source) {
    Ok(()) => resolve_undefined(scope, resolver),
    Err(EvalError::Thrown(value)) => {
      let value = v8::Local::new(scope, value);
      resolver.reject(scope, value);
    }
    Err(EvalError::Fault(fault)) => fault.reject(scope, resolver),
  }
}

fn module_failed(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let id = args.data().number_value(scope).unwrap_or(0.0) as u64;
  fail_job(scope, id);
}

fn take_job(id: u64) -> Option<Job> {
  JOBS.with(|jobs| jobs.borrow_mut().remove(&id))
}

fn fail_job(scope: &mut v8::PinScope, id: u64) {
  let Some(job) = take_job(id) else {
    return;
  };
  let resolver = v8::Local::new(scope, job.resolver);
  Fault::dom("AbortError", "The worklet script could not be loaded")
    .reject(scope, resolver);
}

#[repr(C)]
pub struct AudioWorkletNode {
  pub base: AudioNode,
}

unsafe impl v8::cppgc::GarbageCollected for AudioWorkletNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioWorkletNode"
  }
}

#[op(inherit = AudioNode)]
impl AudioWorkletNode {
  #[constructor]
  #[length(2)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<AudioWorkletNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let name = util::rust_string(scope, args.get(1));
    if name.is_empty() {
      return Err(OpError::dom(
        "NotSupportedError",
        "The processor name is empty",
      ));
    }
    let options = if args.length() > 2 {
      args.get(2)
    } else {
      v8::undefined(scope).into()
    };
    engine::ensure_worklet(scope, context).map_err(op_fault)?;
    let templates =
      engine::processor_templates(context, &name).map_err(op_fault)?;
    let opts = node_options(scope, options, 2, "max", "speakers")?;
    let (inputs, outputs, output_channels) =
      worklet_counts(scope, options, &opts)?;
    let mut params = Vec::new();
    for template in &templates {
      let id = make_param(
        scope,
        template.default,
        template.default,
        template.min,
        template.max,
        template.a_rate,
      );
      params.push(WorkletParam {
        name: template.name.clone(),
        id,
        a_rate: template.a_rate,
      });
    }
    if options.is_object()
      && let Ok(object) = v8::Local::<v8::Object>::try_from(options)
      && let Some(data) = util::member(scope, object, "parameterData")
      && data.is_object()
    {
      let data = v8::Local::<v8::Object>::try_from(data)
        .map_err(|_| OpError::type_("parameterData is not an object"))?;
      for param in &params {
        if let Some(value) = util::member(scope, data, &param.name) {
          let number = util::finite_number(scope, value)?;
          engine::set_param_value(param.id, number).map_err(op_fault)?;
        }
      }
    }
    let (port, processor_port) = engine::create_port_pair(scope);
    let undefined: v8::Local<v8::Value> = v8::undefined(scope).into();
    let instance = engine::construct_processor(
      scope,
      context,
      &name,
      processor_port,
      undefined,
    )
    .map_err(op_fault)?;
    let id = engine::alloc();
    let mut node_shell = shell(context, &opts, inputs, outputs);
    node_shell.outputs = outputs;
    node_shell.inputs = inputs;
    engine::register_worklet_node(
      id,
      node_shell,
      instance,
      params,
      output_channels,
    );
    engine::set_node_port(id, port);
    let map = unsafe {
      yuzuko_core::op::wrap_instance(scope, AudioParamMap { node: id })
    };
    engine::set_parameters(id, v8::Global::new(scope, map));
    Ok(AudioWorkletNode {
      base: AudioNode {
        base: EventTarget::new(),
        id,
      },
    })
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.id, object);
  }

  #[getter]
  fn port(&self, scope: &mut v8::PinScope) -> Js {
    engine::node_port(self.base.id)
      .map(|port| global_object(scope, port))
      .unwrap_or_else(|| Js::null(scope))
  }

  #[getter]
  fn parameters(&self, scope: &mut v8::PinScope) -> Js {
    engine::parameters(self.base.id)
      .map(|map| global_object(scope, map))
      .unwrap_or_else(|| Js::null(scope))
  }
}

fn worklet_counts(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
  _opts: &NodeOpts,
) -> Result<(u32, u32, Vec<u32>), OpError> {
  let mut inputs = 1u32;
  let mut outputs = 1u32;
  if options.is_object() && !options.is_null() {
    let object = v8::Local::<v8::Object>::try_from(options).map_err(|_| {
      OpError::type_("AudioWorkletNode options are not an object")
    })?;
    if let Some(value) = util::member(scope, object, "numberOfInputs") {
      inputs = as_u32(scope, value)?;
    }
    if let Some(value) = util::member(scope, object, "numberOfOutputs") {
      outputs = as_u32(scope, value)?;
    }
    if let Some(value) = util::member(scope, object, "outputChannelCount") {
      let list = v8::Local::<v8::Array>::try_from(value)
        .map_err(|_| OpError::type_("outputChannelCount is not a sequence"))?;
      if list.length() != outputs {
        return Err(OpError::dom(
          "IndexSizeError",
          "outputChannelCount does not match numberOfOutputs",
        ));
      }
      let mut channels = Vec::new();
      for index in 0..list.length() {
        let item = list.get_index(scope, index).ok_or_else(|| {
          OpError::type_("outputChannelCount is not a sequence")
        })?;
        let count = as_u32(scope, item)?;
        if count == 0 || count > 32 {
          return Err(OpError::dom(
            "NotSupportedError",
            "The output channel count is not supported",
          ));
        }
        channels.push(count);
      }
      return Ok((inputs, outputs, channels));
    }
  }
  Ok((inputs, outputs, vec![1; outputs as usize]))
}

pub fn install(scope: &mut v8::PinScope) {
  AudioBuffer::install(scope);
  AudioParam::install(scope);
  AudioParamMap::install(scope);
  AudioNode::install(scope);
  AudioDestinationNode::install(scope);
  AudioScheduledSourceNode::install(scope);
  GainNode::install(scope);
  ConstantSourceNode::install(scope);
  OscillatorNode::install(scope);
  AudioBufferSourceNode::install(scope);
  PeriodicWave::install(scope);
  BaseAudioContext::install(scope);
  AudioContext::install(scope);
  OfflineAudioContext::install(scope);
  AudioWorklet::install(scope);
  AudioWorkletNode::install(scope);
}
