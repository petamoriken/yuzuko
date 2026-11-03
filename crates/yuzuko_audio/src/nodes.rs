//! Processing nodes and the context listener.
//!
//! `MediaElementAudioSourceNode`, `MediaStreamAudioSourceNode`,
//! `MediaStreamAudioDestinationNode`, and `ScriptProcessorNode` stay absent.
//! `"HRTF"` delays the far ear. It is not a head-related impulse response.

use yuzuko_core::op::{Js, OpError};
use yuzuko_dom::EventTarget;
use yuzuko_macros::op;

use crate::classes::{
  AudioNode, NodeOpts, as_u32, buffer_id, curve_values, make_param,
  node_options, op_fault, option_bool, option_number, param_js, remember,
  require_context, shell,
};
use crate::engine::{self, Kind};
use crate::process::{self, BiquadKind, DistanceModel, PanningModel};
use crate::util;

fn node(id: u64) -> AudioNode {
  AudioNode {
    base: EventTarget::new(),
    id,
  }
}

fn nominal_range() -> (f64, f64) {
  (-(f32::MAX as f64), f32::MAX as f64)
}

fn dictionary<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  options: v8::Local<'_, v8::Value>,
  name: &str,
) -> Result<Option<v8::Local<'s, v8::Value>>, OpError> {
  if options.is_null_or_undefined() {
    return Ok(None);
  }
  if !options.is_object() {
    return Err(OpError::type_("AudioNode options are not an object"));
  }
  let object = v8::Local::<v8::Object>::try_from(options)
    .map_err(|_| OpError::type_("AudioNode options are not an object"))?;
  Ok(util::member(scope, object, name))
}

fn member_u32(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
  name: &str,
) -> Result<Option<u32>, OpError> {
  let Some(raw) = dictionary(scope, options, name)? else {
    return Ok(None);
  };
  Ok(Some(as_u32(scope, raw)?))
}

fn member_choice(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
  name: &str,
  invalid: &str,
  parse: fn(&str) -> Option<&'static str>,
) -> Result<Option<&'static str>, OpError> {
  let Some(raw) = dictionary(scope, options, name)? else {
    return Ok(None);
  };
  parse(util::rust_string(scope, raw).as_str())
    .map(Some)
    .ok_or_else(|| OpError::type_(invalid))
}

fn count_mode(value: &str) -> Option<&'static str> {
  match value {
    "max" => Some("max"),
    "clamped-max" => Some("clamped-max"),
    "explicit" => Some("explicit"),
    _ => None,
  }
}

fn interpretation_name(value: &str) -> Option<&'static str> {
  match value {
    "speakers" => Some("speakers"),
    "discrete" => Some("discrete"),
    _ => None,
  }
}

fn enforce_locked(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
  count: u32,
  mode: &str,
  interpretation: Option<&str>,
) -> Result<(), OpError> {
  if member_u32(scope, options, "channelCount")?
    .is_some_and(|given| given != count)
  {
    return Err(OpError::dom(
      "InvalidStateError",
      "The channel count cannot be changed",
    ));
  }
  if member_choice(
    scope,
    options,
    "channelCountMode",
    "The channel count mode is invalid",
    count_mode,
  )?
  .is_some_and(|given| given != mode)
  {
    return Err(OpError::dom(
      "InvalidStateError",
      "The channel count mode cannot be changed",
    ));
  }
  if let Some(expected) = interpretation
    && member_choice(
      scope,
      options,
      "channelInterpretation",
      "The channel interpretation is invalid",
      interpretation_name,
    )?
    .is_some_and(|given| given != expected)
  {
    return Err(OpError::dom(
      "InvalidStateError",
      "The channel interpretation cannot be changed",
    ));
  }
  Ok(())
}

fn stereo_shell(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
) -> Result<NodeOpts, OpError> {
  let opts = node_options(scope, options, 2, "clamped-max", "speakers")?;
  if opts.count > 2 {
    return Err(OpError::dom(
      "NotSupportedError",
      "The channel count is not supported",
    ));
  }
  if opts.mode == "max" {
    return Err(OpError::dom(
      "NotSupportedError",
      "The channel count mode is not supported",
    ));
  }
  Ok(opts)
}

fn stereo_node(context: u64, opts: &NodeOpts) -> crate::engine::NodeShell {
  let mut node = shell(context, opts, 1, 1);
  node.max_channels = 2;
  node
}

fn spatial_param(
  scope: &mut v8::PinScope,
  context: u64,
  value: f64,
  default: f64,
) -> u64 {
  let (min, max) = nominal_range();
  make_param(scope, context, value, default, min, max, true)
}

fn node_time(id: u64) -> f64 {
  engine::node_info(id)
    .and_then(|info| engine::context_time(info.context))
    .unwrap_or(0.0)
}

fn schedule(ids: &[u64], values: &[f64], time: f64) -> Result<(), OpError> {
  if values.iter().any(|value| !value.is_finite()) {
    return Err(OpError::type_("The value is not finite"));
  }
  for (id, value) in ids.iter().zip(values) {
    engine::set_value_at_time(*id, *value, time).map_err(op_fault)?;
  }
  Ok(())
}

fn object_js(scope: &mut v8::PinScope, object: v8::Global<v8::Object>) -> Js {
  let local: v8::Local<v8::Value> = v8::Local::new(scope, object).into();
  Js(v8::Global::new(scope, local))
}

fn reject_shared_f32(view: v8::Local<v8::Float32Array>) -> Result<(), OpError> {
  if view
    .get_backing_store()
    .is_some_and(|store| store.is_shared())
  {
    return Err(OpError::type_("The Float32Array must not be shared"));
  }
  Ok(())
}

fn reject_shared_u8(view: v8::Local<v8::Uint8Array>) -> Result<(), OpError> {
  if view
    .get_backing_store()
    .is_some_and(|store| store.is_shared())
  {
    return Err(OpError::type_("The Uint8Array must not be shared"));
  }
  Ok(())
}

fn float_array<'s>(
  value: v8::Local<'s, v8::Value>,
  message: &str,
) -> Result<v8::Local<'s, v8::Float32Array>, OpError> {
  let view = v8::Local::<v8::Float32Array>::try_from(value)
    .map_err(|_| OpError::type_(message))?;
  reject_shared_f32(view)?;
  Ok(view)
}

fn byte_array<'s>(
  value: v8::Local<'s, v8::Value>,
  message: &str,
) -> Result<v8::Local<'s, v8::Uint8Array>, OpError> {
  let view = v8::Local::<v8::Uint8Array>::try_from(value)
    .map_err(|_| OpError::type_(message))?;
  reject_shared_u8(view)?;
  Ok(view)
}

fn port_count(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
  name: &str,
  override_count: Option<u32>,
  message: &'static str,
) -> Result<u32, OpError> {
  let count = if let Some(count) = override_count {
    count
  } else {
    member_u32(scope, options, name)?.unwrap_or(6)
  };
  if !(1..=32).contains(&count) {
    return Err(OpError::dom("IndexSizeError", message));
  }
  Ok(count)
}

#[repr(C)]
pub struct AudioListener {
  context: u64,
  position: [u64; 3],
  forward: [u64; 3],
  up: [u64; 3],
}

unsafe impl v8::cppgc::GarbageCollected for AudioListener {
  fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AudioListener"
  }
}

pub(crate) fn install_listener(scope: &mut v8::PinScope, context: u64) {
  let position = [
    spatial_param(scope, context, 0.0, 0.0),
    spatial_param(scope, context, 0.0, 0.0),
    spatial_param(scope, context, 0.0, 0.0),
  ];
  let forward = [
    spatial_param(scope, context, 0.0, 0.0),
    spatial_param(scope, context, 0.0, 0.0),
    spatial_param(scope, context, -1.0, -1.0),
  ];
  let up = [
    spatial_param(scope, context, 0.0, 0.0),
    spatial_param(scope, context, 1.0, 1.0),
    spatial_param(scope, context, 0.0, 0.0),
  ];
  let object = unsafe {
    yuzuko_core::op::wrap_instance(
      scope,
      AudioListener {
        context,
        position,
        forward,
        up,
      },
    )
  };
  engine::store_listener(
    context,
    position,
    forward,
    up,
    v8::Global::new(scope, object),
  );
}

#[op]
impl AudioListener {
  #[getter]
  fn position_x(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.position[0])
  }

  #[getter]
  fn position_y(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.position[1])
  }

  #[getter]
  fn position_z(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.position[2])
  }

  #[getter]
  fn forward_x(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.forward[0])
  }

  #[getter]
  fn forward_y(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.forward[1])
  }

  #[getter]
  fn forward_z(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.forward[2])
  }

  #[getter]
  fn up_x(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.up[0])
  }

  #[getter]
  fn up_y(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.up[1])
  }

  #[getter]
  fn up_z(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.up[2])
  }

  fn set_position(&self, x: f64, y: f64, z: f64) -> Result<(), OpError> {
    let time = engine::context_time(self.context).unwrap_or(0.0);
    schedule(&self.position, &[x, y, z], time)
  }

  #[length(6)]
  fn set_orientation(
    &self,
    x: f64,
    y: f64,
    z: f64,
    x_up: f64,
    y_up: f64,
    z_up: f64,
  ) -> Result<(), OpError> {
    if [x, y, z, x_up, y_up, z_up]
      .iter()
      .any(|value| !value.is_finite())
    {
      return Err(OpError::type_("The value is not finite"));
    }
    let time = engine::context_time(self.context).unwrap_or(0.0);
    schedule(&self.forward, &[x, y, z], time)?;
    schedule(&self.up, &[x_up, y_up, z_up], time)
  }
}

#[repr(C)]
pub struct DelayNode {
  pub base: AudioNode,
  delay: u64,
}

unsafe impl v8::cppgc::GarbageCollected for DelayNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DelayNode"
  }
}

pub(crate) fn build_delay(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
  max_override: Option<f64>,
) -> Result<DelayNode, OpError> {
  let max = match max_override {
    Some(max) => max,
    None => option_number(scope, options, "maxDelayTime")?.unwrap_or(1.0),
  };
  if !(max > 0.0 && max < 180.0) {
    return Err(OpError::dom(
      "NotSupportedError",
      "The maximum delay is not supported",
    ));
  }
  let delay_time = if max_override.is_some() {
    0.0
  } else {
    option_number(scope, options, "delayTime")?.unwrap_or(0.0)
  };
  let opts = node_options(scope, options, 2, "max", "speakers")?;
  let rate = engine::context_rate(context).unwrap_or(48000.0);
  let quantum = f64::from(engine::context_quantum(context).unwrap_or(128));
  let frames = (max * rate).ceil().max(quantum) as usize + 2;
  let delay = make_param(scope, context, delay_time, 0.0, 0.0, max, true);
  let id = engine::alloc();
  engine::insert_node(
    id,
    shell(context, &opts, 1, 1),
    Kind::Delay { delay, frames },
  );
  Ok(DelayNode {
    base: node(id),
    delay,
  })
}

#[op(inherit = AudioNode)]
impl DelayNode {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<DelayNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    build_delay(scope, context, options, None)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.id, object);
  }

  #[getter]
  fn delay_time(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.delay)
  }
}

fn biquad_kind(name: &str) -> Result<BiquadKind, OpError> {
  match name {
    "lowpass" => Ok(BiquadKind::Lowpass),
    "highpass" => Ok(BiquadKind::Highpass),
    "bandpass" => Ok(BiquadKind::Bandpass),
    "lowshelf" => Ok(BiquadKind::Lowshelf),
    "highshelf" => Ok(BiquadKind::Highshelf),
    "peaking" => Ok(BiquadKind::Peaking),
    "notch" => Ok(BiquadKind::Notch),
    "allpass" => Ok(BiquadKind::Allpass),
    _ => Err(OpError::type_("The filter type is invalid")),
  }
}

#[repr(C)]
pub struct BiquadFilterNode {
  pub base: AudioNode,
  frequency: u64,
  detune: u64,
  q: u64,
  gain: u64,
}

unsafe impl v8::cppgc::GarbageCollected for BiquadFilterNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"BiquadFilterNode"
  }
}

pub(crate) fn build_biquad(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
) -> Result<BiquadFilterNode, OpError> {
  let opts = node_options(scope, options, 2, "max", "speakers")?;
  let kind = match dictionary(scope, options, "type")? {
    Some(value) => biquad_kind(&util::rust_string(scope, value))?,
    None => BiquadKind::Lowpass,
  };
  let rate = engine::context_rate(context).unwrap_or(48000.0);
  let nyquist = rate / 2.0;
  let (min, max) = nominal_range();
  let frequency = option_number(scope, options, "frequency")?.unwrap_or(350.0);
  let detune = option_number(scope, options, "detune")?.unwrap_or(0.0);
  let q = option_number(scope, options, "Q")?.unwrap_or(1.0);
  let gain = option_number(scope, options, "gain")?.unwrap_or(0.0);
  let frequency =
    make_param(scope, context, frequency, 350.0, 0.0, nyquist, true);
  let detune = make_param(scope, context, detune, 0.0, min, max, true);
  let q = make_param(scope, context, q, 1.0, min, max, true);
  let gain = make_param(scope, context, gain, 0.0, min, max, true);
  let id = engine::alloc();
  engine::insert_node(
    id,
    shell(context, &opts, 1, 1),
    Kind::Biquad {
      filter: kind,
      frequency,
      detune,
      q,
      gain,
    },
  );
  Ok(BiquadFilterNode {
    base: node(id),
    frequency,
    detune,
    q,
    gain,
  })
}

#[op(inherit = AudioNode)]
impl BiquadFilterNode {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<BiquadFilterNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    build_biquad(scope, context, options)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.id, object);
  }

  #[getter]
  #[rename("type")]
  fn type_(&self) -> String {
    engine::biquad_type_name(self.base.id).to_string()
  }

  #[setter]
  #[rename("type")]
  fn type_(&self, value: String) -> Result<(), OpError> {
    engine::set_biquad_type(self.base.id, &value).map_err(op_fault)
  }

  #[getter]
  fn frequency(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.frequency)
  }

  #[getter]
  fn detune(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.detune)
  }

  #[getter]
  #[rename("Q")]
  fn q(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.q)
  }

  #[getter]
  fn gain(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.gain)
  }

  #[length(3)]
  fn get_frequency_response(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    write_frequency(scope, self.base.id, args, engine::biquad_response)
  }
}

fn iir_fault(error: process::IirError) -> OpError {
  match error {
    process::IirError::Length => OpError::dom(
      "NotSupportedError",
      "The coefficient list length is not supported",
    ),
    process::IirError::Feedforward => OpError::dom(
      "InvalidStateError",
      "The feedforward coefficients are all zero",
    ),
    process::IirError::Feedback => OpError::dom(
      "InvalidStateError",
      "The first feedback coefficient is zero",
    ),
  }
}

#[repr(C)]
pub struct IIRFilterNode {
  pub base: AudioNode,
}

unsafe impl v8::cppgc::GarbageCollected for IIRFilterNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"IIRFilterNode"
  }
}

fn coefficient_list(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<f32>, OpError> {
  if value.is_null_or_undefined() {
    return Err(OpError::type_("The coefficient list is required"));
  }
  util::read_f32_sequence(scope, value)
}

pub(crate) fn build_iir(
  scope: &mut v8::PinScope,
  context: u64,
  feedforward: v8::Local<v8::Value>,
  feedback: v8::Local<v8::Value>,
) -> Result<IIRFilterNode, OpError> {
  let feedforward = coefficient_list(scope, feedforward)?;
  let feedback = coefficient_list(scope, feedback)?;
  let (feedforward, feedback) =
    process::normalize_iir(feedforward, feedback).map_err(iir_fault)?;
  let opts =
    node_options(scope, v8::undefined(scope).into(), 2, "max", "speakers")?;
  let id = engine::alloc();
  engine::insert_node(
    id,
    shell(context, &opts, 1, 1),
    Kind::Iir {
      feedforward,
      feedback,
    },
  );
  Ok(IIRFilterNode { base: node(id) })
}

fn iir_from_options(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
) -> Result<IIRFilterNode, OpError> {
  if !options.is_object() || options.is_null() {
    return Err(OpError::type_("IIRFilter options are not an object"));
  }
  let Some(feedforward) = dictionary(scope, options, "feedforward")? else {
    return Err(OpError::type_("The coefficient list is required"));
  };
  let Some(feedback) = dictionary(scope, options, "feedback")? else {
    return Err(OpError::type_("The coefficient list is required"));
  };
  let feedforward = coefficient_list(scope, feedforward)?;
  let feedback = coefficient_list(scope, feedback)?;
  let (feedforward, feedback) =
    process::normalize_iir(feedforward, feedback).map_err(iir_fault)?;
  let opts = node_options(scope, options, 2, "max", "speakers")?;
  let id = engine::alloc();
  engine::insert_node(
    id,
    shell(context, &opts, 1, 1),
    Kind::Iir {
      feedforward,
      feedback,
    },
  );
  Ok(IIRFilterNode { base: node(id) })
}

#[op(inherit = AudioNode)]
impl IIRFilterNode {
  #[constructor]
  #[length(2)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<IIRFilterNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = args.get(1);
    iir_from_options(scope, context, options)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.id, object);
  }

  #[length(3)]
  fn get_frequency_response(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    write_frequency(scope, self.base.id, args, engine::iir_response)
  }
}

fn write_frequency(
  scope: &mut v8::PinScope,
  id: u64,
  args: &v8::FunctionCallbackArguments,
  respond: impl FnOnce(
    u64,
    &[f32],
  ) -> Result<(Vec<f32>, Vec<f32>), crate::engine::Fault>,
) -> Result<(), OpError> {
  let frequency =
    float_array(args.get(0), "The frequency array is not a Float32Array")?;
  let magnitude =
    float_array(args.get(1), "The frequency array is not a Float32Array")?;
  let phase =
    float_array(args.get(2), "The frequency array is not a Float32Array")?;
  if frequency.length() != magnitude.length()
    || frequency.length() != phase.length()
  {
    return Err(OpError::dom(
      "InvalidAccessError",
      "The frequency arrays have different lengths",
    ));
  }
  let probes = util::read_f32_view(scope, frequency);
  let magnitude_object = v8::Global::new(scope, magnitude);
  let phase_object = v8::Global::new(scope, phase);
  let (magnitude, phase) = respond(id, &probes).map_err(op_fault)?;
  util::write_f32_view(
    scope,
    v8::Local::new(scope, magnitude_object),
    &magnitude,
  );
  util::write_f32_view(scope, v8::Local::new(scope, phase_object), &phase);
  Ok(())
}

fn oversample_factor(mode: &str) -> Result<u8, OpError> {
  match mode {
    "none" => Ok(1),
    "2x" => Ok(2),
    "4x" => Ok(4),
    _ => Err(OpError::type_("The oversample mode is invalid")),
  }
}

fn optional_curve(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
) -> Result<Option<Vec<f32>>, OpError> {
  let Some(raw) = dictionary(scope, options, "curve")? else {
    return Ok(None);
  };
  if raw.is_null() {
    return Ok(None);
  }
  let values = curve_values(scope, raw)?;
  if values.len() < 2 {
    return Err(OpError::dom(
      "InvalidStateError",
      "The curve needs at least two values",
    ));
  }
  Ok(Some(values))
}

#[repr(C)]
pub struct WaveShaperNode {
  pub base: AudioNode,
}

unsafe impl v8::cppgc::GarbageCollected for WaveShaperNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"WaveShaperNode"
  }
}

pub(crate) fn build_wave_shaper(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
) -> Result<WaveShaperNode, OpError> {
  let opts = node_options(scope, options, 2, "max", "speakers")?;
  let oversample = match dictionary(scope, options, "oversample")? {
    Some(value) => oversample_factor(&util::rust_string(scope, value))?,
    None => 1,
  };
  let curve = optional_curve(scope, options)?;
  let id = engine::alloc();
  engine::insert_node(
    id,
    shell(context, &opts, 1, 1),
    Kind::WaveShaper { oversample },
  );
  if let Some(curve) = curve {
    engine::set_wave_curve(id, Some(curve)).map_err(op_fault)?;
  }
  Ok(WaveShaperNode { base: node(id) })
}

#[op(inherit = AudioNode)]
impl WaveShaperNode {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<WaveShaperNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    build_wave_shaper(scope, context, options)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.id, object);
  }

  #[getter]
  fn curve(&self, scope: &mut v8::PinScope) -> Js {
    match engine::wave_curve(self.base.id) {
      Some(values) => {
        let array = util::f32_array(scope, &values);
        let value: v8::Local<v8::Value> = array.into();
        Js(v8::Global::new(scope, value))
      }
      None => Js::null(scope),
    }
  }

  #[setter]
  fn curve(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    if value.is_null() {
      return engine::set_wave_curve(self.base.id, None).map_err(op_fault);
    }
    let values = curve_values(scope, value)?;
    engine::set_wave_curve(self.base.id, Some(values)).map_err(op_fault)
  }

  #[getter]
  fn oversample(&self) -> String {
    engine::oversample_name(self.base.id).to_string()
  }

  #[setter]
  fn oversample(&self, value: String) -> Result<(), OpError> {
    engine::set_oversample(self.base.id, &value).map_err(op_fault)
  }
}

#[repr(C)]
pub struct StereoPannerNode {
  pub base: AudioNode,
  pan: u64,
}

unsafe impl v8::cppgc::GarbageCollected for StereoPannerNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"StereoPannerNode"
  }
}

pub(crate) fn build_stereo_panner(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
) -> Result<StereoPannerNode, OpError> {
  let opts = stereo_shell(scope, options)?;
  let pan = option_number(scope, options, "pan")?.unwrap_or(0.0);
  let pan = make_param(scope, context, pan, 0.0, -1.0, 1.0, true);
  let id = engine::alloc();
  engine::insert_node(
    id,
    stereo_node(context, &opts),
    Kind::StereoPanner { pan },
  );
  Ok(StereoPannerNode {
    base: node(id),
    pan,
  })
}

#[op(inherit = AudioNode)]
impl StereoPannerNode {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<StereoPannerNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    build_stereo_panner(scope, context, options)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.id, object);
  }

  #[getter]
  fn pan(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.pan)
  }
}

fn check_panning(model: &str) -> Result<PanningModel, OpError> {
  match model {
    "equalpower" => Ok(PanningModel::EqualPower),
    "HRTF" => Ok(PanningModel::Hrtf),
    _ => Err(OpError::type_("The panning model is invalid")),
  }
}

fn distance_model(model: &str) -> Result<DistanceModel, OpError> {
  match model {
    "linear" => Ok(DistanceModel::Linear),
    "inverse" => Ok(DistanceModel::Inverse),
    "exponential" => Ok(DistanceModel::Exponential),
    _ => Err(OpError::type_("The distance model is invalid")),
  }
}

fn positive_distance(value: f64) -> Result<f64, OpError> {
  if value <= 0.0 {
    return Err(OpError::range("The maximum distance is not positive"));
  }
  Ok(value)
}

#[repr(C)]
pub struct PannerNode {
  pub base: AudioNode,
  position: [u64; 3],
  orientation: [u64; 3],
}

unsafe impl v8::cppgc::GarbageCollected for PannerNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"PannerNode"
  }
}

pub(crate) fn build_panner(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
) -> Result<PannerNode, OpError> {
  let model = match dictionary(scope, options, "panningModel")? {
    Some(value) => check_panning(&util::rust_string(scope, value))?,
    None => PanningModel::EqualPower,
  };
  let distance = match dictionary(scope, options, "distanceModel")? {
    Some(value) => distance_model(&util::rust_string(scope, value))?,
    None => DistanceModel::Inverse,
  };
  let ref_distance =
    option_number(scope, options, "refDistance")?.unwrap_or(1.0);
  if ref_distance < 0.0 {
    return Err(OpError::range("The reference distance is negative"));
  }
  let max_distance = positive_distance(
    option_number(scope, options, "maxDistance")?.unwrap_or(10000.0),
  )?;
  let rolloff = option_number(scope, options, "rolloffFactor")?.unwrap_or(1.0);
  if rolloff < 0.0 {
    return Err(OpError::range("The rolloff factor is negative"));
  }
  let cone_inner =
    option_number(scope, options, "coneInnerAngle")?.unwrap_or(360.0);
  let cone_outer =
    option_number(scope, options, "coneOuterAngle")?.unwrap_or(360.0);
  let cone_gain =
    option_number(scope, options, "coneOuterGain")?.unwrap_or(0.0);
  if !(0.0..=1.0).contains(&cone_gain) {
    return Err(OpError::dom(
      "InvalidStateError",
      "The cone outer gain is out of range",
    ));
  }
  let opts = stereo_shell(scope, options)?;
  let position_x = option_number(scope, options, "positionX")?.unwrap_or(0.0);
  let position_y = option_number(scope, options, "positionY")?.unwrap_or(0.0);
  let position_z = option_number(scope, options, "positionZ")?.unwrap_or(0.0);
  let orientation_x =
    option_number(scope, options, "orientationX")?.unwrap_or(1.0);
  let orientation_y =
    option_number(scope, options, "orientationY")?.unwrap_or(0.0);
  let orientation_z =
    option_number(scope, options, "orientationZ")?.unwrap_or(0.0);
  let position = [
    spatial_param(scope, context, position_x, 0.0),
    spatial_param(scope, context, position_y, 0.0),
    spatial_param(scope, context, position_z, 0.0),
  ];
  let orientation = [
    spatial_param(scope, context, orientation_x, 1.0),
    spatial_param(scope, context, orientation_y, 0.0),
    spatial_param(scope, context, orientation_z, 0.0),
  ];
  let id = engine::alloc();
  engine::insert_node(
    id,
    stereo_node(context, &opts),
    Kind::Panner {
      position,
      orientation,
      distance,
      ref_distance,
      max_distance,
      rolloff,
      cone_inner,
      cone_outer,
      cone_gain,
      model,
    },
  );
  Ok(PannerNode {
    base: node(id),
    position,
    orientation,
  })
}

#[op(inherit = AudioNode)]
impl PannerNode {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<PannerNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    build_panner(scope, context, options)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.id, object);
  }

  #[getter]
  fn panning_model(&self) -> String {
    engine::panning_model_name(self.base.id).to_string()
  }

  #[setter]
  fn panning_model(&self, value: String) -> Result<(), OpError> {
    engine::set_panning_model(self.base.id, &value).map_err(op_fault)
  }

  #[getter]
  fn position_x(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.position[0])
  }

  #[getter]
  fn position_y(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.position[1])
  }

  #[getter]
  fn position_z(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.position[2])
  }

  #[getter]
  fn orientation_x(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.orientation[0])
  }

  #[getter]
  fn orientation_y(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.orientation[1])
  }

  #[getter]
  fn orientation_z(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.orientation[2])
  }

  #[getter]
  fn distance_model(&self) -> String {
    engine::distance_model_name(self.base.id).to_string()
  }

  #[setter]
  fn distance_model(&self, value: String) -> Result<(), OpError> {
    engine::set_distance_model(self.base.id, &value).map_err(op_fault)
  }

  #[getter]
  fn ref_distance(&self) -> f64 {
    engine::ref_distance(self.base.id)
  }

  #[setter]
  fn ref_distance(&self, value: f64) -> Result<(), OpError> {
    engine::set_ref_distance(self.base.id, value).map_err(op_fault)
  }

  #[getter]
  fn max_distance(&self) -> f64 {
    engine::max_distance(self.base.id)
  }

  #[setter]
  fn max_distance(&self, value: f64) -> Result<(), OpError> {
    engine::set_max_distance(self.base.id, value).map_err(op_fault)
  }

  #[getter]
  fn rolloff_factor(&self) -> f64 {
    engine::rolloff(self.base.id)
  }

  #[setter]
  fn rolloff_factor(&self, value: f64) -> Result<(), OpError> {
    engine::set_rolloff(self.base.id, value).map_err(op_fault)
  }

  #[getter]
  fn cone_inner_angle(&self) -> f64 {
    engine::cone_inner(self.base.id)
  }

  #[setter]
  fn cone_inner_angle(&self, value: f64) -> Result<(), OpError> {
    engine::set_cone_inner(self.base.id, value).map_err(op_fault)
  }

  #[getter]
  fn cone_outer_angle(&self) -> f64 {
    engine::cone_outer(self.base.id)
  }

  #[setter]
  fn cone_outer_angle(&self, value: f64) -> Result<(), OpError> {
    engine::set_cone_outer(self.base.id, value).map_err(op_fault)
  }

  #[getter]
  fn cone_outer_gain(&self) -> f64 {
    engine::cone_gain(self.base.id)
  }

  #[setter]
  fn cone_outer_gain(&self, value: f64) -> Result<(), OpError> {
    engine::set_cone_gain(self.base.id, value).map_err(op_fault)
  }

  fn set_position(&self, x: f64, y: f64, z: f64) -> Result<(), OpError> {
    schedule(&self.position, &[x, y, z], node_time(self.base.id))
  }

  fn set_orientation(&self, x: f64, y: f64, z: f64) -> Result<(), OpError> {
    schedule(&self.orientation, &[x, y, z], node_time(self.base.id))
  }
}

fn convolver_buffer(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
) -> Result<Option<(u64, v8::Global<v8::Object>)>, OpError> {
  let Some(raw) = dictionary(scope, options, "buffer")? else {
    return Ok(None);
  };
  if raw.is_null() {
    return Ok(None);
  }
  let id = buffer_id(scope, raw)?;
  let object = v8::Local::<v8::Object>::try_from(raw)
    .map_err(|_| OpError::type_("The buffer is not an AudioBuffer"))?;
  Ok(Some((id, v8::Global::new(scope, object))))
}

fn convolver_channels(
  scope: &mut v8::PinScope,
  buffer: u64,
) -> Result<(), OpError> {
  let (_, channels) =
    engine::buffer_channels(scope, buffer).map_err(op_fault)?;
  if !matches!(channels.len(), 1 | 2 | 4) {
    return Err(OpError::dom(
      "NotSupportedError",
      "The impulse response channel count is not supported",
    ));
  }
  Ok(())
}

#[repr(C)]
pub struct ConvolverNode {
  pub base: AudioNode,
}

unsafe impl v8::cppgc::GarbageCollected for ConvolverNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ConvolverNode"
  }
}

pub(crate) fn build_convolver(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
) -> Result<ConvolverNode, OpError> {
  let opts = stereo_shell(scope, options)?;
  let normalize = !option_bool(scope, options, "disableNormalization");
  let buffer = convolver_buffer(scope, options)?;
  if let Some((buffer, _)) = &buffer {
    convolver_channels(scope, *buffer)?;
  }
  let id = engine::alloc();
  engine::insert_node(
    id,
    stereo_node(context, &opts),
    Kind::Convolver { normalize },
  );
  if let Some((buffer, object)) = buffer {
    engine::set_convolver_buffer(scope, id, Some(buffer)).map_err(op_fault)?;
    engine::remember_convolver_buffer(id, Some(object));
  }
  Ok(ConvolverNode { base: node(id) })
}

#[op(inherit = AudioNode)]
impl ConvolverNode {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<ConvolverNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    build_convolver(scope, context, options)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.id, object);
  }

  #[getter]
  fn buffer(&self, scope: &mut v8::PinScope) -> Js {
    engine::convolver_buffer_object(self.base.id)
      .map(|object| object_js(scope, object))
      .unwrap_or_else(|| Js::null(scope))
  }

  #[setter]
  fn buffer(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    if value.is_null() {
      engine::set_convolver_buffer(scope, self.base.id, None)
        .map_err(op_fault)?;
      engine::remember_convolver_buffer(self.base.id, None);
      return Ok(());
    }
    let buffer = buffer_id(scope, value)?;
    engine::set_convolver_buffer(scope, self.base.id, Some(buffer))
      .map_err(op_fault)?;
    let object = v8::Local::<v8::Object>::try_from(value)
      .map_err(|_| OpError::type_("The buffer is not an AudioBuffer"))?;
    engine::remember_convolver_buffer(
      self.base.id,
      Some(v8::Global::new(scope, object)),
    );
    Ok(())
  }

  #[getter]
  fn normalize(&self) -> bool {
    engine::convolver_normalize(self.base.id)
  }

  #[setter]
  fn normalize(&self, value: bool) -> Result<(), OpError> {
    engine::set_convolver_normalize(self.base.id, value).map_err(op_fault)
  }
}

enum Makeup {
  Automatic,
  Explicit(f64),
}

fn makeup_option(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
) -> Result<Makeup, OpError> {
  let Some(raw) = dictionary(scope, options, "makeupGain")? else {
    return Ok(Makeup::Automatic);
  };
  if raw.is_string() {
    return match util::rust_string(scope, raw).as_str() {
      "automatic" => Ok(Makeup::Automatic),
      _ => Err(OpError::type_("The makeup gain is invalid")),
    };
  }
  if raw.is_number() {
    return Ok(Makeup::Explicit(util::finite_number(scope, raw)?));
  }
  Err(OpError::type_("The makeup gain is invalid"))
}

fn ranged(
  scope: &mut v8::PinScope,
  options: v8::Local<v8::Value>,
  name: &str,
  default: f64,
  min: f64,
  max: f64,
) -> Result<f64, OpError> {
  Ok(
    option_number(scope, options, name)?
      .unwrap_or(default)
      .clamp(min, max),
  )
}

fn k_param(
  scope: &mut v8::PinScope,
  context: u64,
  value: f64,
  default: f64,
  min: f64,
  max: f64,
) -> u64 {
  let id = make_param(scope, context, value, default, min, max, false);
  engine::set_k_rate_only(id);
  id
}

#[repr(C)]
pub struct DynamicsCompressorNode {
  pub base: AudioNode,
  threshold: u64,
  knee: u64,
  ratio: u64,
  attack: u64,
  release: u64,
  makeup: u64,
}

unsafe impl v8::cppgc::GarbageCollected for DynamicsCompressorNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DynamicsCompressorNode"
  }
}

pub(crate) fn build_dynamics(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
) -> Result<DynamicsCompressorNode, OpError> {
  let opts = stereo_shell(scope, options)?;
  let threshold = ranged(scope, options, "threshold", -24.0, -100.0, 0.0)?;
  let knee = ranged(scope, options, "knee", 30.0, 0.0, 40.0)?;
  let ratio = ranged(scope, options, "ratio", 12.0, 1.0, 20.0)?;
  let attack = ranged(scope, options, "attack", 0.003, 0.0, 1.0)?;
  let release = ranged(scope, options, "release", 0.25, 0.0, 1.0)?;
  let makeup = makeup_option(scope, options)?;
  let makeup_max = process::makeup_max_db();
  let makeup_min = 20.0 * (f32::MIN_POSITIVE as f64).log10();
  let (makeup_value, automatic) = match makeup {
    Makeup::Automatic => {
      (process::automatic_makeup_db(threshold, knee, ratio), true)
    }
    Makeup::Explicit(value) => (value, false),
  };
  let threshold = k_param(scope, context, threshold, -24.0, -100.0, 0.0);
  let knee = k_param(scope, context, knee, 30.0, 0.0, 40.0);
  let ratio = k_param(scope, context, ratio, 12.0, 1.0, 20.0);
  let attack = k_param(scope, context, attack, 0.003, 0.0, 1.0);
  let release = k_param(scope, context, release, 0.25, 0.0, 1.0);
  let makeup =
    k_param(scope, context, makeup_value, 0.0, makeup_min, makeup_max);
  if automatic {
    engine::lock_param(makeup);
  }
  let id = engine::alloc();
  engine::insert_node(
    id,
    stereo_node(context, &opts),
    Kind::Dynamics {
      threshold,
      knee,
      ratio,
      attack,
      release,
      makeup,
      automatic,
    },
  );
  Ok(DynamicsCompressorNode {
    base: node(id),
    threshold,
    knee,
    ratio,
    attack,
    release,
    makeup,
  })
}

#[op(inherit = AudioNode)]
impl DynamicsCompressorNode {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<DynamicsCompressorNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    build_dynamics(scope, context, options)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.id, object);
  }

  #[getter]
  fn threshold(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.threshold)
  }

  #[getter]
  fn knee(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.knee)
  }

  #[getter]
  fn ratio(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.ratio)
  }

  #[getter]
  fn attack(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.attack)
  }

  #[getter]
  fn release(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.release)
  }

  #[getter]
  fn makeup_gain(&self, scope: &mut v8::PinScope) -> Js {
    param_js(scope, self.makeup)
  }

  #[getter]
  fn reduction(&self) -> f64 {
    f64::from(engine::compressor_reduction(self.base.id))
  }
}

fn check_fft(size: u32) -> Result<u32, OpError> {
  if !size.is_power_of_two() || !(32..=32768).contains(&size) {
    return Err(OpError::dom(
      "IndexSizeError",
      "The FFT size is not supported",
    ));
  }
  Ok(size)
}

#[repr(C)]
pub struct AnalyserNode {
  pub base: AudioNode,
}

unsafe impl v8::cppgc::GarbageCollected for AnalyserNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"AnalyserNode"
  }
}

pub(crate) fn build_analyser(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
) -> Result<AnalyserNode, OpError> {
  let opts = node_options(scope, options, 2, "max", "speakers")?;
  let fft_size =
    check_fft(member_u32(scope, options, "fftSize")?.unwrap_or(2048))?;
  let min_decibels =
    option_number(scope, options, "minDecibels")?.unwrap_or(-100.0);
  let max_decibels =
    option_number(scope, options, "maxDecibels")?.unwrap_or(-30.0);
  if min_decibels >= max_decibels {
    return Err(OpError::dom(
      "IndexSizeError",
      "The decibel range is invalid",
    ));
  }
  let smoothing =
    option_number(scope, options, "smoothingTimeConstant")?.unwrap_or(0.8);
  if !(0.0..=1.0).contains(&smoothing) {
    return Err(OpError::dom(
      "IndexSizeError",
      "The smoothing constant is out of range",
    ));
  }
  let id = engine::alloc();
  engine::insert_node(
    id,
    shell(context, &opts, 1, 1),
    Kind::Analyser {
      fft_size,
      min_decibels,
      max_decibels,
      smoothing,
    },
  );
  engine::reset_analyser(id, fft_size);
  Ok(AnalyserNode { base: node(id) })
}

#[op(inherit = AudioNode)]
impl AnalyserNode {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<AnalyserNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    build_analyser(scope, context, options)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.id, object);
  }

  #[getter]
  fn fft_size(&self) -> u32 {
    engine::fft_size(self.base.id)
  }

  #[setter]
  fn fft_size(&self, value: u32) -> Result<(), OpError> {
    engine::set_fft_size(self.base.id, value).map_err(op_fault)
  }

  #[getter]
  fn frequency_bin_count(&self) -> u32 {
    engine::fft_size(self.base.id) / 2
  }

  #[getter]
  fn min_decibels(&self) -> f64 {
    engine::min_decibels(self.base.id)
  }

  #[setter]
  fn min_decibels(&self, value: f64) -> Result<(), OpError> {
    engine::set_min_decibels(self.base.id, value).map_err(op_fault)
  }

  #[getter]
  fn max_decibels(&self) -> f64 {
    engine::max_decibels(self.base.id)
  }

  #[setter]
  fn max_decibels(&self, value: f64) -> Result<(), OpError> {
    engine::set_max_decibels(self.base.id, value).map_err(op_fault)
  }

  #[getter]
  fn smoothing_time_constant(&self) -> f64 {
    engine::smoothing(self.base.id)
  }

  #[setter]
  fn smoothing_time_constant(&self, value: f64) -> Result<(), OpError> {
    engine::set_smoothing(self.base.id, value).map_err(op_fault)
  }

  #[length(1)]
  fn get_float_time_domain_data(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let view =
      float_array(value, "The time domain array is not a Float32Array")?;
    let samples = engine::analyser_time(self.base.id);
    util::write_f32_view(scope, view, &samples);
    Ok(())
  }

  #[length(1)]
  fn get_byte_time_domain_data(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let view = byte_array(value, "The time domain array is not a Uint8Array")?;
    let bytes = engine::analyser_time(self.base.id)
      .into_iter()
      .map(process::byte_time_sample)
      .collect::<Vec<_>>();
    util::write_u8_view(scope, view, &bytes);
    Ok(())
  }

  #[length(1)]
  fn get_float_frequency_data(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let view = float_array(value, "The frequency array is not a Float32Array")?;
    let samples = engine::analyser_magnitudes(self.base.id)
      .into_iter()
      .map(process::db_from_magnitude)
      .collect::<Vec<_>>();
    util::write_f32_view(scope, view, &samples);
    Ok(())
  }

  #[length(1)]
  fn get_byte_frequency_data(
    &self,
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
  ) -> Result<(), OpError> {
    let view = byte_array(value, "The frequency array is not a Uint8Array")?;
    let min = engine::min_decibels(self.base.id);
    let max = engine::max_decibels(self.base.id);
    let bytes = engine::analyser_magnitudes(self.base.id)
      .into_iter()
      .map(|magnitude| {
        process::byte_frequency(process::db_from_magnitude(magnitude), min, max)
      })
      .collect::<Vec<_>>();
    util::write_u8_view(scope, view, &bytes);
    Ok(())
  }
}

#[repr(C)]
pub struct ChannelMergerNode {
  pub base: AudioNode,
}

unsafe impl v8::cppgc::GarbageCollected for ChannelMergerNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ChannelMergerNode"
  }
}

pub(crate) fn build_merger(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
  count_override: Option<u32>,
) -> Result<ChannelMergerNode, OpError> {
  let count = port_count(
    scope,
    options,
    "numberOfInputs",
    count_override,
    "The number of inputs is out of range",
  )?;
  enforce_locked(scope, options, 1, "explicit", None)?;
  let opts = node_options(scope, options, 1, "explicit", "speakers")?;
  let id = engine::alloc();
  engine::insert_node(id, shell(context, &opts, count, 1), Kind::Merger);
  Ok(ChannelMergerNode { base: node(id) })
}

#[op(inherit = AudioNode)]
impl ChannelMergerNode {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<ChannelMergerNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    build_merger(scope, context, options, None)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.id, object);
  }
}

#[repr(C)]
pub struct ChannelSplitterNode {
  pub base: AudioNode,
}

unsafe impl v8::cppgc::GarbageCollected for ChannelSplitterNode {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    v8::cppgc::GarbageCollected::trace(&self.base, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ChannelSplitterNode"
  }
}

pub(crate) fn build_splitter(
  scope: &mut v8::PinScope,
  context: u64,
  options: v8::Local<v8::Value>,
  count_override: Option<u32>,
) -> Result<ChannelSplitterNode, OpError> {
  let count = port_count(
    scope,
    options,
    "numberOfOutputs",
    count_override,
    "The number of outputs is out of range",
  )?;
  enforce_locked(scope, options, count, "explicit", Some("discrete"))?;
  let opts = node_options(scope, options, count, "explicit", "discrete")?;
  let id = engine::alloc();
  engine::insert_node(id, shell(context, &opts, 1, count), Kind::Splitter);
  Ok(ChannelSplitterNode { base: node(id) })
}

#[op(inherit = AudioNode)]
impl ChannelSplitterNode {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<ChannelSplitterNode, OpError> {
    let context = require_context(scope, args.get(0))?;
    let options = if args.length() > 1 {
      args.get(1)
    } else {
      v8::undefined(scope).into()
    };
    build_splitter(scope, context, options, None)
  }

  #[construct_hook]
  fn finish(&self, scope: &mut v8::PinScope, object: v8::Local<v8::Object>) {
    remember(scope, self.base.id, object);
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  AudioListener::install(scope);
  DelayNode::install(scope);
  BiquadFilterNode::install(scope);
  IIRFilterNode::install(scope);
  WaveShaperNode::install(scope);
  StereoPannerNode::install(scope);
  PannerNode::install(scope);
  ConvolverNode::install(scope);
  DynamicsCompressorNode::install(scope);
  AnalyserNode::install(scope);
  ChannelMergerNode::install(scope);
  ChannelSplitterNode::install(scope);
}
