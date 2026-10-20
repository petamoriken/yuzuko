//! Audio graph. One quantum in, one block out. No OS audio device.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use yuzuko_core::op::{Js, OpError, OpException};

use crate::util::{self, f32_array, f32_to_bytes};

const MAX_CHANNELS: u32 = 32;
const MAX_FRAMES: u32 = 20_000_000;
const REALTIME_QUANTA: u32 = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RunState {
  Suspended,
  Running,
  Closed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChannelMode {
  Max,
  ClampedMax,
  Explicit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Interpretation {
  Speakers,
  Discrete,
}

#[derive(Clone, Copy)]
enum OverMax {
  NotSupported,
  IndexSize,
}

pub enum Fault {
  Type(String),
  Range(String),
  Dom(&'static str, String),
}

impl Fault {
  pub(crate) fn type_(message: &str) -> Self {
    Self::Type(message.to_string())
  }

  pub(crate) fn range(message: &str) -> Self {
    Self::Range(message.to_string())
  }

  pub(crate) fn dom(name: &'static str, message: &str) -> Self {
    Self::Dom(name, message.to_string())
  }

  pub fn op(self) -> OpError {
    match self {
      Self::Type(message) => OpError::type_(message),
      Self::Range(message) => OpError::range(message),
      Self::Dom(name, message) => OpError::dom(name, message),
    }
  }

  pub fn reject(
    self,
    scope: &mut v8::PinScope,
    resolver: v8::Local<v8::PromiseResolver>,
  ) {
    let error = match self {
      Self::Type(message) => {
        let text = v8::String::new(scope, &message).unwrap();
        v8::Exception::type_error(scope, text)
      }
      Self::Range(message) => {
        let text = v8::String::new(scope, &message).unwrap();
        v8::Exception::range_error(scope, text)
      }
      Self::Dom(name, message) => util::dom_exception(scope, name, &message),
    };
    resolver.reject(scope, error);
  }
}

#[derive(Clone)]
pub struct Play {
  started: bool,
  start_frame: u64,
  stop_frame: Option<u64>,
  position: f64,
  played: f64,
  duration: Option<f64>,
}

impl Play {
  pub fn idle() -> Self {
    Self {
      started: false,
      start_frame: 0,
      stop_frame: None,
      position: 0.0,
      played: 0.0,
      duration: None,
    }
  }
}

#[derive(Clone)]
pub enum Wave {
  Sine,
  Square,
  Sawtooth,
  Triangle,
  Periodic { real: Vec<f32>, imag: Vec<f32> },
}

#[derive(Clone)]
pub struct WorkletParam {
  pub name: String,
  pub id: u64,
  pub a_rate: bool,
}

#[derive(Clone)]
pub enum Kind {
  Destination,
  Gain {
    param: u64,
  },
  Constant {
    offset: u64,
    play: Play,
  },
  Oscillator {
    frequency: u64,
    detune: u64,
    wave: Wave,
    play: Play,
  },
  Buffer {
    buffer: Option<u64>,
    acquired: Option<Vec<Vec<f32>>>,
    rate: u64,
    detune: u64,
    play: Play,
    looped: bool,
    loop_start: f64,
    loop_end: f64,
  },
  Worklet {
    instance: v8::Global<v8::Object>,
    params: Vec<WorkletParam>,
    output_channels: Vec<u32>,
    active: bool,
  },
}

struct Connection {
  dest: u64,
  output: u32,
  input: u32,
}

struct NodeRec {
  context: u64,
  inputs: u32,
  outputs: u32,
  channel_count: u32,
  max_channels: u32,
  over_max: OverMax,
  mode: ChannelMode,
  interpretation: Interpretation,
  kind: Kind,
  connections: Vec<Connection>,
  object: Option<v8::Global<v8::Object>>,
  port: Option<v8::Global<v8::Object>>,
  parameters: Option<v8::Global<v8::Object>>,
  ended: bool,
  pending_ended: bool,
}

pub struct ParamRec {
  value: f64,
  default: f64,
  min: f64,
  max: f64,
  a_rate: bool,
  object: Option<v8::Global<v8::Object>>,
}

struct ChannelData {
  samples: Vec<f32>,
  view: Option<v8::Global<v8::Float32Array>>,
}

struct BufferRec {
  rate: f64,
  channels: Vec<ChannelData>,
}

struct SuspendPoint {
  frame: u64,
  resolver: v8::Global<v8::PromiseResolver>,
}

#[derive(Clone)]
pub struct ParamTemplate {
  pub name: String,
  pub default: f64,
  pub min: f64,
  pub max: f64,
  pub a_rate: bool,
}

struct ProcessorRec {
  ctor: v8::Global<v8::Function>,
  params: Vec<ParamTemplate>,
}

struct WorkletRec {
  context: v8::Global<v8::Context>,
  processors: HashMap<String, ProcessorRec>,
  pending_port: Option<v8::Global<v8::Object>>,
  port: v8::Global<v8::Object>,
}

struct ContextRec {
  sample_rate: f64,
  quantum: u32,
  realtime: bool,
  state: RunState,
  #[allow(dead_code)]
  channels: u32,
  length: Option<u32>,
  frame: u64,
  rendered: u64,
  output: Vec<Vec<f32>>,
  destination: u64,
  destination_object: v8::Global<v8::Object>,
  suspends: Vec<SuspendPoint>,
  rendering: Option<v8::Global<v8::PromiseResolver>>,
  render_called: bool,
  object: Option<v8::Global<v8::Object>>,
  worklet: Option<WorkletRec>,
  pump_left: u32,
  pump_queued: bool,
}

struct WaveRec {
  context: u64,
  real: Vec<f32>,
  imag: Vec<f32>,
}

thread_local! {
  static NEXT: Cell<u64> = const { Cell::new(1) };
  static DRIVING: Cell<u64> = const { Cell::new(0) };
  static CONTEXTS: RefCell<HashMap<u64, ContextRec>> =
    RefCell::new(HashMap::new());
  static NODES: RefCell<HashMap<u64, NodeRec>> = RefCell::new(HashMap::new());
  static PARAMS: RefCell<HashMap<u64, ParamRec>> = RefCell::new(HashMap::new());
  static BUFFERS: RefCell<HashMap<u64, BufferRec>> =
    RefCell::new(HashMap::new());
  static WAVES: RefCell<HashMap<u64, WaveRec>> = RefCell::new(HashMap::new());
  static HANDLERS: RefCell<HashMap<(u64, &'static str), v8::Global<v8::Value>>> =
    RefCell::new(HashMap::new());
}

pub fn clear() {
  CONTEXTS.with(|slot| slot.borrow_mut().clear());
  NODES.with(|slot| slot.borrow_mut().clear());
  PARAMS.with(|slot| slot.borrow_mut().clear());
  BUFFERS.with(|slot| slot.borrow_mut().clear());
  WAVES.with(|slot| slot.borrow_mut().clear());
  HANDLERS.with(|slot| slot.borrow_mut().clear());
  DRIVING.with(|slot| slot.set(0));
}

pub fn alloc() -> u64 {
  NEXT.with(|next| {
    let id = next.get();
    next.set(id + 1);
    id
  })
}

pub enum RenderHint {
  Default,
  Hardware,
  Frames(u32),
}

pub fn quantum_for(
  rate: f64,
  hint: RenderHint,
  realtime: bool,
) -> Result<u32, Fault> {
  let max = (6.0 * rate).floor();
  let max = if max < 1.0 { 1 } else { max as u32 };
  match hint {
    RenderHint::Default => Ok(128),
    RenderHint::Hardware if !realtime => Ok(128),
    RenderHint::Hardware => Ok(hardware_quantum(max)),
    RenderHint::Frames(size) => {
      if size < 1 || size > max {
        Err(Fault::dom(
          "NotSupportedError",
          "The render quantum size is not supported",
        ))
      } else {
        Ok(size)
      }
    }
  }
}

fn hardware_quantum(max: u32) -> u32 {
  match crate::hardware::output_buffer_frames() {
    Some(frames) if (1..=max).contains(&frames) => frames,
    _ => 128,
  }
}

pub fn check_rate(rate: f64) -> Result<f64, Fault> {
  if !rate.is_finite() {
    return Err(Fault::type_("The sample rate is not finite"));
  }
  if !(3000.0..=768000.0).contains(&rate) {
    return Err(Fault::dom(
      "NotSupportedError",
      "The sample rate is not supported",
    ));
  }
  Ok(rate)
}

pub fn check_channels(channels: u32) -> Result<u32, Fault> {
  if !(1..=MAX_CHANNELS).contains(&channels) {
    return Err(Fault::dom(
      "NotSupportedError",
      "The channel count is not supported",
    ));
  }
  Ok(channels)
}

pub fn check_length(length: u32) -> Result<u32, Fault> {
  if !(1..=MAX_FRAMES).contains(&length) {
    return Err(Fault::dom(
      "NotSupportedError",
      "The buffer length is not supported",
    ));
  }
  Ok(length)
}

pub fn float_limits() -> (f64, f64) {
  (f32::MIN as f64, f32::MAX as f64)
}

fn nyquist(rate: f64) -> f64 {
  rate / 2.0
}

pub struct NodeShell {
  pub context: u64,
  pub inputs: u32,
  pub outputs: u32,
  pub channel_count: u32,
  pub max_channels: u32,
  pub index_over_max: bool,
  pub mode: &'static str,
  pub interpretation: &'static str,
}

pub fn insert_node(id: u64, shell: NodeShell, kind: Kind) {
  let mode = match shell.mode {
    "clamped-max" => ChannelMode::ClampedMax,
    "explicit" => ChannelMode::Explicit,
    _ => ChannelMode::Max,
  };
  let interpretation = if shell.interpretation == "discrete" {
    Interpretation::Discrete
  } else {
    Interpretation::Speakers
  };
  NODES.with(|nodes| {
    nodes.borrow_mut().insert(
      id,
      NodeRec {
        context: shell.context,
        inputs: shell.inputs,
        outputs: shell.outputs,
        channel_count: shell.channel_count,
        max_channels: shell.max_channels,
        over_max: if shell.index_over_max {
          OverMax::IndexSize
        } else {
          OverMax::NotSupported
        },
        mode,
        interpretation,
        kind,
        connections: Vec::new(),
        object: None,
        port: None,
        parameters: None,
        ended: false,
        pending_ended: false,
      },
    );
  });
}

pub fn set_node_object(
  scope: &mut v8::PinScope,
  id: u64,
  object: v8::Local<v8::Object>,
) {
  NODES.with(|nodes| {
    if let Some(node) = nodes.borrow_mut().get_mut(&id) {
      node.object = Some(v8::Global::new(scope, object));
    }
  });
}

pub fn set_node_port(id: u64, port: v8::Global<v8::Object>) {
  NODES.with(|nodes| {
    if let Some(node) = nodes.borrow_mut().get_mut(&id) {
      node.port = Some(port);
    }
  });
}

pub fn node_port(id: u64) -> Option<v8::Global<v8::Object>> {
  NODES.with(|nodes| nodes.borrow().get(&id).and_then(|node| node.port.clone()))
}

pub fn set_parameters(id: u64, map: v8::Global<v8::Object>) {
  NODES.with(|nodes| {
    if let Some(node) = nodes.borrow_mut().get_mut(&id) {
      node.parameters = Some(map);
    }
  });
}

pub fn parameters(id: u64) -> Option<v8::Global<v8::Object>> {
  NODES.with(|nodes| {
    nodes
      .borrow()
      .get(&id)
      .and_then(|node| node.parameters.clone())
  })
}

pub fn insert_param(
  id: u64,
  value: f64,
  default: f64,
  min: f64,
  max: f64,
  a_rate: bool,
) {
  let value = clamp_param(value, min, max);
  PARAMS.with(|params| {
    params.borrow_mut().insert(
      id,
      ParamRec {
        value,
        default,
        min,
        max,
        a_rate,
        object: None,
      },
    );
  });
}

pub fn set_param_object(
  scope: &mut v8::PinScope,
  id: u64,
  object: v8::Local<v8::Object>,
) {
  PARAMS.with(|params| {
    if let Some(param) = params.borrow_mut().get_mut(&id) {
      param.object = Some(v8::Global::new(scope, object));
    }
  });
}

pub fn param_object(id: u64) -> Option<v8::Global<v8::Object>> {
  PARAMS.with(|params| {
    params
      .borrow()
      .get(&id)
      .and_then(|param| param.object.clone())
  })
}

pub struct ParamView {
  pub value: f64,
  pub default: f64,
  pub min: f64,
  pub max: f64,
  pub a_rate: bool,
}

pub fn param_view(id: u64) -> Option<ParamView> {
  PARAMS.with(|params| {
    params.borrow().get(&id).map(|param| ParamView {
      value: param.value,
      default: param.default,
      min: param.min,
      max: param.max,
      a_rate: param.a_rate,
    })
  })
}

pub fn set_param_value(id: u64, value: f64) -> Result<(), Fault> {
  if value.is_nan() {
    return Ok(());
  }
  PARAMS.with(|params| {
    let mut params = params.borrow_mut();
    let Some(param) = params.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioParam"));
    };
    param.value = clamp_param(value, param.min, param.max);
    Ok(())
  })
}

pub fn set_param_rate(id: u64, a_rate: bool) -> Result<(), Fault> {
  PARAMS.with(|params| {
    let mut params = params.borrow_mut();
    let Some(param) = params.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioParam"));
    };
    param.a_rate = a_rate;
    Ok(())
  })
}

fn clamp_param(value: f64, min: f64, max: f64) -> f64 {
  if !value.is_finite() {
    if value.is_infinite() && value.is_sign_negative() {
      min
    } else if value.is_infinite() {
      max
    } else {
      0.0
    }
  } else {
    value.clamp(min, max)
  }
}

fn param_value(id: u64) -> f64 {
  PARAMS.with(|params| {
    params
      .borrow()
      .get(&id)
      .map(|param| param.value)
      .unwrap_or(0.0)
  })
}

pub fn insert_buffer(id: u64, rate: f64, channels: Vec<Vec<f32>>) {
  BUFFERS.with(|buffers| {
    buffers.borrow_mut().insert(
      id,
      BufferRec {
        rate,
        channels: channels
          .into_iter()
          .map(|samples| ChannelData {
            samples,
            view: None,
          })
          .collect(),
      },
    );
  });
}

pub struct BufferView {
  pub rate: f64,
  pub length: u32,
  pub channels: u32,
}

pub fn buffer_view(id: u64) -> Option<BufferView> {
  BUFFERS.with(|buffers| {
    buffers.borrow().get(&id).map(|buffer| BufferView {
      rate: buffer.rate,
      length: buffer
        .channels
        .first()
        .map(|channel| channel.samples.len() as u32)
        .unwrap_or(0),
      channels: buffer.channels.len() as u32,
    })
  })
}

pub fn channel_view(
  scope: &mut v8::PinScope,
  id: u64,
  channel: u32,
) -> Result<Js, Fault> {
  let samples = read_channel(scope, id, channel)?;
  let existing = BUFFERS.with(|buffers| {
    buffers.borrow().get(&id).and_then(|buffer| {
      buffer
        .channels
        .get(channel as usize)
        .and_then(|channel| channel.view.clone())
    })
  });
  if let Some(view) = existing {
    let local: v8::Local<v8::Value> = v8::Local::new(scope, view).into();
    return Ok(Js(v8::Global::new(scope, local)));
  }
  let view = f32_array(scope, &samples);
  BUFFERS.with(|buffers| {
    if let Some(buffer) = buffers.borrow_mut().get_mut(&id)
      && let Some(channel) = buffer.channels.get_mut(channel as usize)
    {
      channel.view = Some(v8::Global::new(scope, view));
    }
  });
  let local: v8::Local<v8::Value> = view.into();
  Ok(Js(v8::Global::new(scope, local)))
}

pub fn copy_from_channel(
  scope: &mut v8::PinScope,
  id: u64,
  channel: u32,
  start: u32,
  destination: v8::Local<v8::Float32Array>,
) -> Result<(), Fault> {
  reject_shared(destination)?;
  let samples = read_channel(scope, id, channel)?;
  if start as usize >= samples.len() {
    return Ok(());
  }
  let count = (samples.len() - start as usize).min(destination.length());
  let start = start as usize;
  write_view_range(scope, destination, &samples[start..start + count]);
  Ok(())
}

pub fn copy_to_channel(
  scope: &mut v8::PinScope,
  id: u64,
  channel: u32,
  start: u32,
  source: v8::Local<v8::Float32Array>,
) -> Result<(), Fault> {
  reject_shared(source)?;
  let incoming = util::read_f32_view(scope, source);
  let mut samples = read_channel(scope, id, channel)?;
  if start as usize >= samples.len() {
    return Ok(());
  }
  let start_index = start as usize;
  let count = (samples.len() - start_index).min(incoming.len());
  samples[start_index..start_index + count].copy_from_slice(&incoming[..count]);
  write_channel(scope, id, channel, &samples)
}

fn reject_shared(view: v8::Local<v8::Float32Array>) -> Result<(), Fault> {
  if view
    .get_backing_store()
    .is_some_and(|store| store.is_shared())
  {
    return Err(Fault::type_("The Float32Array must not be shared"));
  }
  Ok(())
}

fn write_channel(
  scope: &mut v8::PinScope,
  id: u64,
  channel: u32,
  samples: &[f32],
) -> Result<(), Fault> {
  let view = BUFFERS.with(|buffers| {
    let mut buffers = buffers.borrow_mut();
    let Some(buffer) = buffers.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioBuffer"));
    };
    let Some(stored) = buffer.channels.get_mut(channel as usize) else {
      return Err(Fault::dom(
        "IndexSizeError",
        "The channel index is out of range",
      ));
    };
    stored.samples.clear();
    stored.samples.extend_from_slice(samples);
    Ok(stored.view.clone())
  })?;
  if let Some(view) = view {
    let view = v8::Local::new(scope, view);
    if view
      .buffer(scope)
      .is_some_and(|buffer| !buffer.was_detached())
    {
      write_view_range(scope, view, samples);
    }
  }
  Ok(())
}

fn write_view_range(
  scope: &mut v8::PinScope,
  view: v8::Local<v8::Float32Array>,
  samples: &[f32],
) {
  let count = samples.len().min(view.length());
  if count == 0 {
    return;
  }
  let bytes = f32_to_bytes(&samples[..count]);
  let Some(buffer) = view.buffer(scope) else {
    return;
  };
  let Some(data) = buffer.data() else {
    return;
  };
  let offset = view.byte_offset();
  unsafe {
    std::ptr::copy_nonoverlapping(
      bytes.as_ptr(),
      data.as_ptr().cast::<u8>().add(offset),
      bytes.len(),
    );
  }
}

fn read_channel(
  scope: &mut v8::PinScope,
  id: u64,
  channel: u32,
) -> Result<Vec<f32>, Fault> {
  let (view, samples) = BUFFERS.with(|buffers| {
    let buffers = buffers.borrow();
    let Some(buffer) = buffers.get(&id) else {
      return Err(Fault::type_("Invalid AudioBuffer"));
    };
    let Some(stored) = buffer.channels.get(channel as usize) else {
      return Err(Fault::dom(
        "IndexSizeError",
        "The channel index is out of range",
      ));
    };
    Ok((stored.view.clone(), stored.samples.clone()))
  })?;
  if let Some(view) = view {
    let view = v8::Local::new(scope, view);
    return Ok(util::read_f32_view(scope, view));
  }
  Ok(samples)
}

pub fn copy_wave(id: u64) -> Option<(u64, Wave)> {
  WAVES.with(|waves| {
    let waves = waves.borrow();
    let wave = waves.get(&id)?;
    Some((
      wave.context,
      Wave::Periodic {
        real: wave.real.clone(),
        imag: wave.imag.clone(),
      },
    ))
  })
}

pub fn register_wave(id: u64, context: u64, real: Vec<f32>, imag: Vec<f32>) {
  WAVES.with(|waves| {
    waves.borrow_mut().insert(
      id,
      WaveRec {
        context,
        real,
        imag,
      },
    );
  });
}

#[allow(clippy::too_many_arguments)]
pub fn register_context(
  id: u64,
  sample_rate: f64,
  quantum: u32,
  realtime: bool,
  channels: u32,
  length: Option<u32>,
  destination: u64,
  destination_object: v8::Global<v8::Object>,
) {
  let output = if realtime {
    Vec::new()
  } else {
    let frames = length.unwrap_or(0) as usize;
    vec![vec![0.0; frames]; channels as usize]
  };
  CONTEXTS.with(|contexts| {
    contexts.borrow_mut().insert(
      id,
      ContextRec {
        sample_rate,
        quantum,
        realtime,
        state: if realtime {
          RunState::Running
        } else {
          RunState::Suspended
        },
        channels,
        length,
        frame: 0,
        rendered: 0,
        output,
        destination,
        destination_object,
        suspends: Vec::new(),
        rendering: None,
        render_called: false,
        object: None,
        worklet: None,
        pump_left: 0,
        pump_queued: false,
      },
    );
  });
}

pub fn set_context_object(
  scope: &mut v8::PinScope,
  id: u64,
  object: v8::Local<v8::Object>,
) {
  CONTEXTS.with(|contexts| {
    if let Some(context) = contexts.borrow_mut().get_mut(&id) {
      context.object = Some(v8::Global::new(scope, object));
    }
  });
}

pub fn context_rate(id: u64) -> Option<f64> {
  CONTEXTS.with(|contexts| {
    contexts
      .borrow()
      .get(&id)
      .map(|context| context.sample_rate)
  })
}

pub fn context_quantum(id: u64) -> Option<u32> {
  CONTEXTS
    .with(|contexts| contexts.borrow().get(&id).map(|context| context.quantum))
}

pub fn context_state(id: u64) -> Option<&'static str> {
  CONTEXTS.with(|contexts| {
    contexts
      .borrow()
      .get(&id)
      .map(|context| match context.state {
        RunState::Suspended => "suspended",
        RunState::Running => "running",
        RunState::Closed => "closed",
      })
  })
}

pub fn context_time(id: u64) -> Option<f64> {
  CONTEXTS.with(|contexts| {
    contexts
      .borrow()
      .get(&id)
      .map(|context| context.frame as f64 / context.sample_rate)
  })
}

pub fn context_length(id: u64) -> Option<Option<u32>> {
  CONTEXTS
    .with(|contexts| contexts.borrow().get(&id).map(|context| context.length))
}

pub fn base_latency(id: u64) -> Option<f64> {
  CONTEXTS.with(|contexts| {
    contexts
      .borrow()
      .get(&id)
      .map(|context| f64::from(context.quantum) / context.sample_rate)
  })
}

pub fn destination_object(id: u64) -> Option<v8::Global<v8::Object>> {
  CONTEXTS.with(|contexts| {
    contexts
      .borrow()
      .get(&id)
      .map(|context| context.destination_object.clone())
  })
}

pub struct NodeInfo {
  pub inputs: u32,
  pub outputs: u32,
  pub channel_count: u32,
  pub max_channels: u32,
  pub mode: &'static str,
  pub interpretation: &'static str,
  pub context: u64,
}

pub fn node_info(id: u64) -> Option<NodeInfo> {
  NODES.with(|nodes| {
    nodes.borrow().get(&id).map(|node| NodeInfo {
      inputs: node.inputs,
      outputs: node.outputs,
      channel_count: node.channel_count,
      max_channels: node.max_channels,
      mode: match node.mode {
        ChannelMode::Max => "max",
        ChannelMode::ClampedMax => "clamped-max",
        ChannelMode::Explicit => "explicit",
      },
      interpretation: match node.interpretation {
        Interpretation::Speakers => "speakers",
        Interpretation::Discrete => "discrete",
      },
      context: node.context,
    })
  })
}

pub fn set_channel_count(id: u64, count: u32) -> Result<(), Fault> {
  NODES.with(|nodes| {
    let mut nodes = nodes.borrow_mut();
    let Some(node) = nodes.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioNode"));
    };
    if count == 0 || count > MAX_CHANNELS {
      return Err(Fault::dom(
        "NotSupportedError",
        "The channel count is not supported",
      ));
    }
    if count > node.max_channels {
      return Err(match node.over_max {
        OverMax::IndexSize => {
          Fault::dom("IndexSizeError", "The channel count exceeds the maximum")
        }
        OverMax::NotSupported => {
          Fault::dom("NotSupportedError", "The channel count is not supported")
        }
      });
    }
    node.channel_count = count;
    Ok(())
  })
}

pub fn set_channel_mode(id: u64, mode: &str) -> Result<(), Fault> {
  let mode = match mode {
    "max" => ChannelMode::Max,
    "clamped-max" => ChannelMode::ClampedMax,
    "explicit" => ChannelMode::Explicit,
    _ => return Err(Fault::type_("The channel count mode is invalid")),
  };
  NODES.with(|nodes| {
    let mut nodes = nodes.borrow_mut();
    let Some(node) = nodes.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioNode"));
    };
    node.mode = mode;
    Ok(())
  })
}

pub fn set_interpretation(id: u64, value: &str) -> Result<(), Fault> {
  let interpretation = match value {
    "speakers" => Interpretation::Speakers,
    "discrete" => Interpretation::Discrete,
    _ => {
      return Err(Fault::type_("The channel interpretation is invalid"));
    }
  };
  NODES.with(|nodes| {
    let mut nodes = nodes.borrow_mut();
    let Some(node) = nodes.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioNode"));
    };
    node.interpretation = interpretation;
    Ok(())
  })
}

pub fn connect_node(
  source: u64,
  dest: u64,
  output: u32,
  input: u32,
) -> Result<(), Fault> {
  let (source_ctx, outputs) = NODES.with(|nodes| {
    let nodes = nodes.borrow();
    let Some(node) = nodes.get(&source) else {
      return Err(Fault::type_("Invalid AudioNode"));
    };
    Ok((node.context, node.outputs))
  })?;
  let (dest_ctx, inputs) = NODES.with(|nodes| {
    let nodes = nodes.borrow();
    let Some(node) = nodes.get(&dest) else {
      return Err(Fault::type_("The destination is not an AudioNode"));
    };
    Ok((node.context, node.inputs))
  })?;
  if source_ctx != dest_ctx {
    return Err(Fault::dom(
      "InvalidAccessError",
      "The nodes belong to different contexts",
    ));
  }
  if output >= outputs {
    return Err(Fault::dom(
      "IndexSizeError",
      "The output index is out of range",
    ));
  }
  if input >= inputs {
    return Err(Fault::dom(
      "IndexSizeError",
      "The input index is out of range",
    ));
  }
  NODES.with(|nodes| {
    let mut nodes = nodes.borrow_mut();
    let Some(node) = nodes.get_mut(&source) else {
      return;
    };
    let duplicate = node.connections.iter().any(|connection| {
      connection.dest == dest
        && connection.output == output
        && connection.input == input
    });
    if !duplicate {
      node.connections.push(Connection {
        dest,
        output,
        input,
      });
    }
  });
  Ok(())
}

pub fn disconnect_all(id: u64) -> Result<(), Fault> {
  NODES.with(|nodes| {
    let mut nodes = nodes.borrow_mut();
    let Some(node) = nodes.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioNode"));
    };
    node.connections.clear();
    Ok(())
  })
}

pub fn disconnect_output(id: u64, output: u32) -> Result<(), Fault> {
  NODES.with(|nodes| {
    let mut nodes = nodes.borrow_mut();
    let Some(node) = nodes.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioNode"));
    };
    if output >= node.outputs {
      return Err(Fault::dom(
        "IndexSizeError",
        "The output index is out of range",
      ));
    }
    node
      .connections
      .retain(|connection| connection.output != output);
    Ok(())
  })
}

pub fn disconnect_dest(id: u64, dest: u64) -> Result<(), Fault> {
  NODES.with(|nodes| {
    let mut nodes = nodes.borrow_mut();
    let Some(node) = nodes.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioNode"));
    };
    let before = node.connections.len();
    node
      .connections
      .retain(|connection| connection.dest != dest);
    if node.connections.len() == before {
      return Err(Fault::dom(
        "InvalidAccessError",
        "The node is not connected to that destination",
      ));
    }
    Ok(())
  })
}

fn context_of(node: u64) -> Option<u64> {
  NODES.with(|nodes| nodes.borrow().get(&node).map(|node| node.context))
}

pub fn source_start(
  scope: &mut v8::PinScope,
  id: u64,
  when: f64,
  offset: f64,
  duration: Option<f64>,
) -> Result<(), Fault> {
  if when.is_sign_negative() || offset.is_sign_negative() {
    return Err(Fault::range("The time is negative"));
  }
  if let Some(duration) = duration
    && duration.is_sign_negative()
  {
    return Err(Fault::range("The duration is negative"));
  }
  let ctx = context_of(id).ok_or_else(|| Fault::type_("Invalid AudioNode"))?;
  let (rate, frame, realtime, running) = CONTEXTS.with(|contexts| {
    let contexts = contexts.borrow();
    let Some(context) = contexts.get(&ctx) else {
      return Err(Fault::type_("Invalid AudioContext"));
    };
    Ok((
      context.sample_rate,
      context.frame,
      context.realtime,
      context.state == RunState::Running,
    ))
  })?;
  let start_frame = frame_at(when, rate).max(frame);
  let (buffer_id, buffer_rate) = NODES.with(|nodes| {
    let nodes = nodes.borrow();
    let Some(node) = nodes.get(&id) else {
      return Err(Fault::type_("Invalid AudioNode"));
    };
    let Some(play) = play_ref(&node.kind) else {
      return Err(Fault::type_("The node is not an AudioScheduledSourceNode"));
    };
    if play.started {
      return Err(Fault::dom(
        "InvalidStateError",
        "The source has already been started",
      ));
    }
    let buffer_id = match &node.kind {
      Kind::Buffer { buffer, .. } => *buffer,
      _ => None,
    };
    let buffer_rate = buffer_id.and_then(|buffer| {
      BUFFERS
        .with(|buffers| buffers.borrow().get(&buffer).map(|buffer| buffer.rate))
    });
    Ok((buffer_id, buffer_rate))
  })?;
  let acquired =
    buffer_id.map(|buffer| acquire_contents(scope, buffer).unwrap_or_default());
  NODES.with(|nodes| {
    let mut nodes = nodes.borrow_mut();
    let Some(node) = nodes.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioNode"));
    };
    match &mut node.kind {
      Kind::Buffer {
        play,
        acquired: slot,
        ..
      } => {
        if play.started {
          return Err(Fault::dom(
            "InvalidStateError",
            "The source has already been started",
          ));
        }
        arm_play(play, start_frame, duration, rate);
        if let Some(buffer_rate) = buffer_rate {
          play.position = offset * buffer_rate;
        }
        if let Some(acquired) = acquired {
          *slot = Some(acquired);
        }
      }
      Kind::Constant { play, .. } | Kind::Oscillator { play, .. } => {
        if play.started {
          return Err(Fault::dom(
            "InvalidStateError",
            "The source has already been started",
          ));
        }
        arm_play(play, start_frame, duration, rate);
      }
      _ => {
        return Err(Fault::type_(
          "The node is not an AudioScheduledSourceNode",
        ));
      }
    }
    node.ended = false;
    Ok(())
  })?;
  if realtime && running {
    kick(scope, ctx);
  }
  Ok(())
}

fn arm_play(
  play: &mut Play,
  start_frame: u64,
  duration: Option<f64>,
  rate: f64,
) {
  play.started = true;
  play.start_frame = start_frame;
  play.duration = duration.map(|duration| duration * rate);
  play.played = 0.0;
}

pub fn source_stop(id: u64, when: f64) -> Result<(), Fault> {
  if when.is_sign_negative() {
    return Err(Fault::range("The time is negative"));
  }
  let ctx = context_of(id).ok_or_else(|| Fault::type_("Invalid AudioNode"))?;
  let rate =
    context_rate(ctx).ok_or_else(|| Fault::type_("Invalid AudioContext"))?;
  let frame = context_frame(ctx);
  NODES.with(|nodes| {
    let mut nodes = nodes.borrow_mut();
    let Some(node) = nodes.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioNode"));
    };
    let play = play_mut(&mut node.kind).ok_or_else(|| {
      Fault::type_("The node is not an AudioScheduledSourceNode")
    })?;
    if !play.started || play.stop_frame.is_some() {
      return Err(Fault::dom(
        "InvalidStateError",
        "The source has not been started",
      ));
    }
    play.stop_frame = Some(frame_at(when, rate).max(frame));
    Ok(())
  })
}

fn context_frame(id: u64) -> u64 {
  CONTEXTS.with(|contexts| {
    contexts
      .borrow()
      .get(&id)
      .map(|context| context.frame)
      .unwrap_or(0)
  })
}

fn play_ref(kind: &Kind) -> Option<&Play> {
  match kind {
    Kind::Constant { play, .. }
    | Kind::Oscillator { play, .. }
    | Kind::Buffer { play, .. } => Some(play),
    _ => None,
  }
}

fn play_mut(kind: &mut Kind) -> Option<&mut Play> {
  match kind {
    Kind::Constant { play, .. }
    | Kind::Oscillator { play, .. }
    | Kind::Buffer { play, .. } => Some(play),
    _ => None,
  }
}

fn frame_at(time: f64, rate: f64) -> u64 {
  let frame = (time * rate).round();
  if frame <= 0.0 { 0 } else { frame as u64 }
}

pub fn set_source_buffer(
  scope: &mut v8::PinScope,
  id: u64,
  buffer: Option<u64>,
) -> Result<(), Fault> {
  let started = NODES.with(|nodes| {
    let nodes = nodes.borrow();
    let Some(node) = nodes.get(&id) else {
      return Err(Fault::type_("Invalid AudioNode"));
    };
    match &node.kind {
      Kind::Buffer { play, .. } => Ok(play.started),
      _ => Err(Fault::type_("The node is not an AudioBufferSourceNode")),
    }
  })?;
  let acquired = if started {
    Some(match buffer {
      Some(buffer) => acquire_contents(scope, buffer).unwrap_or_default(),
      None => Vec::new(),
    })
  } else {
    None
  };
  NODES.with(|nodes| {
    let mut nodes = nodes.borrow_mut();
    let Some(node) = nodes.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioNode"));
    };
    if let Kind::Buffer {
      buffer: slot,
      play,
      acquired: stored,
      ..
    } = &mut node.kind
    {
      *slot = buffer;
      if let Some(acquired) = acquired {
        *stored = Some(acquired);
      } else {
        *stored = None;
        play.position = 0.0;
      }
      return Ok(());
    }
    Err(Fault::type_("The node is not an AudioBufferSourceNode"))
  })
}

pub fn source_buffer(id: u64) -> Option<Option<u64>> {
  NODES.with(|nodes| {
    nodes.borrow().get(&id).and_then(|node| match &node.kind {
      Kind::Buffer { buffer, .. } => Some(*buffer),
      _ => None,
    })
  })
}

pub fn set_loop(id: u64, looped: bool, start: Option<f64>, end: Option<f64>) {
  NODES.with(|nodes| {
    if let Some(node) = nodes.borrow_mut().get_mut(&id)
      && let Kind::Buffer {
        looped: slot,
        loop_start,
        loop_end,
        ..
      } = &mut node.kind
    {
      *slot = looped;
      if let Some(start) = start {
        *loop_start = start;
      }
      if let Some(end) = end {
        *loop_end = end;
      }
    }
  });
}

pub fn loop_state(id: u64) -> Option<(bool, f64, f64)> {
  NODES.with(|nodes| {
    nodes.borrow().get(&id).and_then(|node| match &node.kind {
      Kind::Buffer {
        looped,
        loop_start,
        loop_end,
        ..
      } => Some((*looped, *loop_start, *loop_end)),
      _ => None,
    })
  })
}

pub fn set_oscillator_type(id: u64, name: &str) -> Result<(), Fault> {
  let wave = match name {
    "sine" => Wave::Sine,
    "square" => Wave::Square,
    "sawtooth" => Wave::Sawtooth,
    "triangle" => Wave::Triangle,
    "custom" => {
      return Err(Fault::dom(
        "InvalidStateError",
        "A custom oscillator requires a PeriodicWave",
      ));
    }
    _ => return Err(Fault::type_("The oscillator type is invalid")),
  };
  NODES.with(|nodes| {
    let mut nodes = nodes.borrow_mut();
    let Some(node) = nodes.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioNode"));
    };
    if let Kind::Oscillator { wave: slot, .. } = &mut node.kind {
      *slot = wave;
      return Ok(());
    }
    Err(Fault::type_("The node is not an OscillatorNode"))
  })
}

pub fn oscillator_type(id: u64) -> Option<String> {
  NODES.with(|nodes| {
    nodes.borrow().get(&id).and_then(|node| match &node.kind {
      Kind::Oscillator { wave, .. } => Some(
        match wave {
          Wave::Sine => "sine",
          Wave::Square => "square",
          Wave::Sawtooth => "sawtooth",
          Wave::Triangle => "triangle",
          Wave::Periodic { .. } => "custom",
        }
        .to_string(),
      ),
      _ => None,
    })
  })
}

pub fn set_periodic_wave(node: u64, wave: u64) -> Result<(), Fault> {
  let (context, real, imag) = WAVES.with(|waves| {
    let waves = waves.borrow();
    let Some(wave) = waves.get(&wave) else {
      return Err(Fault::type_("The periodic wave is invalid"));
    };
    Ok((wave.context, wave.real.clone(), wave.imag.clone()))
  })?;
  NODES.with(|nodes| {
    let mut nodes = nodes.borrow_mut();
    let Some(record) = nodes.get_mut(&node) else {
      return Err(Fault::type_("Invalid AudioNode"));
    };
    if record.context != context {
      return Err(Fault::dom(
        "InvalidAccessError",
        "The periodic wave belongs to a different context",
      ));
    }
    if let Kind::Oscillator { wave: slot, .. } = &mut record.kind {
      *slot = Wave::Periodic { real, imag };
      return Ok(());
    }
    Err(Fault::type_("The node is not an OscillatorNode"))
  })
}

pub fn suspend_realtime(
  scope: &mut v8::PinScope,
  id: u64,
) -> Result<(), Fault> {
  transition(scope, id, |context| {
    if !context.realtime {
      return Err(Fault::dom(
        "InvalidStateError",
        "OfflineAudioContext.suspend requires a time",
      ));
    }
    if context.state == RunState::Closed {
      return Err(Fault::dom("InvalidStateError", "The context is closed"));
    }
    context.state = RunState::Suspended;
    Ok(true)
  })
}

pub fn resume_realtime(scope: &mut v8::PinScope, id: u64) -> Result<(), Fault> {
  transition(scope, id, |context| {
    if !context.realtime {
      return Err(Fault::dom(
        "InvalidStateError",
        "The offline context is not rendering",
      ));
    }
    if context.state == RunState::Closed {
      return Err(Fault::dom("InvalidStateError", "The context is closed"));
    }
    context.state = RunState::Running;
    Ok(true)
  })?;
  kick(scope, id);
  Ok(())
}

pub fn close_context(scope: &mut v8::PinScope, id: u64) -> Result<(), Fault> {
  transition(scope, id, |context| {
    if !context.realtime {
      return Err(Fault::type_("OfflineAudioContext has no close"));
    }
    if context.state == RunState::Closed {
      return Err(Fault::dom("InvalidStateError", "The context is closed"));
    }
    context.state = RunState::Closed;
    context.pump_left = 0;
    Ok(true)
  })
}

fn transition(
  scope: &mut v8::PinScope,
  id: u64,
  apply: impl FnOnce(&mut ContextRec) -> Result<bool, Fault>,
) -> Result<(), Fault> {
  let changed = CONTEXTS.with(|contexts| {
    let mut contexts = contexts.borrow_mut();
    let Some(context) = contexts.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioContext"));
    };
    apply(context)
  })?;
  if changed {
    fire_state(scope, id);
  }
  Ok(())
}

fn fire_state(scope: &mut v8::PinScope, id: u64) {
  let object = CONTEXTS.with(|contexts| {
    contexts
      .borrow()
      .get(&id)
      .and_then(|context| context.object.clone())
  });
  if let Some(object) = object {
    let object = v8::Local::new(scope, object);
    yuzuko_dom::dispatch_trusted_event(scope, object, "statechange");
  }
}

pub enum DriveResult {
  Done(u64),
  Suspended,
}

pub fn start_offline(
  scope: &mut v8::PinScope,
  id: u64,
) -> Result<DriveResult, Fault> {
  {
    let guard = CONTEXTS.with(|contexts| {
      let mut contexts = contexts.borrow_mut();
      let Some(context) = contexts.get_mut(&id) else {
        return Err(Fault::type_("Invalid AudioContext"));
      };
      if context.realtime {
        return Err(Fault::type_("AudioContext has no startRendering"));
      }
      if context.render_called || context.state == RunState::Closed {
        return Err(Fault::dom(
          "InvalidStateError",
          "Rendering has already started",
        ));
      }
      if context.length.is_none() {
        return Err(Fault::dom(
          "InvalidStateError",
          "The offline context has no length",
        ));
      }
      context.render_called = true;
      context.state = RunState::Running;
      Ok(())
    });
    guard?;
  }
  fire_state(scope, id);
  drive(scope, id)
}

pub fn resume_offline(
  scope: &mut v8::PinScope,
  id: u64,
) -> Result<DriveResult, Fault> {
  CONTEXTS.with(|contexts| {
    let mut contexts = contexts.borrow_mut();
    let Some(context) = contexts.get_mut(&id) else {
      return Err(Fault::type_("Invalid AudioContext"));
    };
    if context.realtime || !context.render_called {
      return Err(Fault::dom(
        "InvalidStateError",
        "The offline context is not suspended",
      ));
    }
    if context.state != RunState::Suspended {
      return Err(Fault::dom(
        "InvalidStateError",
        "The offline context is not suspended",
      ));
    }
    context.state = RunState::Running;
    Ok(())
  })?;
  fire_state(scope, id);
  drive(scope, id)
}

pub fn suspend_frame(id: u64, time: f64) -> Result<u64, Fault> {
  if !time.is_finite() || time.is_sign_negative() {
    return Err(Fault::dom(
      "InvalidStateError",
      "The suspend time is invalid",
    ));
  }
  CONTEXTS.with(|contexts| {
    let contexts = contexts.borrow();
    let Some(context) = contexts.get(&id) else {
      return Err(Fault::type_("Invalid AudioContext"));
    };
    if context.realtime {
      return Err(Fault::dom(
        "InvalidStateError",
        "AudioContext.suspend does not take a time",
      ));
    }
    if context.state == RunState::Closed {
      return Err(Fault::dom("InvalidStateError", "The context is closed"));
    }
    let quantum = u64::from(context.quantum.max(1));
    let raw = (time * context.sample_rate).round();
    let raw = if raw <= 0.0 { 0 } else { raw as u64 };
    let frame = ((raw as f64) / (quantum as f64)).round() as u64 * quantum;
    if frame < context.frame {
      return Err(Fault::dom(
        "InvalidStateError",
        "The suspend time is in the past",
      ));
    }
    if let Some(length) = context.length {
      let completion =
        quantum * u64::from(length.div_ceil(context.quantum.max(1)));
      if frame > completion {
        return Err(Fault::dom(
          "InvalidStateError",
          "The suspend time is past the render length",
        ));
      }
    }
    Ok(frame)
  })
}

pub fn push_suspend(
  id: u64,
  frame: u64,
  resolver: v8::Global<v8::PromiseResolver>,
) {
  CONTEXTS.with(|contexts| {
    if let Some(context) = contexts.borrow_mut().get_mut(&id) {
      context.suspends.push(SuspendPoint { frame, resolver });
      context.suspends.sort_by_key(|point| point.frame);
    }
  });
}

pub fn set_rendering(id: u64, resolver: v8::Global<v8::PromiseResolver>) {
  CONTEXTS.with(|contexts| {
    if let Some(context) = contexts.borrow_mut().get_mut(&id) {
      context.rendering = Some(resolver);
    }
  });
}

pub fn take_rendering(id: u64) -> Option<v8::Global<v8::PromiseResolver>> {
  CONTEXTS.with(|contexts| {
    contexts
      .borrow_mut()
      .get_mut(&id)
      .and_then(|context| context.rendering.take())
  })
}

fn drive(scope: &mut v8::PinScope, id: u64) -> Result<DriveResult, Fault> {
  if DRIVING.with(|driving| driving.get()) == id {
    return Ok(DriveResult::Suspended);
  }
  DRIVING.with(|driving| driving.set(id));
  let result = drive_loop(scope, id);
  DRIVING.with(|driving| driving.set(0));
  result
}

fn drive_loop(scope: &mut v8::PinScope, id: u64) -> Result<DriveResult, Fault> {
  loop {
    let (quantum, length, frame, rendered, suspend_due) = CONTEXTS
      .with(|contexts| {
        let contexts = contexts.borrow();
        let context = contexts.get(&id)?;
        let due = context
          .suspends
          .first()
          .is_some_and(|point| point.frame <= context.frame);
        Some((
          context.quantum,
          context.length.unwrap_or(0),
          context.frame,
          context.rendered,
          due,
        ))
      })
      .ok_or_else(|| Fault::type_("Invalid AudioContext"))?;
    if rendered >= u64::from(length) {
      return Ok(DriveResult::Done(finish_buffer(scope, id)?));
    }
    if suspend_due {
      suspend_now(scope, id);
      return Ok(DriveResult::Suspended);
    }
    let need = (u64::from(length) - rendered).min(u64::from(quantum)) as usize;
    let block = render_destination(scope, id, frame, quantum as usize);
    CONTEXTS.with(|contexts| {
      if let Some(context) = contexts.borrow_mut().get_mut(&id) {
        for (index, channel) in block.iter().enumerate() {
          if let Some(stored) = context.output.get_mut(index) {
            let end = (rendered as usize + need).min(stored.len());
            let count = end - rendered as usize;
            if count > 0 && channel.len() >= count {
              stored[rendered as usize..end].copy_from_slice(&channel[..count]);
            }
          }
        }
        context.rendered = rendered + need as u64;
        context.frame = frame + u64::from(quantum);
      }
    });
    publish_worklet_time(scope, id);
    fire_ended(scope);
  }
}

fn finish_buffer(scope: &mut v8::PinScope, id: u64) -> Result<u64, Fault> {
  let (rate, output) = CONTEXTS
    .with(|contexts| {
      let mut contexts = contexts.borrow_mut();
      let context = contexts.get_mut(&id)?;
      context.state = RunState::Closed;
      Some((context.sample_rate, std::mem::take(&mut context.output)))
    })
    .ok_or_else(|| Fault::type_("Invalid AudioContext"))?;
  fire_state(scope, id);
  let buffer = alloc();
  insert_buffer(buffer, rate, output);
  Ok(buffer)
}

fn suspend_now(scope: &mut v8::PinScope, id: u64) {
  let resolver = CONTEXTS.with(|contexts| {
    let mut contexts = contexts.borrow_mut();
    let context = contexts.get_mut(&id)?;
    context.state = RunState::Suspended;
    if context.suspends.is_empty() {
      None
    } else {
      Some(context.suspends.remove(0).resolver)
    }
  });
  publish_worklet_time(scope, id);
  fire_state(scope, id);
  if let Some(resolver) = resolver {
    let resolver = v8::Local::new(scope, resolver);
    let value: v8::Local<v8::Value> = v8::undefined(scope).into();
    resolver.resolve(scope, value);
  }
}

fn kick(scope: &mut v8::PinScope, id: u64) {
  let queued = CONTEXTS.with(|contexts| {
    let mut contexts = contexts.borrow_mut();
    let Some(context) = contexts.get_mut(&id) else {
      return true;
    };
    if !context.realtime || context.state != RunState::Running {
      return true;
    }
    if context.pump_queued {
      return true;
    }
    if context.pump_left == 0 {
      context.pump_left = REALTIME_QUANTA;
    }
    context.pump_queued = true;
    false
  });
  if queued {
    return;
  }
  // The current stack may still be assigning `onmessage`. Render on a task.
  let _ = scope;
  yuzuko_core::enqueue(move |scope| pump(scope, id));
}

fn pump(scope: &mut v8::PinScope, id: u64) {
  let (quantum, frame, running) = CONTEXTS
    .with(|contexts| {
      contexts.borrow().get(&id).map(|context| {
        (
          context.quantum,
          context.frame,
          context.state == RunState::Running,
        )
      })
    })
    .unwrap_or((0, 0, false));
  CONTEXTS.with(|contexts| {
    if let Some(context) = contexts.borrow_mut().get_mut(&id) {
      context.pump_queued = false;
    }
  });
  if !running || quantum == 0 {
    return;
  }
  let _block = render_destination(scope, id, frame, quantum as usize);
  CONTEXTS.with(|contexts| {
    if let Some(context) = contexts.borrow_mut().get_mut(&id) {
      context.frame = frame + u64::from(quantum);
      if context.pump_left > 0 {
        context.pump_left -= 1;
      }
    }
  });
  publish_worklet_time(scope, id);
  fire_ended(scope);
  let more = CONTEXTS.with(|contexts| {
    contexts.borrow().get(&id).is_some_and(|context| {
      context.pump_left > 0 && context.state == RunState::Running
    })
  });
  if more {
    kick(scope, id);
  }
}

fn render_destination(
  scope: &mut v8::PinScope,
  ctx: u64,
  origin: u64,
  quantum: usize,
) -> Vec<Vec<f32>> {
  let destination = CONTEXTS.with(|contexts| {
    contexts
      .borrow()
      .get(&ctx)
      .map(|context| context.destination)
  });
  let Some(destination) = destination else {
    return Vec::new();
  };
  let mut cache = HashMap::new();
  let mut stack = Vec::new();
  let mut ended = Vec::new();
  let output = pull(
    scope,
    destination,
    origin,
    quantum,
    &mut cache,
    &mut stack,
    &mut ended,
  );
  let extra = NODES.with(|nodes| {
    nodes
      .borrow()
      .iter()
      .filter_map(|(id, node)| {
        if node.context == ctx
          && scheduled(&node.kind)
          && !cache.contains_key(id)
        {
          Some(*id)
        } else {
          None
        }
      })
      .collect::<Vec<_>>()
  });
  for id in extra {
    if cache.contains_key(&id) {
      continue;
    }
    let _ = pull(
      scope, id, origin, quantum, &mut cache, &mut stack, &mut ended,
    );
  }
  output
}

fn scheduled(kind: &Kind) -> bool {
  matches!(
    kind,
    Kind::Constant { .. } | Kind::Oscillator { .. } | Kind::Buffer { .. }
  )
}

fn pull(
  scope: &mut v8::PinScope,
  id: u64,
  origin: u64,
  quantum: usize,
  cache: &mut HashMap<u64, Vec<Vec<f32>>>,
  stack: &mut Vec<u64>,
  ended: &mut Vec<u64>,
) -> Vec<Vec<f32>> {
  if let Some(hit) = cache.get(&id) {
    return hit.clone();
  }
  if stack.contains(&id) {
    return vec![vec![0.0; quantum]];
  }
  stack.push(id);
  let kind =
    NODES.with(|nodes| nodes.borrow().get(&id).map(|node| node.kind.clone()));
  let Some(kind) = kind else {
    stack.pop();
    return vec![vec![0.0; quantum]];
  };
  let output = match kind {
    Kind::Destination => {
      mix_inputs(scope, id, 0, origin, quantum, cache, stack, ended)
    }
    Kind::Gain { param } => {
      let mut mixed =
        mix_inputs(scope, id, 0, origin, quantum, cache, stack, ended);
      let gain = param_value(param) as f32;
      for channel in &mut mixed {
        for sample in channel {
          *sample *= gain;
        }
      }
      mixed
    }
    Kind::Constant { offset, play } => {
      let (block, play, stop) =
        render_constant(play, param_value(offset), origin, quantum);
      write_play(id, play);
      if stop {
        mark_pending(id);
        ended.push(id);
      }
      block
    }
    Kind::Oscillator {
      frequency,
      detune,
      wave,
      play,
    } => {
      let rate = context_of(id).and_then(context_rate).unwrap_or(48000.0);
      let computed =
        param_value(frequency) * 2f64.powf(param_value(detune) / 1200.0);
      let (block, play, stop) =
        render_oscillator(play, &wave, computed, rate, origin, quantum);
      write_play(id, play);
      if stop {
        mark_pending(id);
        ended.push(id);
      }
      block
    }
    Kind::Buffer {
      buffer,
      acquired,
      rate,
      detune,
      play,
      looped,
      loop_start,
      loop_end,
    } => {
      let context_rate =
        context_of(id).and_then(context_rate).unwrap_or(48000.0);
      let speed = param_value(rate) * 2f64.powf(param_value(detune) / 1200.0);
      let pcm = match &acquired {
        Some(channels) if channels.is_empty() => None,
        Some(channels) => {
          let buffer_rate = buffer
            .and_then(|buffer| buffer_view(buffer).map(|view| view.rate))
            .unwrap_or(0.0);
          Some((buffer_rate, channels.clone()))
        }
        None => buffer.and_then(|buffer| read_buffer(scope, buffer)),
      };
      let (block, play, stop) = render_buffer_source(
        play,
        pcm.as_ref(),
        speed,
        context_rate,
        looped,
        loop_start,
        loop_end,
        origin,
        quantum,
      );
      write_play(id, play);
      if stop {
        mark_pending(id);
        ended.push(id);
      }
      block
    }
    Kind::Worklet {
      instance,
      params,
      output_channels,
      active,
    } => {
      if !active {
        silence_outputs(&output_channels, quantum)
      } else {
        render_worklet(
          scope,
          id,
          &instance,
          &params,
          &output_channels,
          origin,
          quantum,
          cache,
          stack,
          ended,
        )
      }
    }
  };
  stack.pop();
  cache.insert(id, output.clone());
  output
}

fn write_play(id: u64, play: Play) {
  NODES.with(|nodes| {
    let mut nodes = nodes.borrow_mut();
    let Some(node) = nodes.get_mut(&id) else {
      return;
    };
    let Some(slot) = play_mut(&mut node.kind) else {
      return;
    };
    *slot = play;
  });
}

#[allow(clippy::too_many_arguments)]
fn mix_inputs(
  scope: &mut v8::PinScope,
  id: u64,
  input: u32,
  origin: u64,
  quantum: usize,
  cache: &mut HashMap<u64, Vec<Vec<f32>>>,
  stack: &mut Vec<u64>,
  ended: &mut Vec<u64>,
) -> Vec<Vec<f32>> {
  let (mode, count, interpretation) = NODES
    .with(|nodes| {
      nodes
        .borrow()
        .get(&id)
        .map(|node| (node.mode, node.channel_count, node.interpretation))
    })
    .unwrap_or((ChannelMode::Max, 1, Interpretation::Speakers));
  let sources = incoming(id, input);
  if sources.is_empty() {
    let channels = match mode {
      ChannelMode::Explicit => count as usize,
      _ => 1,
    };
    return vec![vec![0.0; quantum]; channels.max(1)];
  }
  let mut blocks = Vec::new();
  for source in sources {
    blocks.push(pull(scope, source, origin, quantum, cache, stack, ended));
  }
  let computed = computed_channels(mode, count, &blocks);
  let mut mixed = vec![vec![0.0; quantum]; computed];
  for block in blocks {
    let fitted = fit(&block, computed, interpretation, quantum);
    for (dest, source) in mixed.iter_mut().zip(fitted) {
      for (sample, value) in dest.iter_mut().zip(source) {
        *sample += value;
      }
    }
  }
  mixed
}

fn computed_channels(
  mode: ChannelMode,
  count: u32,
  blocks: &[Vec<Vec<f32>>],
) -> usize {
  let max_in = blocks
    .iter()
    .map(|block| block.len())
    .max()
    .unwrap_or(1)
    .max(1);
  match mode {
    ChannelMode::Explicit => count.max(1) as usize,
    ChannelMode::Max => max_in,
    ChannelMode::ClampedMax => max_in.min(count.max(1) as usize).max(1),
  }
}

fn fit(
  input: &[Vec<f32>],
  count: usize,
  interpretation: Interpretation,
  quantum: usize,
) -> Vec<Vec<f32>> {
  let mut output = vec![vec![0.0; quantum]; count];
  if input.is_empty() || count == 0 {
    return output;
  }
  let speakers = interpretation == Interpretation::Speakers;
  if input.len() == count {
    for (dest, source) in output.iter_mut().zip(input) {
      let n = quantum.min(source.len());
      dest[..n].copy_from_slice(&source[..n]);
    }
    return output;
  }
  if speakers && input.len() == 1 {
    for channel in &mut output {
      let n = quantum.min(input[0].len());
      channel[..n].copy_from_slice(&input[0][..n]);
    }
    return output;
  }
  if speakers && count == 1 {
    let scale = input.len() as f32;
    for (index, channel) in input.iter().enumerate() {
      let _ = index;
      for (sample, value) in output[0].iter_mut().zip(channel) {
        *sample += *value / scale;
      }
    }
    return output;
  }
  for (dest, source) in output.iter_mut().zip(input) {
    let n = quantum.min(source.len());
    dest[..n].copy_from_slice(&source[..n]);
  }
  output
}

fn incoming(dest: u64, input: u32) -> Vec<u64> {
  NODES.with(|nodes| {
    let nodes = nodes.borrow();
    let mut sources = Vec::new();
    for (id, node) in nodes.iter() {
      for connection in &node.connections {
        if connection.dest == dest && connection.input == input {
          sources.push(*id);
        }
      }
    }
    sources
  })
}

fn render_constant(
  mut play: Play,
  offset: f64,
  origin: u64,
  quantum: usize,
) -> (Vec<Vec<f32>>, Play, bool) {
  let mut channel = vec![0.0; quantum];
  let mut stop = false;
  for (index, sample) in channel.iter_mut().enumerate() {
    let frame = origin + index as u64;
    if playing(&play, frame) {
      *sample = offset as f32;
      play.played += 1.0;
    } else if stopped(&play, frame) {
      stop = true;
    }
  }
  (vec![channel], play, stop)
}

fn render_oscillator(
  mut play: Play,
  wave: &Wave,
  computed: f64,
  rate: f64,
  origin: u64,
  quantum: usize,
) -> (Vec<Vec<f32>>, Play, bool) {
  let mut channel = vec![0.0; quantum];
  let mut stop = false;
  let limit = nyquist(rate);
  for (index, sample) in channel.iter_mut().enumerate() {
    let frame = origin + index as u64;
    if !playing(&play, frame) {
      if stopped(&play, frame) {
        stop = true;
      }
      continue;
    }
    *sample = if !computed.is_finite() || computed.abs() >= limit {
      0.0
    } else {
      wave_sample(wave, computed, rate, frame)
    };
    play.played += 1.0;
  }
  (vec![channel], play, stop)
}

fn wave_sample(wave: &Wave, frequency: f64, rate: f64, frame: u64) -> f32 {
  let theta = std::f64::consts::TAU * frequency / rate * frame as f64;
  match wave {
    Wave::Sine => theta.sin() as f32,
    Wave::Square => {
      let cycle = theta.rem_euclid(std::f64::consts::TAU);
      if cycle < std::f64::consts::PI {
        1.0
      } else {
        -1.0
      }
    }
    Wave::Sawtooth => {
      let phase = (frequency * frame as f64 / rate).rem_euclid(1.0);
      (2.0 * phase - 1.0) as f32
    }
    Wave::Triangle => {
      let phase = (frequency * frame as f64 / rate).rem_euclid(1.0);
      if phase < 0.5 {
        (-1.0 + 4.0 * phase) as f32
      } else {
        (3.0 - 4.0 * phase) as f32
      }
    }
    Wave::Periodic { real, imag } => {
      let mut sample = 0.0f64;
      let harmonics = real.len().max(imag.len());
      for harmonic in 1..harmonics {
        let angle = theta * harmonic as f64;
        let real = real.get(harmonic).copied().unwrap_or(0.0) as f64;
        let imag = imag.get(harmonic).copied().unwrap_or(0.0) as f64;
        sample += real * angle.cos() + imag * angle.sin();
      }
      sample as f32
    }
  }
}

#[allow(clippy::too_many_arguments)]
fn render_buffer_source(
  mut play: Play,
  pcm: Option<&(f64, Vec<Vec<f32>>)>,
  speed: f64,
  context_rate: f64,
  looped: bool,
  loop_start: f64,
  loop_end: f64,
  origin: u64,
  quantum: usize,
) -> (Vec<Vec<f32>>, Play, bool) {
  let Some((buffer_rate, channels)) = pcm else {
    return (vec![vec![0.0; quantum]], play, false);
  };
  if channels.is_empty() {
    return (vec![vec![0.0; quantum]], play, false);
  }
  let mut output = vec![vec![0.0; quantum]; channels.len()];
  let length = channels[0].len() as f64;
  let mut stop = false;
  let step = if context_rate == 0.0 {
    0.0
  } else {
    speed * buffer_rate / context_rate
  };
  for index in 0..quantum {
    let frame = origin + index as u64;
    if !playing(&play, frame) {
      if stopped(&play, frame) || duration_done(&play) {
        stop = true;
      }
      continue;
    }
    if !looped && (play.position < 0.0 || play.position >= length) {
      stop = true;
      continue;
    }
    for (channel, data) in output.iter_mut().zip(channels.iter()) {
      channel[index] = interpolate(data, play.position);
    }
    play.position += step;
    play.played += 1.0;
    if duration_done(&play) {
      stop = true;
    }
    if looped {
      let start = (loop_start * buffer_rate).max(0.0);
      let end = if loop_end > 0.0 {
        loop_end * buffer_rate
      } else {
        length
      };
      let end = if end <= start { length } else { end };
      let span = (end - start).max(1.0);
      if play.position >= end || play.position < start {
        play.position = start + (play.position - start).rem_euclid(span);
      }
    }
  }
  (output, play, stop)
}

fn interpolate(data: &[f32], position: f64) -> f32 {
  if data.is_empty() || position < 0.0 {
    return 0.0;
  }
  let index = position.floor() as usize;
  if index >= data.len() {
    return 0.0;
  }
  let next = if index + 1 < data.len() {
    data[index + 1]
  } else {
    0.0
  };
  let fraction = (position - index as f64) as f32;
  data[index] + (next - data[index]) * fraction
}

fn playing(play: &Play, frame: u64) -> bool {
  play.started
    && frame >= play.start_frame
    && play.stop_frame.is_none_or(|stop| frame < stop)
    && !duration_done(play)
}

fn stopped(play: &Play, frame: u64) -> bool {
  play.started && play.stop_frame.is_some_and(|stop| frame >= stop)
}

fn duration_done(play: &Play) -> bool {
  play
    .duration
    .is_some_and(|duration| play.played >= duration)
}

fn acquire_contents(
  scope: &mut v8::PinScope,
  id: u64,
) -> Result<Vec<Vec<f32>>, ()> {
  let count = BUFFERS
    .with(|buffers| {
      buffers
        .borrow()
        .get(&id)
        .map(|buffer| buffer.channels.len())
    })
    .ok_or(())?;
  for index in 0..count {
    if view_is_detached(scope, id, index as u32) {
      return Err(());
    }
  }
  let mut channels = Vec::with_capacity(count);
  for index in 0..count {
    channels.push(read_channel(scope, id, index as u32).map_err(|_| ())?);
  }
  let views = BUFFERS
    .with(|buffers| {
      let mut buffers = buffers.borrow_mut();
      let buffer = buffers.get_mut(&id)?;
      let mut views = Vec::new();
      for (channel, samples) in buffer.channels.iter_mut().zip(channels.iter())
      {
        channel.samples.clone_from(samples);
        views.push(channel.view.take());
      }
      Some(views)
    })
    .ok_or(())?;
  for view in views.into_iter().flatten() {
    let view = v8::Local::new(scope, view);
    if let Some(buffer) = view.buffer(scope) {
      let _ = buffer.detach(None);
    }
  }
  Ok(channels)
}

fn view_is_detached(scope: &mut v8::PinScope, id: u64, channel: u32) -> bool {
  let view = BUFFERS.with(|buffers| {
    buffers.borrow().get(&id).and_then(|buffer| {
      buffer
        .channels
        .get(channel as usize)
        .and_then(|channel| channel.view.clone())
    })
  });
  let Some(view) = view else {
    return false;
  };
  let view = v8::Local::new(scope, view);
  view
    .buffer(scope)
    .is_none_or(|buffer| buffer.was_detached())
}

fn read_buffer(
  scope: &mut v8::PinScope,
  id: u64,
) -> Option<(f64, Vec<Vec<f32>>)> {
  let view = buffer_view(id)?;
  let mut channels = Vec::new();
  for index in 0..view.channels {
    channels.push(read_channel(scope, id, index).ok()?);
  }
  Some((view.rate, channels))
}

fn silence_outputs(channels: &[u32], quantum: usize) -> Vec<Vec<f32>> {
  let mut output = Vec::new();
  for count in channels {
    for _ in 0..*count {
      output.push(vec![0.0; quantum]);
    }
  }
  if output.is_empty() {
    output.push(vec![0.0; quantum]);
  }
  output
}

fn mark_pending(id: u64) {
  NODES.with(|nodes| {
    if let Some(node) = nodes.borrow_mut().get_mut(&id) {
      node.pending_ended = true;
    }
  });
}

fn fire_ended(scope: &mut v8::PinScope) {
  let objects = NODES.with(|nodes| {
    let mut nodes = nodes.borrow_mut();
    let mut objects = Vec::new();
    for node in nodes.values_mut() {
      if node.pending_ended && !node.ended {
        node.ended = true;
        if let Some(object) = node.object.clone() {
          objects.push(object);
        }
      }
    }
    objects
  });
  for object in objects {
    let object = v8::Local::new(scope, object);
    yuzuko_dom::dispatch_trusted_event(scope, object, "ended");
  }
}

pub fn bind_handler(
  scope: &mut v8::PinScope,
  target: &yuzuko_dom::EventTarget,
  id: u64,
  name: &'static str,
  value: v8::Local<v8::Value>,
) {
  if let Some(previous) =
    HANDLERS.with(|handlers| handlers.borrow_mut().remove(&(id, name)))
  {
    let previous = v8::Local::new(scope, previous);
    target.remove_callback(scope, name, previous, false);
  }
  if value.is_function() {
    target.add_callback(scope, name, value, false, false, false);
    HANDLERS.with(|handlers| {
      handlers
        .borrow_mut()
        .insert((id, name), v8::Global::new(scope, value));
    });
  }
}

pub fn handler(scope: &mut v8::PinScope, id: u64, name: &'static str) -> Js {
  HANDLERS.with(|handlers| {
    handlers
      .borrow()
      .get(&(id, name))
      .map(|value| Js(v8::Global::new(scope, value.clone())))
      .unwrap_or_else(|| Js::null(scope))
  })
}

// The rest of this module is the worklet scope, ports, and process().

const WORKLET_BOOTSTRAP: &str = r#"
class AudioWorkletProcessor {
  constructor() {
    const port = globalThis.__yuzukoTakePort();
    if (port === null || port === undefined) {
      throw new TypeError("Illegal constructor");
    }
    Object.defineProperty(this, "port", {
      value: port,
      writable: true,
      configurable: true,
      enumerable: true
    });
  }
  process() {
    return true;
  }
}
globalThis.AudioWorkletProcessor = AudioWorkletProcessor;
globalThis.registerProcessor = function registerProcessor(name, processorCtor) {
  let descriptors;
  if (processorCtor != null && "parameterDescriptors" in processorCtor) {
    descriptors = processorCtor.parameterDescriptors;
  }
  globalThis.__yuzukoRegister(name, processorCtor, descriptors);
};
"#;

pub fn ensure_worklet(scope: &mut v8::PinScope, id: u64) -> Result<(), Fault> {
  let exists = CONTEXTS.with(|contexts| {
    contexts
      .borrow()
      .get(&id)
      .is_some_and(|context| context.worklet.is_some())
  });
  if exists {
    publish_worklet_time(scope, id);
    return Ok(());
  }
  let token = scope.get_current_context().get_security_token(scope);
  let context = v8::Context::new(scope, Default::default());
  context.set_security_token(token);
  let (main_port, worker_port) = yuzuko_dom::entangled_ports(scope);
  let main_port = v8::Global::new(scope, main_port);
  {
    let scope = &mut v8::ContextScope::new(scope, context);
    yuzuko_core::delete_object_prototype_proto(scope);
    install_worklet_host(scope, id);
    let global = scope.get_current_context().global(scope);
    let key = v8::String::new(scope, "port").unwrap();
    global.set(scope, key.into(), worker_port.into());
    eval_classic(scope, WORKLET_BOOTSTRAP)
      .map_err(|message| Fault::type_(&message))?;
  }
  CONTEXTS.with(|contexts| {
    if let Some(record) = contexts.borrow_mut().get_mut(&id) {
      record.worklet = Some(WorkletRec {
        context: v8::Global::new(scope, context),
        processors: HashMap::new(),
        pending_port: None,
        port: main_port,
      });
    }
  });
  publish_worklet_time(scope, id);
  Ok(())
}

fn install_worklet_host(scope: &mut v8::PinScope, id: u64) {
  let global = scope.get_current_context().global(scope);
  let data = v8::Number::new(scope, id as f64);
  define_fn(scope, global, "__yuzukoRegister", host_register, data);
  define_fn(scope, global, "__yuzukoTakePort", host_take_port, data);
}

fn define_fn(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
  data: v8::Local<v8::Number>,
) {
  let Some(function) = v8::Function::builder(callback)
    .data(data.into())
    .build(scope)
  else {
    return;
  };
  let key = v8::String::new(scope, name).unwrap();
  function.set_name(key);
  object.set(scope, key.into(), function.into());
}

fn host_register(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  if let Err(fault) = register_from_js(scope, &args) {
    OpException::throw(fault.op(), scope);
  }
}

fn register_from_js(
  scope: &mut v8::PinScope,
  args: &v8::FunctionCallbackArguments,
) -> Result<(), Fault> {
  let ctx = args.data().number_value(scope).unwrap_or(0.0) as u64;
  if args.length() < 2 {
    return Err(Fault::type_(
      "registerProcessor requires a name and a constructor",
    ));
  }
  let name = util::rust_string(scope, args.get(0));
  if name.is_empty() {
    return Err(Fault::dom(
      "NotSupportedError",
      "The processor name is empty",
    ));
  }
  let ctor = v8::Local::<v8::Function>::try_from(args.get(1))
    .map_err(|_| Fault::type_("The processor is not a constructor"))?;
  let templates = parse_descriptors(scope, args.get(2))?;
  let duplicate = CONTEXTS.with(|contexts| {
    contexts.borrow().get(&ctx).is_some_and(|context| {
      context
        .worklet
        .as_ref()
        .is_some_and(|worklet| worklet.processors.contains_key(&name))
    })
  });
  if duplicate {
    return Err(Fault::dom(
      "NotSupportedError",
      "The processor name is already registered",
    ));
  }
  CONTEXTS.with(|contexts| {
    let mut contexts = contexts.borrow_mut();
    let Some(context) = contexts.get_mut(&ctx) else {
      return;
    };
    if let Some(worklet) = context.worklet.as_mut() {
      worklet.processors.insert(
        name,
        ProcessorRec {
          ctor: v8::Global::new(scope, ctor),
          params: templates,
        },
      );
    }
  });
  Ok(())
}

fn parse_descriptors(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Result<Vec<ParamTemplate>, Fault> {
  if value.is_null_or_undefined() {
    return Ok(Vec::new());
  }
  let array = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| Fault::type_("parameterDescriptors is not a sequence"))?;
  let mut templates: Vec<ParamTemplate> = Vec::new();
  let (min_default, max_default) = float_limits();
  for index in 0..array.length() {
    let item = array
      .get_index(scope, index)
      .ok_or_else(|| Fault::type_("parameterDescriptors is not a sequence"))?;
    let object = v8::Local::<v8::Object>::try_from(item)
      .map_err(|_| Fault::type_("A parameter descriptor is not an object"))?;
    let name = util::member(scope, object, "name")
      .map(|value| util::rust_string(scope, value))
      .unwrap_or_default();
    if name.is_empty() {
      return Err(Fault::dom(
        "NotSupportedError",
        "A parameter descriptor needs a name",
      ));
    }
    if templates.iter().any(|template| template.name == name) {
      return Err(Fault::dom(
        "NotSupportedError",
        "A parameter name is duplicated",
      ));
    }
    let default = match util::member(scope, object, "defaultValue") {
      Some(value) => util::finite_number(scope, value)
        .map_err(|error| Fault::type_(&error_text(error)))?,
      None => 0.0,
    };
    let min = match util::member(scope, object, "minValue") {
      Some(value) => util::finite_number(scope, value)
        .map_err(|error| Fault::type_(&error_text(error)))?,
      None => min_default,
    };
    let max = match util::member(scope, object, "maxValue") {
      Some(value) => util::finite_number(scope, value)
        .map_err(|error| Fault::type_(&error_text(error)))?,
      None => max_default,
    };
    if default < min || default > max {
      return Err(Fault::dom(
        "NotSupportedError",
        "The default value is outside minValue and maxValue",
      ));
    }
    let rate = util::member(scope, object, "automationRate")
      .map(|value| util::rust_string(scope, value))
      .unwrap_or_else(|| "a-rate".to_string());
    let a_rate = match rate.as_str() {
      "a-rate" => true,
      "k-rate" => false,
      _ => {
        return Err(Fault::type_("The automation rate is invalid"));
      }
    };
    templates.push(ParamTemplate {
      name,
      default,
      min,
      max,
      a_rate,
    });
  }
  Ok(templates)
}

fn error_text(error: OpError) -> String {
  match error {
    OpError::Type(message) | OpError::Range(message) => message,
    OpError::Dom { message, .. } => message,
    OpError::Thrown(_) => "The value is invalid".to_string(),
  }
}

fn host_take_port(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let ctx = args.data().number_value(scope).unwrap_or(0.0) as u64;
  let port = CONTEXTS.with(|contexts| {
    let mut contexts = contexts.borrow_mut();
    contexts
      .get_mut(&ctx)
      .and_then(|context| context.worklet.as_mut())
      .and_then(|worklet| worklet.pending_port.take())
  });
  if let Some(port) = port {
    ret.set(v8::Local::new(scope, port).into());
  } else {
    ret.set(v8::null(scope).into());
  }
}

pub enum EvalError {
  Fault(Fault),
  Thrown(v8::Global<v8::Value>),
}

pub fn eval_module(
  scope: &mut v8::PinScope,
  id: u64,
  source: &str,
) -> Result<(), EvalError> {
  ensure_worklet(scope, id).map_err(EvalError::Fault)?;
  let context = CONTEXTS
    .with(|contexts| {
      contexts.borrow().get(&id).and_then(|context| {
        context
          .worklet
          .as_ref()
          .map(|worklet| worklet.context.clone())
      })
    })
    .ok_or_else(|| {
      EvalError::Fault(Fault::type_("The worklet is unavailable"))
    })?;
  publish_worklet_time(scope, id);
  let context = v8::Local::new(scope, context);
  let scope = &mut v8::ContextScope::new(scope, context);
  v8::tc_scope!(let scope, scope);
  let Some(code) = v8::String::new(scope, source) else {
    return Err(EvalError::Fault(Fault::type_("The script is too large")));
  };
  let Some(script) = v8::Script::compile(scope, code, None) else {
    let exception = scope
      .exception()
      .unwrap_or_else(|| v8::undefined(scope).into());
    return Err(EvalError::Thrown(v8::Global::new(scope, exception)));
  };
  if script.run(scope).is_none() {
    let exception = scope
      .exception()
      .unwrap_or_else(|| v8::undefined(scope).into());
    return Err(EvalError::Thrown(v8::Global::new(scope, exception)));
  }
  Ok(())
}

fn eval_classic(scope: &mut v8::PinScope, source: &str) -> Result<(), String> {
  v8::tc_scope!(let scope, scope);
  let Some(code) = v8::String::new(scope, source) else {
    return Err("The script is too large".to_string());
  };
  let Some(script) = v8::Script::compile(scope, code, None) else {
    return Err(exception_message(scope, "The script failed to compile"));
  };
  if script.run(scope).is_none() {
    return Err(exception_message(scope, "The script failed"));
  }
  Ok(())
}

fn exception_message(
  scope: &mut v8::PinnedRef<v8::TryCatch<v8::HandleScope>>,
  fallback: &str,
) -> String {
  scope
    .exception()
    .and_then(|exception| exception.to_string(scope))
    .map(|text| text.to_rust_string_lossy(scope))
    .unwrap_or_else(|| fallback.to_string())
}

pub fn processor_templates(
  id: u64,
  name: &str,
) -> Result<Vec<ParamTemplate>, Fault> {
  CONTEXTS.with(|contexts| {
    let contexts = contexts.borrow();
    let Some(context) = contexts.get(&id) else {
      return Err(Fault::type_("Invalid AudioContext"));
    };
    let Some(worklet) = context.worklet.as_ref() else {
      return Err(Fault::dom(
        "InvalidStateError",
        "The processor is not registered",
      ));
    };
    worklet
      .processors
      .get(name)
      .map(|processor| processor.params.clone())
      .ok_or_else(|| {
        Fault::dom("InvalidStateError", "The processor is not registered")
      })
  })
}

pub fn construct_processor(
  scope: &mut v8::PinScope,
  ctx: u64,
  name: &str,
  port: v8::Global<v8::Object>,
  options: v8::Local<v8::Value>,
) -> Result<v8::Global<v8::Object>, Fault> {
  let (context, ctor) = CONTEXTS
    .with(|contexts| {
      let contexts = contexts.borrow();
      let record = contexts.get(&ctx)?;
      let worklet = record.worklet.as_ref()?;
      let ctor = worklet.processors.get(name)?.ctor.clone();
      Some((worklet.context.clone(), ctor))
    })
    .ok_or_else(|| {
      Fault::dom("InvalidStateError", "The processor is not registered")
    })?;
  CONTEXTS.with(|contexts| {
    if let Some(record) = contexts.borrow_mut().get_mut(&ctx)
      && let Some(worklet) = record.worklet.as_mut()
    {
      worklet.pending_port = Some(port);
    }
  });
  publish_worklet_time(scope, ctx);
  let constructed = {
    let context = v8::Local::new(scope, context);
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let ctor = v8::Local::new(scope, ctor);
    let options = if options.is_null_or_undefined() {
      v8::Object::new(scope).into()
    } else {
      options
    };
    match ctor.new_instance(scope, &[options]) {
      Some(instance) => Ok(v8::Global::new(scope, instance)),
      None => Err(exception_message(scope, "The processor constructor failed")),
    }
  };
  constructed.map_err(|message| Fault::type_(&message))
}

pub fn register_worklet_node(
  id: u64,
  shell: NodeShell,
  instance: v8::Global<v8::Object>,
  params: Vec<WorkletParam>,
  output_channels: Vec<u32>,
) {
  insert_node(
    id,
    shell,
    Kind::Worklet {
      instance,
      params,
      output_channels,
      active: true,
    },
  );
}

pub fn worklet_param_ids(node: u64) -> Vec<(String, u64)> {
  NODES.with(|nodes| {
    nodes
      .borrow()
      .get(&node)
      .and_then(|node| match &node.kind {
        Kind::Worklet { params, .. } => Some(
          params
            .iter()
            .map(|param| (param.name.clone(), param.id))
            .collect(),
        ),
        _ => None,
      })
      .unwrap_or_default()
  })
}

#[allow(clippy::too_many_arguments)]
fn render_worklet(
  scope: &mut v8::PinScope,
  id: u64,
  instance: &v8::Global<v8::Object>,
  params: &[WorkletParam],
  output_channels: &[u32],
  origin: u64,
  quantum: usize,
  cache: &mut HashMap<u64, Vec<Vec<f32>>>,
  stack: &mut Vec<u64>,
  ended: &mut Vec<u64>,
) -> Vec<Vec<f32>> {
  let ctx = context_of(id).unwrap_or(0);
  let context = CONTEXTS.with(|contexts| {
    contexts.borrow().get(&ctx).and_then(|context| {
      context
        .worklet
        .as_ref()
        .map(|worklet| worklet.context.clone())
    })
  });
  let Some(context) = context else {
    return silence_outputs(output_channels, quantum);
  };
  let input_count = node_info(id).map(|info| info.inputs).unwrap_or(1);
  let mut inputs = Vec::new();
  for input in 0..input_count {
    let sources = incoming(id, input);
    if sources.is_empty() {
      inputs.push(Vec::new());
      continue;
    }
    let mut blocks = Vec::new();
    for source in sources {
      blocks.push(pull(scope, source, origin, quantum, cache, stack, ended));
    }
    let width = blocks.iter().map(|block| block.len()).max().unwrap_or(0);
    if width == 0 {
      inputs.push(Vec::new());
      continue;
    }
    let mut mixed = vec![vec![0.0; quantum]; width];
    for block in blocks {
      for (dest, source) in mixed.iter_mut().zip(block) {
        for (sample, value) in dest.iter_mut().zip(source) {
          *sample += value;
        }
      }
    }
    inputs.push(mixed);
  }
  publish_worklet_time(scope, ctx);
  let rendered = {
    let context = v8::Local::new(scope, context);
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    call_process(scope, instance, &inputs, output_channels, params, quantum)
  };
  match rendered {
    Ok((outputs, keep)) => {
      if !keep {
        NODES.with(|nodes| {
          if let Some(node) = nodes.borrow_mut().get_mut(&id)
            && let Kind::Worklet { active, .. } = &mut node.kind
          {
            *active = false;
          }
        });
      }
      outputs
    }
    Err(()) => silence_outputs(output_channels, quantum),
  }
}

fn call_process(
  scope: &mut v8::PinScope,
  instance: &v8::Global<v8::Object>,
  inputs: &[Vec<Vec<f32>>],
  output_channels: &[u32],
  params: &[WorkletParam],
  quantum: usize,
) -> Result<(Vec<Vec<f32>>, bool), ()> {
  let instance = v8::Local::new(scope, instance);
  let input_value = nested_arrays(scope, inputs);
  let mut output_locals = Vec::new();
  let outputs = v8::Array::new(scope, output_channels.len() as i32);
  for (index, count) in output_channels.iter().enumerate() {
    let channels = v8::Array::new(scope, *count as i32);
    let mut locals = Vec::new();
    for channel in 0..*count {
      let view = f32_array(scope, &vec![0.0; quantum]);
      channels.set_index(scope, channel, view.into());
      locals.push(view);
    }
    outputs.set_index(scope, index as u32, channels.into());
    output_locals.push(locals);
  }
  let parameters = v8::Object::new(scope);
  for param in params {
    let length = if param.a_rate { quantum } else { 1 };
    let value = param_value(param.id) as f32;
    let view = f32_array(scope, &vec![value; length]);
    let key = v8::String::new(scope, &param.name).ok_or(())?;
    parameters.set(scope, key.into(), view.into());
  }
  let key = v8::String::new(scope, "process").ok_or(())?;
  let method = instance.get(scope, key.into()).ok_or(())?;
  let function = v8::Local::<v8::Function>::try_from(method).map_err(|_| ())?;
  let result = function
    .call(
      scope,
      instance.into(),
      &[input_value.into(), outputs.into(), parameters.into()],
    )
    .ok_or(())?;
  let keep = !result.is_false();
  let mut rendered = Vec::new();
  for locals in output_locals {
    for view in locals {
      rendered.push(util::read_f32_view(scope, view));
    }
  }
  // One output bus is the node output. Extra buses are appended; the graph
  // uses the first `output_channels[0]` channels when there is one output.
  if output_channels.len() == 1 {
    let count = output_channels[0] as usize;
    rendered.truncate(count);
    Ok((rendered, keep))
  } else {
    Ok((rendered, keep))
  }
}

fn nested_arrays<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  inputs: &[Vec<Vec<f32>>],
) -> v8::Local<'s, v8::Array> {
  let outer = v8::Array::new(scope, inputs.len() as i32);
  for (index, channels) in inputs.iter().enumerate() {
    let inner = v8::Array::new(scope, channels.len() as i32);
    for (channel, samples) in channels.iter().enumerate() {
      let view = f32_array(scope, samples);
      inner.set_index(scope, channel as u32, view.into());
    }
    outer.set_index(scope, index as u32, inner.into());
  }
  outer
}

fn publish_worklet_time(scope: &mut v8::PinScope, id: u64) {
  let snapshot = CONTEXTS.with(|contexts| {
    contexts.borrow().get(&id).and_then(|context| {
      context.worklet.as_ref().map(|worklet| {
        (
          worklet.context.clone(),
          context.frame,
          context.frame as f64 / context.sample_rate,
          context.sample_rate,
          context.quantum,
        )
      })
    })
  });
  let Some((context, frame, time, rate, quantum)) = snapshot else {
    return;
  };
  let context = v8::Local::new(scope, context);
  let scope = &mut v8::ContextScope::new(scope, context);
  set_number(scope, "currentFrame", frame as f64);
  set_number(scope, "currentTime", time);
  set_number(scope, "sampleRate", rate);
  set_number(scope, "renderQuantumSize", f64::from(quantum));
}

fn set_number(scope: &mut v8::PinScope, name: &str, value: f64) {
  let global = scope.get_current_context().global(scope);
  let key = v8::String::new(scope, name).unwrap();
  let number = v8::Number::new(scope, value);
  global.set(scope, key.into(), number.into());
}

pub fn worklet_port(id: u64) -> Option<v8::Global<v8::Object>> {
  CONTEXTS.with(|contexts| {
    contexts.borrow().get(&id).and_then(|context| {
      context.worklet.as_ref().map(|worklet| worklet.port.clone())
    })
  })
}

pub fn create_port_pair(
  scope: &mut v8::PinScope,
) -> (v8::Global<v8::Object>, v8::Global<v8::Object>) {
  let (main, processor) = yuzuko_dom::entangled_ports(scope);
  (
    v8::Global::new(scope, main),
    v8::Global::new(scope, processor),
  )
}
