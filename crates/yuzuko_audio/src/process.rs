//! Sample-level processors. No V8.
//!
//! A few steps differ from the Web Audio specification and are recorded in
//! `docs/deviations.md`: the compressor has no look-ahead and its detector
//! settles in one sample, shelf filters treat `Q` as slope, convolution
//! resamples linearly, waveshaper oversampling is a causal average, and
//! HRTF panning is an azimuth delay rather than a head-related impulse
//! response.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BiquadKind {
  Lowpass,
  Highpass,
  Bandpass,
  Lowshelf,
  Highshelf,
  Peaking,
  Notch,
  Allpass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistanceModel {
  Linear,
  Inverse,
  Exponential,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanningModel {
  EqualPower,
  Hrtf,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BiquadMemory {
  pub x1: f32,
  pub x2: f32,
  pub y1: f32,
  pub y2: f32,
}

#[derive(Clone, Debug)]
pub struct IirMemory {
  pub x: Vec<f32>,
  pub y: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct DelayState {
  pub lines: Vec<Vec<f32>>,
  pub write: usize,
}

/// Mono history for the azimuth delay. The newest sample is last.
#[derive(Clone, Debug, Default)]
pub struct HrtfState {
  pub history: Vec<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct ShapeState {
  pub previous: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct CompressorState {
  pub gain: f32,
  pub detector: f32,
  pub reduction: f32,
}

impl Default for CompressorState {
  fn default() -> Self {
    Self {
      gain: 1.0,
      detector: 0.0,
      reduction: 0.0,
    }
  }
}

#[derive(Clone, Debug)]
pub struct AnalyserState {
  pub ring: Vec<f32>,
  pub write: usize,
  pub count: usize,
  pub mag: Vec<f32>,
}

#[derive(Debug)]
pub enum IirError {
  Length,
  Feedforward,
  Feedback,
}

#[allow(clippy::needless_range_loop)]
pub fn delay_block(
  input: &[Vec<f32>],
  delay: &[f64],
  frames: usize,
  min_samples: f64,
  state: &mut DelayState,
) -> Vec<Vec<f32>> {
  let quantum = block_len(input, delay.len());
  let channels = input.len().max(1);
  let width = frames.max(2);
  if state.lines.len() != channels
    || state.lines.first().is_some_and(|line| line.len() != width)
  {
    state.lines = vec![vec![0.0; width]; channels];
    state.write = 0;
  }
  let mut output = vec![vec![0.0; quantum]; channels];
  let limit = (width - 2) as f64;
  let floor = min_samples.max(0.0);
  for index in 0..quantum {
    let samples = delay.get(index).copied().unwrap_or(0.0);
    let delay_samples = if samples.is_finite() {
      samples.clamp(floor, limit)
    } else {
      floor.clamp(0.0, limit)
    };
    let write = state.write % width;
    for channel in 0..channels {
      let sample = channel_sample(input, channel, index);
      let line = &mut state.lines[channel];
      line[write] = sample;
      let position = (write as f64 - delay_samples).rem_euclid(width as f64);
      let base = position.floor() as usize % width;
      let fraction = (position - base as f64) as f32;
      let next = (base + 1) % width;
      output[channel][index] =
        line[base] * (1.0 - fraction) + line[next] * fraction;
    }
    state.write = (write + 1) % width;
  }
  output
}

/// Output of one quantum from samples already stored in the delay line.
///
/// `min_samples` is one render quantum when the node is in a cycle, so none
/// of these reads overlap the input that [`delay_commit`] will append.
#[allow(clippy::needless_range_loop)]
pub fn delay_read_past(
  state: &DelayState,
  delay: &[f64],
  min_samples: f64,
  quantum: usize,
) -> Vec<Vec<f32>> {
  let channels = state.lines.len().max(1);
  let mut output = vec![vec![0.0; quantum]; channels];
  if state.lines.is_empty() {
    return output;
  }
  let width = state.lines[0].len().max(2);
  let limit = (width.saturating_sub(2)) as f64;
  let floor = min_samples.max(0.0);
  for index in 0..quantum {
    let samples = delay.get(index).copied().unwrap_or(0.0);
    let delay_samples = if samples.is_finite() {
      samples.clamp(floor, limit)
    } else {
      floor.clamp(0.0, limit)
    };
    let cursor = state.write as f64 + index as f64;
    for channel in 0..channels {
      let line = &state.lines[channel];
      let position = (cursor - delay_samples).rem_euclid(width as f64);
      let base = position.floor() as usize % width;
      let fraction = (position - base as f64) as f32;
      let next = (base + 1) % width;
      output[channel][index] =
        line[base] * (1.0 - fraction) + line[next] * fraction;
    }
  }
  output
}

/// Append `input` to the delay line. The line is resized to `frames` when the
/// channel count or capacity does not match, which drops history.
pub fn delay_commit(state: &mut DelayState, input: &[Vec<f32>], frames: usize) {
  let quantum = input.first().map(|channel| channel.len()).unwrap_or(0);
  let channels = input.len().max(1);
  let width = frames.max(2);
  if state.lines.len() != channels
    || state.lines.first().is_some_and(|line| line.len() != width)
  {
    state.lines = vec![vec![0.0; width]; channels];
    state.write = 0;
  }
  for index in 0..quantum {
    let write = state.write % width;
    for (channel, line) in state.lines.iter_mut().enumerate() {
      line[write] = channel_sample(input, channel, index);
    }
    state.write = (write + 1) % width;
  }
}

#[allow(clippy::needless_range_loop, clippy::too_many_arguments)]
pub fn biquad_block(
  input: &[Vec<f32>],
  kind: BiquadKind,
  frequency: &[f64],
  detune: &[f64],
  q: &[f64],
  gain: &[f64],
  rate: f64,
  memory: &mut Vec<BiquadMemory>,
) -> Vec<Vec<f32>> {
  let quantum = block_len(input, frequency.len());
  let channels = input.len().max(1);
  memory.resize(channels, BiquadMemory::default());
  let mut output = vec![vec![0.0; quantum]; channels];
  for index in 0..quantum {
    let coeffs = biquad_coeffs(
      kind,
      frequency.get(index).copied().unwrap_or(0.0),
      detune.get(index).copied().unwrap_or(0.0),
      q.get(index).copied().unwrap_or(1.0),
      gain.get(index).copied().unwrap_or(0.0),
      rate,
    );
    for channel in 0..channels {
      let sample = channel_sample(input, channel, index);
      output[channel][index] =
        apply_biquad(sample, coeffs, &mut memory[channel]);
    }
  }
  output
}

pub fn biquad_response_at(
  kind: BiquadKind,
  frequency: f64,
  detune: f64,
  q: f64,
  gain: f64,
  rate: f64,
  probe: f64,
) -> (f32, f32) {
  if !probe.is_finite() || probe < 0.0 || rate <= 0.0 || probe > rate / 2.0 {
    return (f32::NAN, f32::NAN);
  }
  let coeffs = biquad_coeffs(kind, frequency, detune, q, gain, rate);
  response_sample(coeffs, 2.0 * PI * probe / rate)
}

pub fn normalize_iir(
  mut feedforward: Vec<f32>,
  mut feedback: Vec<f32>,
) -> Result<(Vec<f32>, Vec<f32>), IirError> {
  if !(1..=20).contains(&feedforward.len())
    || !(1..=20).contains(&feedback.len())
  {
    return Err(IirError::Length);
  }
  if feedforward.iter().all(|coef| *coef == 0.0) {
    return Err(IirError::Feedforward);
  }
  if feedback[0] == 0.0 {
    return Err(IirError::Feedback);
  }
  let scale = feedback[0];
  for coef in &mut feedforward {
    *coef /= scale;
  }
  for coef in &mut feedback {
    *coef /= scale;
  }
  Ok((feedforward, feedback))
}

#[allow(clippy::needless_range_loop)]
pub fn iir_block(
  input: &[Vec<f32>],
  feedforward: &[f32],
  feedback: &[f32],
  memory: &mut Vec<IirMemory>,
) -> Vec<Vec<f32>> {
  let quantum = block_len(input, 0);
  let channels = input.len().max(1);
  if memory.len() != channels {
    memory.clear();
    memory.resize(channels, empty_iir(feedforward, feedback));
  }
  for slot in memory.iter_mut() {
    fit_iir(slot, feedforward, feedback);
  }
  let mut output = vec![vec![0.0; quantum]; channels];
  for index in 0..quantum {
    for channel in 0..channels {
      let sample = channel_sample(input, channel, index);
      output[channel][index] =
        iir_sample(sample, feedforward, feedback, &mut memory[channel]);
    }
  }
  output
}

pub fn iir_response_at(
  feedforward: &[f32],
  feedback: &[f32],
  rate: f64,
  probe: f64,
) -> (f32, f32) {
  if !probe.is_finite() || probe < 0.0 || rate <= 0.0 || probe > rate / 2.0 {
    return (f32::NAN, f32::NAN);
  }
  let omega = 2.0 * PI * probe / rate;
  let mut num_re = 0.0;
  let mut num_im = 0.0;
  let mut den_re = 0.0;
  let mut den_im = 0.0;
  for (index, coef) in feedforward.iter().enumerate() {
    let angle = omega * index as f64;
    num_re += f64::from(*coef) * angle.cos();
    num_im -= f64::from(*coef) * angle.sin();
  }
  for (index, coef) in feedback.iter().enumerate() {
    let angle = omega * index as f64;
    den_re += f64::from(*coef) * angle.cos();
    den_im -= f64::from(*coef) * angle.sin();
  }
  complex_response(num_re, num_im, den_re, den_im)
}

#[allow(clippy::needless_range_loop)]
pub fn shape_block(
  input: &[Vec<f32>],
  curve: Option<&[f32]>,
  factor: u8,
  state: &mut ShapeState,
) -> Vec<Vec<f32>> {
  let quantum = block_len(input, 0);
  let channels = input.len().max(1);
  if state.previous.len() != channels {
    state.previous.resize(channels, 0.0);
  }
  let steps = match factor {
    2 => 2,
    4 => 4,
    _ => 1,
  };
  let mut output = vec![vec![0.0; quantum]; channels];
  for channel in 0..channels {
    for index in 0..quantum {
      let sample = channel_sample(input, channel, index);
      output[channel][index] =
        shape_oversampled(state.previous[channel], sample, curve, steps);
      state.previous[channel] = sample;
    }
  }
  output
}

pub fn shape_sample(sample: f32, curve: &[f32]) -> f32 {
  let length = curve.len();
  if length < 2 {
    return sample;
  }
  if sample <= -1.0 {
    return curve[0];
  }
  if sample >= 1.0 {
    return curve[length - 1];
  }
  let position = (f64::from(sample) + 1.0) * 0.5 * (length - 1) as f64;
  let index = position.floor() as usize;
  let fraction = (position - index as f64) as f32;
  let next = (index + 1).min(length - 1);
  curve[index] + (curve[next] - curve[index]) * fraction
}

pub fn stereo_pan_block(input: &[Vec<f32>], pan: &[f64]) -> Vec<Vec<f32>> {
  let quantum = block_len(input, pan.len());
  let stereo = input.len() >= 2;
  let mut left = vec![0.0; quantum];
  let mut right = vec![0.0; quantum];
  for index in 0..quantum {
    let value = pan.get(index).copied().unwrap_or(0.0);
    let pan = if value.is_finite() {
      value.clamp(-1.0, 1.0)
    } else {
      0.0
    };
    let (gain_l, gain_r, negative) = stereo_gains(pan, stereo);
    let inl = channel_sample(input, 0, index);
    let inr = if stereo {
      channel_sample(input, 1, index)
    } else {
      inl
    };
    let (sample_l, sample_r) =
      apply_pan(inl, inr, gain_l, gain_r, stereo, negative);
    left[index] = sample_l;
    right[index] = sample_r;
  }
  vec![left, right]
}

pub fn azimuth_elevation(
  source: [f64; 3],
  listener: [f64; 3],
  forward: [f64; 3],
  up: [f64; 3],
) -> (f64, f64) {
  let mut source_listener = sub(source, listener);
  let magnitude = length(source_listener);
  if magnitude == 0.0 {
    return (0.0, 0.0);
  }
  source_listener = scale(source_listener, 1.0 / magnitude);
  let mut listener_right = cross(forward, up);
  let right_mag = length(listener_right);
  if right_mag == 0.0 {
    return (0.0, 0.0);
  }
  listener_right = scale(listener_right, 1.0 / right_mag);
  let Some(forward_norm) = normalized(forward) else {
    return (0.0, 0.0);
  };
  let up_norm = cross(listener_right, forward_norm);
  let up_projection = dot(source_listener, up_norm);
  let mut projected = sub(source_listener, scale(up_norm, up_projection));
  let projected_mag = length(projected);
  let azimuth = if projected_mag == 0.0 {
    0.0
  } else {
    projected = scale(projected, 1.0 / projected_mag);
    let mut angle =
      180.0 * clamp_unit(dot(projected, listener_right)).acos() / PI;
    if dot(projected, forward_norm) < 0.0 {
      angle = 360.0 - angle;
    }
    if (0.0..=270.0).contains(&angle) {
      90.0 - angle
    } else {
      450.0 - angle
    }
  };
  let mut elevation =
    90.0 - 180.0 * clamp_unit(dot(source_listener, up_norm)).acos() / PI;
  if elevation > 90.0 {
    elevation = 180.0 - elevation;
  } else if elevation < -90.0 {
    elevation = -180.0 - elevation;
  }
  (azimuth, elevation)
}

#[allow(clippy::too_many_arguments)]
pub fn panner_block(
  input: &[Vec<f32>],
  source_position: [&[f64]; 3],
  source_orientation: [&[f64]; 3],
  listener_position: [&[f64]; 3],
  listener_forward: [&[f64]; 3],
  listener_up: [&[f64]; 3],
  model: DistanceModel,
  ref_distance: f64,
  max_distance: f64,
  rolloff: f64,
  cone_inner: f64,
  cone_outer: f64,
  cone_gain: f64,
) -> Vec<Vec<f32>> {
  let quantum = block_len(input, source_position[0].len());
  let stereo = input.len() >= 2;
  let mut left = vec![0.0; quantum];
  let mut right = vec![0.0; quantum];
  for index in 0..quantum {
    let source = sample_vec(source_position, index);
    let orientation = sample_vec(source_orientation, index);
    let listener = sample_vec(listener_position, index);
    let forward = sample_vec(listener_forward, index);
    let up = sample_vec(listener_up, index);
    let (azimuth, _elevation) =
      azimuth_elevation(source, listener, forward, up);
    let wrapped = wrap_azimuth(azimuth);
    let (gain_l, gain_r, negative) = panner_gains(wrapped, stereo);
    let distance = length(sub(source, listener));
    let spatial =
      distance_gain(model, distance, ref_distance, max_distance, rolloff)
        * cone_gain_of(
          orientation,
          source,
          listener,
          cone_inner,
          cone_outer,
          cone_gain,
        );
    let inl = channel_sample(input, 0, index);
    let inr = if stereo {
      channel_sample(input, 1, index)
    } else {
      inl
    };
    let (sample_l, sample_r) =
      apply_pan(inl, inr, gain_l, gain_r, stereo, negative);
    let scale = spatial as f32;
    left[index] = sample_l * scale;
    right[index] = sample_r * scale;
  }
  vec![left, right]
}

const HRTF_DELAY_SAMPLES: usize = 16;

/// Positive azimuth delays the left ear. This is not an HRIR.
fn hrtf_ears(azimuth: f64) -> (usize, usize, f32, f32) {
  let side = (wrap_azimuth(azimuth) / 90.0).clamp(-1.0, 1.0);
  let magnitude = side.abs();
  let samples = HRTF_DELAY_SAMPLES as f64;
  let delay = (magnitude * samples).round() as usize;
  let far = (1.0 - 0.45 * magnitude) as f32;
  if side > 0.0 {
    (delay, 0, far, 1.0)
  } else if side < 0.0 {
    (0, delay, 1.0, far)
  } else {
    (0, 0, 1.0, 1.0)
  }
}

fn delayed(history: &[f32], delay: usize) -> f32 {
  let Some(index) = history.len().checked_sub(delay + 1) else {
    return 0.0;
  };
  history[index]
}

#[allow(clippy::too_many_arguments)]
pub fn hrtf_block(
  input: &[Vec<f32>],
  source_position: [&[f64]; 3],
  source_orientation: [&[f64]; 3],
  listener_position: [&[f64]; 3],
  listener_forward: [&[f64]; 3],
  listener_up: [&[f64]; 3],
  model: DistanceModel,
  ref_distance: f64,
  max_distance: f64,
  rolloff: f64,
  cone_inner: f64,
  cone_outer: f64,
  cone_gain: f64,
  state: &mut HrtfState,
) -> Vec<Vec<f32>> {
  let quantum = block_len(input, source_position[0].len());
  let stereo = input.len() >= 2;
  let mut left = vec![0.0; quantum];
  let mut right = vec![0.0; quantum];
  let keep = HRTF_DELAY_SAMPLES + 1;
  for index in 0..quantum {
    let source = sample_vec(source_position, index);
    let orientation = sample_vec(source_orientation, index);
    let listener = sample_vec(listener_position, index);
    let forward = sample_vec(listener_forward, index);
    let up = sample_vec(listener_up, index);
    let (azimuth, _elevation) =
      azimuth_elevation(source, listener, forward, up);
    let distance = length(sub(source, listener));
    let spatial =
      distance_gain(model, distance, ref_distance, max_distance, rolloff)
        * cone_gain_of(
          orientation,
          source,
          listener,
          cone_inner,
          cone_outer,
          cone_gain,
        );
    let inl = channel_sample(input, 0, index);
    let inr = if stereo {
      channel_sample(input, 1, index)
    } else {
      inl
    };
    let mono = if stereo { 0.5 * (inl + inr) } else { inl };
    state.history.push(mono * spatial as f32);
    if state.history.len() > keep {
      let extra = state.history.len() - keep;
      state.history.drain(0..extra);
    }
    let (delay_l, delay_r, gain_l, gain_r) = hrtf_ears(azimuth);
    left[index] = delayed(&state.history, delay_l) * gain_l;
    right[index] = delayed(&state.history, delay_r) * gain_r;
  }
  vec![left, right]
}

pub fn distance_gain(
  model: DistanceModel,
  distance: f64,
  ref_distance: f64,
  max_distance: f64,
  rolloff: f64,
) -> f64 {
  let rolloff = rolloff.max(0.0);
  match model {
    DistanceModel::Inverse => {
      let reached = distance.max(ref_distance);
      let denom = ref_distance + rolloff * (reached - ref_distance);
      if denom == 0.0 {
        1.0
      } else {
        ref_distance / denom
      }
    }
    DistanceModel::Linear => {
      if max_distance <= ref_distance {
        if distance <= ref_distance { 1.0 } else { 0.0 }
      } else {
        let reached = distance.clamp(ref_distance, max_distance);
        let gain = 1.0
          - rolloff * (reached - ref_distance) / (max_distance - ref_distance);
        gain.clamp(0.0, 1.0)
      }
    }
    DistanceModel::Exponential => {
      if ref_distance == 0.0 {
        if distance == 0.0 { 1.0 } else { 0.0 }
      } else {
        (distance.max(ref_distance) / ref_distance).powf(-rolloff)
      }
    }
  }
}

/// Angle between the source orientation and the direction toward the listener.
pub fn cone_gain_of(
  orientation: [f64; 3],
  source: [f64; 3],
  listener: [f64; 3],
  inner: f64,
  outer: f64,
  outer_gain: f64,
) -> f64 {
  if length(orientation) == 0.0
    || (inner.abs() == 360.0 && outer.abs() == 360.0)
  {
    return 1.0;
  }
  let toward = sub(listener, source);
  let toward_len = length(toward);
  if toward_len == 0.0 {
    return 1.0;
  }
  let cosine =
    clamp_unit(dot(orientation, toward) / (length(orientation) * toward_len));
  let angle = cosine.acos().to_degrees();
  let abs_inner = inner.abs() / 2.0;
  let abs_outer = outer.abs() / 2.0;
  if angle <= abs_inner {
    return 1.0;
  }
  if abs_outer <= abs_inner || angle >= abs_outer {
    return outer_gain;
  }
  let blend = (angle - abs_inner) / (abs_outer - abs_inner);
  (1.0 - blend) + outer_gain * blend
}

pub fn convolver_scale(
  channels: &[Vec<f32>],
  sample_rate: f64,
  normalize: bool,
) -> f32 {
  if !normalize {
    return 1.0;
  }
  let mut sum = 0.0;
  let mut count = 0.0;
  for channel in channels {
    for sample in channel {
      let value = f64::from(*sample);
      sum += value * value;
      count += 1.0;
    }
  }
  let mut power = if count == 0.0 {
    0.0
  } else {
    (sum / count).sqrt()
  };
  if !power.is_finite() || power < 0.000125 {
    power = 0.000125;
  }
  let mut scale = 0.00125 / power;
  if sample_rate > 0.0 {
    scale *= 44100.0 / sample_rate;
  }
  if channels.len() == 4 {
    scale *= 0.5;
  }
  scale as f32
}

pub fn resample_linear(input: &[f32], from: f64, to: f64) -> Vec<f32> {
  if input.is_empty()
    || !from.is_finite()
    || !to.is_finite()
    || from <= 0.0
    || to <= 0.0
  {
    return input.to_vec();
  }
  if (from - to).abs() < 1e-9 {
    return input.to_vec();
  }
  let out_len = ((input.len() as f64) * to / from).round().max(1.0) as usize;
  let step = from / to;
  let mut output = Vec::with_capacity(out_len);
  for index in 0..out_len {
    let position = index as f64 * step;
    let base = position.floor() as usize;
    let fraction = (position - base as f64) as f32;
    let left = input.get(base).copied().unwrap_or(0.0);
    let right = input.get(base + 1).copied().unwrap_or(0.0);
    output.push(left + (right - left) * fraction);
  }
  output
}

pub fn convolve_block(
  input: &[Vec<f32>],
  kernels: &[Vec<f32>],
  history: &mut Vec<Vec<f32>>,
  quantum: usize,
) -> Vec<Vec<f32>> {
  if kernels.is_empty() {
    return vec![vec![0.0; quantum]; input.len().max(1)];
  }
  match kernels.len() {
    1 => convolve_each(input, &kernels[0], history, quantum),
    2 => {
      let stereo = input.len() >= 2;
      prepare_paths(history, 2);
      prepare_kernel(&mut history[0], kernels[0].len());
      prepare_kernel(&mut history[1], kernels[1].len());
      let mut left = vec![0.0; quantum];
      let mut right = vec![0.0; quantum];
      for index in 0..quantum {
        let inl = channel_sample(input, 0, index);
        let inr = if stereo {
          channel_sample(input, 1, index)
        } else {
          inl
        };
        left[index] = convolve_sample(inl, &kernels[0], &mut history[0]);
        right[index] = convolve_sample(inr, &kernels[1], &mut history[1]);
      }
      vec![left, right]
    }
    4 => {
      prepare_paths(history, 4);
      for (slot, kernel) in kernels.iter().enumerate() {
        prepare_kernel(&mut history[slot], kernel.len());
      }
      let mut left = vec![0.0; quantum];
      let mut right = vec![0.0; quantum];
      for index in 0..quantum {
        let inl = channel_sample(input, 0, index);
        let inr = channel_sample(input, 1, index);
        left[index] = convolve_sample(inl, &kernels[0], &mut history[0])
          + convolve_sample(inr, &kernels[2], &mut history[2]);
        right[index] = convolve_sample(inl, &kernels[1], &mut history[1])
          + convolve_sample(inr, &kernels[3], &mut history[3]);
      }
      vec![left, right]
    }
    _ => vec![vec![0.0; quantum]; input.len().max(1)],
  }
}

pub fn compression_curve(
  input: f64,
  threshold_db: f64,
  knee_db: f64,
  ratio: f64,
) -> f64 {
  let input = input.abs();
  let ratio = ratio.max(1.0);
  let threshold = db_to_lin(threshold_db);
  if input <= threshold {
    return input;
  }
  let sloped = threshold + (input - threshold) / ratio;
  if knee_db <= 0.0 {
    return sloped;
  }
  let knee_end = db_to_lin(threshold_db + knee_db);
  if input >= knee_end || (knee_end - threshold).abs() < 1e-12 {
    return threshold + (input - threshold) / ratio;
  }
  let span = knee_end - threshold;
  let y1 = threshold + span / ratio;
  let t = (input - threshold) / span;
  hermite(t, threshold, y1, span, span / ratio)
}

pub fn automatic_makeup_linear(
  threshold_db: f64,
  knee_db: f64,
  ratio: f64,
) -> f64 {
  let full = compression_curve(1.0, threshold_db, knee_db, ratio);
  if full <= 0.0 || !full.is_finite() {
    1.0
  } else {
    (1.0 / full).powf(0.6)
  }
}

pub fn automatic_makeup_db(threshold_db: f64, knee_db: f64, ratio: f64) -> f64 {
  linear_to_db(automatic_makeup_linear(threshold_db, knee_db, ratio))
}

pub fn db_to_lin(db: f64) -> f64 {
  10f64.powf(db / 20.0)
}

pub fn linear_to_db(value: f64) -> f64 {
  if value <= 0.0 || !value.is_finite() {
    -1000.0
  } else {
    20.0 * value.log10()
  }
}

pub fn makeup_max_db() -> f64 {
  20.0 * (f32::MAX as f64).log10()
}

/// One detector follows the loudest channel. Attack 0 reaches the curve on
/// the same sample. There is no look-ahead delay.
#[allow(clippy::needless_range_loop, clippy::too_many_arguments)]
pub fn compress_block(
  input: &[Vec<f32>],
  threshold: f64,
  knee: f64,
  ratio: f64,
  attack: f64,
  release: f64,
  makeup: f64,
  rate: f64,
  state: &mut CompressorState,
) -> Vec<Vec<f32>> {
  let quantum = block_len(input, 0);
  let channels = input.len().max(1);
  let mut output = vec![vec![0.0; quantum]; channels];
  if quantum == 0 {
    return output;
  }
  let rate = if rate.is_finite() && rate > 0.0 {
    rate
  } else {
    48000.0
  };
  for index in 0..quantum {
    let mut level = 0.0f32;
    for channel in 0..channels {
      level = level.max(channel_sample(input, channel, index).abs());
    }
    let attenuation = if level < 0.0001 {
      1.0
    } else {
      let shaped = compression_curve(f64::from(level), threshold, knee, ratio);
      (shaped / f64::from(level)) as f32
    };
    let attenuation = if attenuation.is_finite() {
      attenuation
    } else {
      1.0
    };
    // The follower uses a full-rate step so attack 0 settles immediately.
    state.detector += (attenuation - state.detector) * 1.0;
    if state.detector > 1.0 {
      state.detector = 1.0;
    }
    let releasing = attenuation > state.gain;
    if !releasing {
      let attack_samples = attack.max(0.0) * rate;
      let envelope = if attack_samples <= 0.0 {
        1.0
      } else {
        1.0 - (-1.0 / attack_samples).exp()
      };
      state.gain += (state.detector - state.gain) * envelope as f32;
    } else {
      let release_samples = release.max(0.0) * rate;
      let envelope = if release_samples <= 0.0 {
        1.0e9
      } else {
        1.0 / (1.0 - (-1.0 / release_samples).exp())
      };
      state.gain = (state.gain * envelope as f32).min(1.0);
    }
    if !state.gain.is_finite() {
      state.gain = 1.0;
    }
    let reduction = state.gain * makeup as f32;
    for channel in 0..channels {
      output[channel][index] =
        channel_sample(input, channel, index) * reduction;
    }
    if index + 1 == quantum {
      state.reduction = linear_to_db(f64::from(reduction)) as f32;
    }
  }
  output
}

pub fn analyser_reset(fft_size: usize) -> AnalyserState {
  let fft_size = fft_size.max(2);
  AnalyserState {
    ring: vec![0.0; fft_size],
    write: 0,
    count: 0,
    mag: vec![0.0; fft_size / 2],
  }
}

pub fn push_ring(
  ring: &mut [f32],
  write: &mut usize,
  count: &mut usize,
  samples: &[f32],
) {
  let width = ring.len();
  if width == 0 {
    return;
  }
  for sample in samples {
    if *count < width {
      ring[*count] = *sample;
      *count += 1;
      *write = *count % width;
    } else {
      ring[*write] = *sample;
      *write = (*write + 1) % width;
    }
  }
}

pub fn time_domain(ring: &[f32], write: usize, count: usize) -> Vec<f32> {
  let width = ring.len();
  let mut output = vec![0.0; width];
  if width == 0 || count == 0 {
    return output;
  }
  if count < width {
    let start = width - count;
    output[start..].copy_from_slice(&ring[..count]);
    return output;
  }
  let write = write % width;
  output[..width - write].copy_from_slice(&ring[write..]);
  output[width - write..].copy_from_slice(&ring[..write]);
  output
}

pub fn analyser_process(
  state: &mut AnalyserState,
  mono: &[f32],
  smoothing: f64,
) {
  push_ring(&mut state.ring, &mut state.write, &mut state.count, mono);
  let mut windowed = time_domain(&state.ring, state.write, state.count);
  let window = blackman(windowed.len());
  for (sample, weight) in windowed.iter_mut().zip(window) {
    *sample *= weight;
  }
  let magnitudes = fft_magnitudes(&windowed);
  if state.mag.len() != magnitudes.len() {
    state.mag = vec![0.0; magnitudes.len()];
  }
  let smoothing = if smoothing.is_finite() {
    smoothing.clamp(0.0, 1.0)
  } else {
    0.0
  };
  for (slot, magnitude) in state.mag.iter_mut().zip(magnitudes) {
    let magnitude = if magnitude.is_finite() {
      f64::from(magnitude)
    } else {
      0.0
    };
    let next = smoothing * f64::from(*slot) + (1.0 - smoothing) * magnitude;
    *slot = if next.is_finite() { next as f32 } else { 0.0 };
  }
}

pub fn blackman(width: usize) -> Vec<f32> {
  let alpha = 0.16;
  let a0 = (1.0 - alpha) / 2.0;
  let a1 = 0.5;
  let a2 = alpha / 2.0;
  (0..width)
    .map(|index| {
      let n = width as f64;
      let i = index as f64;
      (a0 - a1 * (2.0 * PI * i / n).cos() + a2 * (4.0 * PI * i / n).cos())
        as f32
    })
    .collect()
}

pub fn fft_magnitudes(input: &[f32]) -> Vec<f32> {
  let width = input.len();
  if width < 2 || !width.is_power_of_two() {
    return Vec::new();
  }
  let (mut real, mut imag) = bit_reverse(input);
  let mut len = 2;
  while len <= width {
    let half = len / 2;
    let theta = -2.0 * PI / len as f64;
    for start in (0..width).step_by(len) {
      for bin in 0..half {
        let angle = theta * bin as f64;
        let (w_re, w_im) = (angle.cos(), angle.sin());
        let even = start + bin;
        let odd = even + half;
        let t_re = w_re * real[odd] - w_im * imag[odd];
        let t_im = w_re * imag[odd] + w_im * real[odd];
        real[odd] = real[even] - t_re;
        imag[odd] = imag[even] - t_im;
        real[even] += t_re;
        imag[even] += t_im;
      }
    }
    len *= 2;
  }
  let scale = 1.0 / width as f64;
  (0..width / 2)
    .map(|bin| {
      ((real[bin] * real[bin] + imag[bin] * imag[bin]).sqrt() * scale) as f32
    })
    .collect()
}

pub fn db_from_magnitude(magnitude: f32) -> f32 {
  if magnitude <= 0.0 || !magnitude.is_finite() {
    f32::NEG_INFINITY
  } else {
    20.0 * magnitude.log10()
  }
}

pub fn byte_time_sample(sample: f32) -> u8 {
  let value = 128.0 + 128.0 * f64::from(sample);
  if !value.is_finite() {
    return if value.is_sign_negative() { 0 } else { 255 };
  }
  value.round().clamp(0.0, 255.0) as u8
}

pub fn byte_frequency(db: f32, min_db: f64, max_db: f64) -> u8 {
  if !db.is_finite() {
    return if db.is_sign_positive() { 255 } else { 0 };
  }
  if max_db <= min_db {
    return 0;
  }
  let scaled = (f64::from(db) - min_db) / (max_db - min_db) * 255.0;
  if !scaled.is_finite() {
    return 0;
  }
  scaled.round().clamp(0.0, 255.0) as u8
}

pub fn downmix_mono(input: &[Vec<f32>], quantum: usize) -> Vec<f32> {
  if input.is_empty() {
    return vec![0.0; quantum];
  }
  let mut output = vec![vec![0.0; quantum]];
  if mix_speakers(input, &mut output, quantum) {
    return output.remove(0);
  }
  let mut mono = vec![0.0; quantum];
  copy_channel(&input[0], &mut mono, quantum);
  mono
}

/// Speaker up-mix and down-mix for the canonical layouts 1, 2, 4, and 6.
/// `output` starts at zero. Other channel counts return false so the caller
/// can use discrete mixing.
pub(crate) fn mix_speakers(
  input: &[Vec<f32>],
  output: &mut [Vec<f32>],
  quantum: usize,
) -> bool {
  if input.is_empty() || output.is_empty() {
    return false;
  }
  let half = std::f32::consts::FRAC_1_SQRT_2;
  match (input.len(), output.len()) {
    (1, 1) => copy_channel(&input[0], &mut output[0], quantum),
    (1, 2) | (1, 4) => {
      copy_channel(&input[0], &mut output[0], quantum);
      copy_channel(&input[0], &mut output[1], quantum);
    }
    (1, 6) => copy_channel(&input[0], &mut output[2], quantum),
    (2, 4) | (2, 6) | (4, 6) => {
      copy_channel(&input[0], &mut output[0], quantum);
      copy_channel(&input[1], &mut output[1], quantum);
      if input.len() == 4 {
        copy_channel(&input[2], &mut output[4], quantum);
        copy_channel(&input[3], &mut output[5], quantum);
      }
    }
    (2, 1) => {
      add_scaled(&mut output[0], &input[0], quantum, 0.5);
      add_scaled(&mut output[0], &input[1], quantum, 0.5);
    }
    (4, 1) => {
      for channel in input {
        add_scaled(&mut output[0], channel, quantum, 0.25);
      }
    }
    (6, 1) => {
      add_scaled(&mut output[0], &input[0], quantum, half);
      add_scaled(&mut output[0], &input[1], quantum, half);
      add_scaled(&mut output[0], &input[2], quantum, 1.0);
      add_scaled(&mut output[0], &input[4], quantum, 0.5);
      add_scaled(&mut output[0], &input[5], quantum, 0.5);
    }
    (4, 2) => {
      add_scaled(&mut output[0], &input[0], quantum, 0.5);
      add_scaled(&mut output[0], &input[2], quantum, 0.5);
      add_scaled(&mut output[1], &input[1], quantum, 0.5);
      add_scaled(&mut output[1], &input[3], quantum, 0.5);
    }
    (6, 2) => {
      copy_channel(&input[0], &mut output[0], quantum);
      copy_channel(&input[1], &mut output[1], quantum);
      add_scaled(&mut output[0], &input[2], quantum, half);
      add_scaled(&mut output[0], &input[4], quantum, half);
      add_scaled(&mut output[1], &input[2], quantum, half);
      add_scaled(&mut output[1], &input[5], quantum, half);
    }
    (6, 4) => {
      copy_channel(&input[0], &mut output[0], quantum);
      copy_channel(&input[1], &mut output[1], quantum);
      add_scaled(&mut output[0], &input[2], quantum, half);
      add_scaled(&mut output[1], &input[2], quantum, half);
      copy_channel(&input[4], &mut output[2], quantum);
      copy_channel(&input[5], &mut output[3], quantum);
    }
    _ => return false,
  }
  true
}

fn copy_channel(source: &[f32], dest: &mut [f32], quantum: usize) {
  let count = quantum.min(source.len()).min(dest.len());
  dest[..count].copy_from_slice(&source[..count]);
}

fn add_scaled(dest: &mut [f32], source: &[f32], quantum: usize, scale: f32) {
  let count = quantum.min(source.len()).min(dest.len());
  for index in 0..count {
    dest[index] += source[index] * scale;
  }
}

fn biquad_coeffs(
  kind: BiquadKind,
  frequency: f64,
  detune: f64,
  q: f64,
  gain_db: f64,
  rate: f64,
) -> [f64; 5] {
  // Lowpass and highpass use sin(ω) / (2 * 10^(Q/20)). Frequency 0, Nyquist,
  // and Q <= 0 for the other types use the reference limits. Shelf filters
  // still take Q as the slope.
  let nyquist = if rate.is_finite() && rate > 0.0 {
    rate / 2.0
  } else {
    0.0
  };
  let hz = if frequency.is_finite() && detune.is_finite() {
    (frequency * 2f64.powf(detune / 1200.0)).clamp(0.0, nyquist)
  } else {
    0.0
  };
  let gain_lin = db_to_lin(gain_db);
  let at_dc = hz <= 0.0 || nyquist == 0.0;
  if at_dc || hz >= nyquist {
    return biquad_endpoint(kind, at_dc, gain_lin);
  }
  let w0 = PI * hz / nyquist;
  let (sin, cos) = (w0.sin(), w0.cos());
  let q_positive = q.is_finite() && q > 0.0;
  match kind {
    BiquadKind::Lowpass | BiquadKind::Highpass => {
      let q_db = if q.is_finite() { q } else { 0.0 };
      let alpha = sin / (2.0 * 10f64.powf(q_db / 20.0));
      if kind == BiquadKind::Lowpass {
        let b = (1.0 - cos) / 2.0;
        normalize_biquad(b, 1.0 - cos, b, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
      } else {
        let b = (1.0 + cos) / 2.0;
        normalize_biquad(
          b,
          -(1.0 + cos),
          b,
          1.0 + alpha,
          -2.0 * cos,
          1.0 - alpha,
        )
      }
    }
    BiquadKind::Bandpass if !q_positive => [1.0, 0.0, 0.0, 0.0, 0.0],
    BiquadKind::Notch if !q_positive => [0.0, 0.0, 0.0, 0.0, 0.0],
    BiquadKind::Allpass if !q_positive => [-1.0, 0.0, 0.0, 0.0, 0.0],
    BiquadKind::Peaking if !q_positive => constant_gain(gain_lin),
    BiquadKind::Bandpass
    | BiquadKind::Notch
    | BiquadKind::Allpass
    | BiquadKind::Peaking => {
      let alpha = sin / (2.0 * q);
      let a = 10f64.powf(gain_db / 40.0);
      match kind {
        BiquadKind::Bandpass => normalize_biquad(
          alpha,
          0.0,
          -alpha,
          1.0 + alpha,
          -2.0 * cos,
          1.0 - alpha,
        ),
        BiquadKind::Notch => normalize_biquad(
          1.0,
          -2.0 * cos,
          1.0,
          1.0 + alpha,
          -2.0 * cos,
          1.0 - alpha,
        ),
        BiquadKind::Allpass => normalize_biquad(
          1.0 - alpha,
          -2.0 * cos,
          1.0 + alpha,
          1.0 + alpha,
          -2.0 * cos,
          1.0 - alpha,
        ),
        _ => normalize_biquad(
          1.0 + alpha * a,
          -2.0 * cos,
          1.0 - alpha * a,
          1.0 + alpha / a,
          -2.0 * cos,
          1.0 - alpha / a,
        ),
      }
    }
    BiquadKind::Lowshelf | BiquadKind::Highshelf => {
      let a = 10f64.powf(gain_db / 40.0);
      let slope = if q.is_finite() {
        q.abs().max(1e-6)
      } else {
        1e-6
      };
      let inside = (a + 1.0 / a) * (1.0 / slope - 1.0) + 2.0;
      let shelf_alpha = sin / 2.0 * inside.max(0.0).sqrt();
      let a_sqrt = a.sqrt();
      if kind == BiquadKind::Lowshelf {
        normalize_biquad(
          a * ((a + 1.0) - (a - 1.0) * cos + 2.0 * a_sqrt * shelf_alpha),
          2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
          a * ((a + 1.0) - (a - 1.0) * cos - 2.0 * a_sqrt * shelf_alpha),
          (a + 1.0) + (a - 1.0) * cos + 2.0 * a_sqrt * shelf_alpha,
          -2.0 * ((a - 1.0) + (a + 1.0) * cos),
          (a + 1.0) + (a - 1.0) * cos - 2.0 * a_sqrt * shelf_alpha,
        )
      } else {
        normalize_biquad(
          a * ((a + 1.0) + (a - 1.0) * cos + 2.0 * a_sqrt * shelf_alpha),
          -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
          a * ((a + 1.0) + (a - 1.0) * cos - 2.0 * a_sqrt * shelf_alpha),
          (a + 1.0) - (a - 1.0) * cos + 2.0 * a_sqrt * shelf_alpha,
          2.0 * ((a - 1.0) - (a + 1.0) * cos),
          (a + 1.0) - (a - 1.0) * cos - 2.0 * a_sqrt * shelf_alpha,
        )
      }
    }
  }
}

fn biquad_endpoint(kind: BiquadKind, at_dc: bool, gain_lin: f64) -> [f64; 5] {
  let identity = [1.0, 0.0, 0.0, 0.0, 0.0];
  let silence = [0.0, 0.0, 0.0, 0.0, 0.0];
  match kind {
    BiquadKind::Lowpass => {
      if at_dc {
        [0.0, 0.0, 0.0, -2.0, 1.0]
      } else {
        identity
      }
    }
    BiquadKind::Highpass => {
      if at_dc {
        identity
      } else {
        silence
      }
    }
    BiquadKind::Bandpass => silence,
    BiquadKind::Notch | BiquadKind::Allpass | BiquadKind::Peaking => identity,
    BiquadKind::Lowshelf => {
      if at_dc {
        identity
      } else {
        constant_gain(gain_lin)
      }
    }
    BiquadKind::Highshelf => {
      if at_dc {
        constant_gain(gain_lin)
      } else {
        identity
      }
    }
  }
}

fn constant_gain(gain: f64) -> [f64; 5] {
  if gain.is_finite() {
    [gain, 0.0, 0.0, 0.0, 0.0]
  } else {
    [1.0, 0.0, 0.0, 0.0, 0.0]
  }
}

fn normalize_biquad(
  b0: f64,
  b1: f64,
  b2: f64,
  a0: f64,
  a1: f64,
  a2: f64,
) -> [f64; 5] {
  if !a0.is_finite() || a0 == 0.0 {
    return [1.0, 0.0, 0.0, 0.0, 0.0];
  }
  [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
}

fn apply_biquad(
  sample: f32,
  coeffs: [f64; 5],
  memory: &mut BiquadMemory,
) -> f32 {
  let x = f64::from(sample);
  let y = coeffs[0] * x
    + coeffs[1] * f64::from(memory.x1)
    + coeffs[2] * f64::from(memory.x2)
    - coeffs[3] * f64::from(memory.y1)
    - coeffs[4] * f64::from(memory.y2);
  memory.x2 = memory.x1;
  memory.x1 = sample;
  let y = if y.is_finite() { y as f32 } else { 0.0 };
  memory.y2 = memory.y1;
  memory.y1 = y;
  y
}

fn response_sample(coeffs: [f64; 5], omega: f64) -> (f32, f32) {
  let (z1_re, z1_im) = (omega.cos(), -omega.sin());
  let (z2_re, z2_im) = ((2.0 * omega).cos(), -(2.0 * omega).sin());
  let num_re = coeffs[0] + coeffs[1] * z1_re + coeffs[2] * z2_re;
  let num_im = coeffs[1] * z1_im + coeffs[2] * z2_im;
  let den_re = 1.0 + coeffs[3] * z1_re + coeffs[4] * z2_re;
  let den_im = coeffs[3] * z1_im + coeffs[4] * z2_im;
  complex_response(num_re, num_im, den_re, den_im)
}

fn complex_response(
  num_re: f64,
  num_im: f64,
  den_re: f64,
  den_im: f64,
) -> (f32, f32) {
  let den = den_re * den_re + den_im * den_im;
  if den == 0.0 || !den.is_finite() {
    return (f32::NAN, f32::NAN);
  }
  let real = (num_re * den_re + num_im * den_im) / den;
  let imag = (num_im * den_re - num_re * den_im) / den;
  let magnitude = (real * real + imag * imag).sqrt();
  if !magnitude.is_finite() || !real.is_finite() || !imag.is_finite() {
    return (f32::NAN, f32::NAN);
  }
  (magnitude as f32, imag.atan2(real) as f32)
}

fn empty_iir(feedforward: &[f32], feedback: &[f32]) -> IirMemory {
  IirMemory {
    x: vec![0.0; feedforward.len().saturating_sub(1)],
    y: vec![0.0; feedback.len().saturating_sub(1)],
  }
}

fn fit_iir(memory: &mut IirMemory, feedforward: &[f32], feedback: &[f32]) {
  let x_len = feedforward.len().saturating_sub(1);
  let y_len = feedback.len().saturating_sub(1);
  if memory.x.len() != x_len {
    memory.x = vec![0.0; x_len];
  }
  if memory.y.len() != y_len {
    memory.y = vec![0.0; y_len];
  }
}

fn iir_sample(
  sample: f32,
  feedforward: &[f32],
  feedback: &[f32],
  memory: &mut IirMemory,
) -> f32 {
  let mut value =
    f64::from(feedforward.first().copied().unwrap_or(0.0)) * f64::from(sample);
  for (index, coef) in feedforward.iter().skip(1).enumerate() {
    value +=
      f64::from(*coef) * f64::from(memory.x.get(index).copied().unwrap_or(0.0));
  }
  for (index, coef) in feedback.iter().skip(1).enumerate() {
    value -=
      f64::from(*coef) * f64::from(memory.y.get(index).copied().unwrap_or(0.0));
  }
  let y = if value.is_finite() { value as f32 } else { 0.0 };
  if !memory.x.is_empty() {
    memory.x.rotate_right(1);
    memory.x[0] = sample;
  }
  if !memory.y.is_empty() {
    memory.y.rotate_right(1);
    memory.y[0] = y;
  }
  y
}

fn shape_oversampled(
  previous: f32,
  sample: f32,
  curve: Option<&[f32]>,
  steps: u32,
) -> f32 {
  let Some(curve) = curve.filter(|curve| curve.len() >= 2) else {
    return sample;
  };
  let mut sum = 0.0;
  for step in 1..=steps {
    let t = step as f32 / steps as f32;
    let point = previous + (sample - previous) * t;
    sum += shape_sample(point, curve);
  }
  sum / steps as f32
}

fn stereo_gains(pan: f64, stereo: bool) -> (f32, f32, bool) {
  let x = if !stereo {
    (pan + 1.0) / 2.0
  } else if pan <= 0.0 {
    pan + 1.0
  } else {
    pan
  };
  let gain_l = (x * std::f64::consts::FRAC_PI_2).cos() as f32;
  let gain_r = (x * std::f64::consts::FRAC_PI_2).sin() as f32;
  (gain_l, gain_r, pan <= 0.0)
}

fn panner_gains(azimuth: f64, stereo: bool) -> (f32, f32, bool) {
  let x = if !stereo {
    (azimuth + 90.0) / 180.0
  } else if azimuth <= 0.0 {
    (azimuth + 90.0) / 90.0
  } else {
    azimuth / 90.0
  };
  let gain_l = (x * std::f64::consts::FRAC_PI_2).cos() as f32;
  let gain_r = (x * std::f64::consts::FRAC_PI_2).sin() as f32;
  (gain_l, gain_r, azimuth <= 0.0)
}

fn apply_pan(
  left: f32,
  right: f32,
  gain_l: f32,
  gain_r: f32,
  stereo: bool,
  negative: bool,
) -> (f32, f32) {
  if !stereo {
    return (left * gain_l, left * gain_r);
  }
  if negative {
    (left + right * gain_l, right * gain_r)
  } else {
    (left * gain_l, right + left * gain_r)
  }
}

fn wrap_azimuth(azimuth: f64) -> f64 {
  let mut azimuth = azimuth.clamp(-180.0, 180.0);
  if azimuth < -90.0 {
    azimuth = -180.0 - azimuth;
  } else if azimuth > 90.0 {
    azimuth = 180.0 - azimuth;
  }
  azimuth
}

#[allow(clippy::needless_range_loop)]
fn convolve_each(
  input: &[Vec<f32>],
  kernel: &[f32],
  history: &mut Vec<Vec<f32>>,
  quantum: usize,
) -> Vec<Vec<f32>> {
  let channels = input.len().max(1);
  prepare_paths(history, channels);
  for slot in history.iter_mut() {
    prepare_kernel(slot, kernel.len());
  }
  let mut output = vec![vec![0.0; quantum]; channels];
  for channel in 0..channels {
    for index in 0..quantum {
      let sample = channel_sample(input, channel, index);
      output[channel][index] =
        convolve_sample(sample, kernel, &mut history[channel]);
    }
  }
  output
}

fn prepare_paths(history: &mut Vec<Vec<f32>>, paths: usize) {
  if history.len() != paths {
    history.clear();
    history.resize(paths, Vec::new());
  }
}

fn prepare_kernel(history: &mut Vec<f32>, kernel_len: usize) {
  let need = kernel_len.saturating_sub(1);
  if history.len() != need {
    history.clear();
    history.resize(need, 0.0);
  }
}

fn convolve_sample(sample: f32, kernel: &[f32], history: &mut [f32]) -> f32 {
  if kernel.is_empty() {
    return 0.0;
  }
  let mut acc = f64::from(sample) * f64::from(kernel[0]);
  for (lag, coef) in kernel.iter().enumerate().skip(1) {
    let index = history.len().saturating_sub(lag);
    acc +=
      f64::from(history.get(index).copied().unwrap_or(0.0)) * f64::from(*coef);
  }
  if !history.is_empty() {
    history.rotate_left(1);
    if let Some(last) = history.last_mut() {
      *last = sample;
    }
  }
  if acc.is_finite() { acc as f32 } else { 0.0 }
}

fn hermite(t: f64, y0: f64, y1: f64, m0: f64, m1: f64) -> f64 {
  let t2 = t * t;
  let t3 = t2 * t;
  (2.0 * t3 - 3.0 * t2 + 1.0) * y0
    + (t3 - 2.0 * t2 + t) * m0
    + (-2.0 * t3 + 3.0 * t2) * y1
    + (t3 - t2) * m1
}

fn bit_reverse(input: &[f32]) -> (Vec<f64>, Vec<f64>) {
  let width = input.len();
  let bits = width.trailing_zeros();
  let mut real = vec![0.0; width];
  let imag = vec![0.0; width];
  for (index, sample) in input.iter().enumerate() {
    real[reverse_bits(index, bits)] = f64::from(*sample);
  }
  (real, imag)
}

fn reverse_bits(mut index: usize, bits: u32) -> usize {
  let mut rev = 0;
  for _ in 0..bits {
    rev = (rev << 1) | (index & 1);
    index >>= 1;
  }
  rev
}

fn sample_vec(channels: [&[f64]; 3], index: usize) -> [f64; 3] {
  [
    channels[0].get(index).copied().unwrap_or(0.0),
    channels[1].get(index).copied().unwrap_or(0.0),
    channels[2].get(index).copied().unwrap_or(0.0),
  ]
}

fn block_len(input: &[Vec<f32>], fallback: usize) -> usize {
  input
    .iter()
    .map(|channel| channel.len())
    .max()
    .unwrap_or(fallback)
}

fn channel_sample(input: &[Vec<f32>], channel: usize, index: usize) -> f32 {
  input
    .get(channel)
    .and_then(|samples| samples.get(index))
    .copied()
    .unwrap_or(0.0)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(value: [f64; 3], factor: f64) -> [f64; 3] {
  [value[0] * factor, value[1] * factor, value[2] * factor]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
  a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ]
}

fn length(value: [f64; 3]) -> f64 {
  dot(value, value).sqrt()
}

fn normalized(value: [f64; 3]) -> Option<[f64; 3]> {
  let mag = length(value);
  if mag == 0.0 {
    None
  } else {
    Some(scale(value, 1.0 / mag))
  }
}

fn clamp_unit(value: f64) -> f64 {
  value.clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
  use super::{
    BiquadKind, BiquadMemory, CompressorState, DelayState, DistanceModel,
    HrtfState, IirError, IirMemory, ShapeState, analyser_process,
    analyser_reset, automatic_makeup_linear, azimuth_elevation, biquad_block,
    biquad_response_at, blackman, byte_time_sample, compress_block,
    compression_curve, cone_gain_of, convolve_block, db_to_lin, delay_block,
    distance_gain, downmix_mono, fft_magnitudes, hrtf_block, iir_block,
    mix_speakers, normalize_iir, panner_block, shape_block, shape_sample,
    stereo_pan_block, time_domain,
  };

  fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
  }

  fn impulse(length: usize) -> Vec<Vec<f32>> {
    let mut channel = vec![0.0; length];
    channel[0] = 1.0;
    vec![channel]
  }

  #[test]
  fn delay_zero_is_identity_and_ten_samples_lands_the_impulse() {
    let input = impulse(16);
    let mut state = DelayState {
      lines: Vec::new(),
      write: 0,
    };
    let same = delay_block(&input, &[0.0; 16], 32, 0.0, &mut state);
    near(same[0][0], 1.0);
    near(same[0][1], 0.0);
    let mut state = DelayState {
      lines: Vec::new(),
      write: 0,
    };
    let delayed = delay_block(&input, &[10.0; 16], 32, 0.0, &mut state);
    near(delayed[0][9], 0.0);
    near(delayed[0][10], 1.0);
  }

  #[test]
  fn azimuth_matches_the_listener_axes() {
    let forward = [0.0, 0.0, -1.0];
    let up = [0.0, 1.0, 0.0];
    let origin = [0.0, 0.0, 0.0];
    let (right, _) = azimuth_elevation([1.0, 0.0, 0.0], origin, forward, up);
    let (left, _) = azimuth_elevation([-1.0, 0.0, 0.0], origin, forward, up);
    let (ahead, _) = azimuth_elevation([0.0, 0.0, -1.0], origin, forward, up);
    let (above, elevation) =
      azimuth_elevation([0.0, 1.0, 0.0], origin, forward, up);
    assert!((right - 90.0).abs() < 1e-6, "{right}");
    assert!((left + 90.0).abs() < 1e-6, "{left}");
    assert!(ahead.abs() < 1e-6, "{ahead}");
    assert!(above.abs() < 1e-6 && (elevation - 90.0).abs() < 1e-6);
    let (same, _) = azimuth_elevation(origin, origin, forward, up);
    assert_eq!(same, 0.0);
  }

  #[test]
  fn equal_power_places_a_mono_source() {
    let input = vec![vec![1.0; 1]];
    let zero = vec![0.0; 1];
    let one = vec![1.0; 1];
    let forward_x = vec![0.0; 1];
    let forward_y = vec![0.0; 1];
    let forward_z = vec![-1.0; 1];
    let up_x = vec![0.0; 1];
    let up_y = vec![1.0; 1];
    let up_z = vec![0.0; 1];
    let right = panner_block(
      &input,
      [&one, &zero, &zero],
      [&one, &zero, &zero],
      [&zero, &zero, &zero],
      [&forward_x, &forward_y, &forward_z],
      [&up_x, &up_y, &up_z],
      DistanceModel::Inverse,
      1.0,
      10000.0,
      1.0,
      360.0,
      360.0,
      0.0,
    );
    near(right[0][0], 0.0);
    near(right[1][0], 1.0);
    let left = panner_block(
      &input,
      [&[-1.0], &zero, &zero],
      [&one, &zero, &zero],
      [&zero, &zero, &zero],
      [&forward_x, &forward_y, &forward_z],
      [&up_x, &up_y, &up_z],
      DistanceModel::Inverse,
      1.0,
      10000.0,
      1.0,
      360.0,
      360.0,
      0.0,
    );
    near(left[0][0], 1.0);
    near(left[1][0], 0.0);
    let center = panner_block(
      &input,
      [&zero, &zero, &zero],
      [&one, &zero, &zero],
      [&zero, &zero, &zero],
      [&forward_x, &forward_y, &forward_z],
      [&up_x, &up_y, &up_z],
      DistanceModel::Inverse,
      1.0,
      10000.0,
      1.0,
      360.0,
      360.0,
      0.0,
    );
    let half = std::f32::consts::FRAC_1_SQRT_2;
    near(center[0][0], half);
    near(center[1][0], half);
    assert!(
      (distance_gain(DistanceModel::Inverse, 0.0, 1.0, 10000.0, 1.0) - 1.0)
        .abs()
        < 1e-9
    );
  }

  fn peak_index(channel: &[f32]) -> usize {
    let mut index = 0;
    let mut peak = 0.0f32;
    for (frame, sample) in channel.iter().enumerate() {
      let value = sample.abs();
      if value > peak {
        peak = value;
        index = frame;
      }
    }
    assert!(peak > 0.05, "{peak}");
    index
  }

  fn side_hrtf(input: &[Vec<f32>], state: &mut HrtfState) -> Vec<Vec<f32>> {
    let frames = input[0].len();
    let zero = vec![0.0; frames];
    let one = vec![1.0; frames];
    let forward_z = vec![-1.0; frames];
    let up_y = vec![1.0; frames];
    hrtf_block(
      input,
      [&one, &zero, &zero],
      [&one, &zero, &zero],
      [&zero, &zero, &zero],
      [&zero, &zero, &forward_z],
      [&zero, &up_y, &zero],
      DistanceModel::Inverse,
      1.0,
      10000.0,
      1.0,
      360.0,
      360.0,
      0.0,
      state,
    )
  }

  #[test]
  fn hrtf_side_click_peaks_on_different_frames() {
    let mut state = HrtfState::default();
    let block = side_hrtf(&impulse(32), &mut state);
    let left = peak_index(&block[0]);
    let right = peak_index(&block[1]);
    assert_ne!(left, right);
    assert_eq!(left.min(right), 0);

    let mut first = vec![vec![0.0; 8]];
    first[0][7] = 1.0;
    let mut state = HrtfState::default();
    let head = side_hrtf(&first, &mut state);
    let tail = side_hrtf(&[vec![0.0; 24]], &mut state);
    let mut left_ch = head[0].clone();
    left_ch.extend_from_slice(&tail[0]);
    let mut right_ch = head[1].clone();
    right_ch.extend_from_slice(&tail[1]);
    let left_at = peak_index(&left_ch);
    let right_at = peak_index(&right_ch);
    assert_ne!(left_at, right_at);
    assert_eq!(left_at.min(right_at), 7);
  }

  #[test]
  fn stereo_pan_sends_mono_hard_left_and_centers() {
    let input = vec![vec![1.0]];
    let left = stereo_pan_block(&input, &[-1.0]);
    near(left[0][0], 1.0);
    near(left[1][0], 0.0);
    let center = stereo_pan_block(&input, &[0.0]);
    let half = std::f32::consts::FRAC_1_SQRT_2;
    near(center[0][0], half);
    near(center[1][0], half);
    let stereo = stereo_pan_block(&[vec![0.25], vec![0.75]], &[0.0]);
    near(stereo[0][0], 0.25);
    near(stereo[1][0], 0.75);
  }

  #[test]
  fn cone_points_toward_the_listener() {
    let gain = cone_gain_of(
      [1.0, 0.0, 0.0],
      [0.0, 0.0, 0.0],
      [1.0, 0.0, 0.0],
      0.0,
      180.0,
      0.0,
    );
    assert!((gain - 1.0).abs() < 1e-9);
    let side = cone_gain_of(
      [1.0, 0.0, 0.0],
      [0.0, 0.0, 0.0],
      [1.0, 1.0, 0.0],
      0.0,
      180.0,
      0.0,
    );
    assert!((side - 0.5).abs() < 1e-6, "{side}");
  }

  #[test]
  fn biquad_dc_special_cases_and_lowpass_response() {
    let input = vec![vec![1.0; 8]];
    let mut memory = vec![BiquadMemory::default()];
    let low = biquad_block(
      &input,
      BiquadKind::Lowpass,
      &[0.0; 8],
      &[0.0; 8],
      &[1.0; 8],
      &[0.0; 8],
      48000.0,
      &mut memory,
    );
    near(low[0][7], 0.0);
    let mut memory = vec![BiquadMemory::default()];
    let high = biquad_block(
      &input,
      BiquadKind::Highpass,
      &[0.0; 8],
      &[0.0; 8],
      &[1.0; 8],
      &[0.0; 8],
      48000.0,
      &mut memory,
    );
    near(high[0][7], 1.0);
    let mut memory = vec![BiquadMemory::default()];
    let shelf = biquad_block(
      &input,
      BiquadKind::Lowshelf,
      &[0.0; 8],
      &[0.0; 8],
      &[10.0; 8],
      &[10.0; 8],
      48000.0,
      &mut memory,
    );
    near(shelf[0][0], 1.0);
    let mut memory = vec![BiquadMemory::default()];
    let nyquist_shelf = biquad_block(
      &input,
      BiquadKind::Lowshelf,
      &[24000.0; 8],
      &[0.0; 8],
      &[10.0; 8],
      &[10.0; 8],
      48000.0,
      &mut memory,
    );
    near(nyquist_shelf[0][0], 10f32.powf(0.5));
    let mut memory = vec![BiquadMemory::default()];
    let allpass = biquad_block(
      &input,
      BiquadKind::Allpass,
      &[12000.0; 8],
      &[0.0; 8],
      &[0.0; 8],
      &[0.0; 8],
      48000.0,
      &mut memory,
    );
    near(allpass[0][0], -1.0);
    let (magnitude, _) = biquad_response_at(
      BiquadKind::Lowpass,
      350.0,
      0.0,
      1.0,
      0.0,
      48000.0,
      0.0,
    );
    near(magnitude, 1.0);
    let (nyquist, _) = biquad_response_at(
      BiquadKind::Lowpass,
      350.0,
      0.0,
      1.0,
      0.0,
      48000.0,
      24000.0,
    );
    assert!(nyquist.abs() < 1e-3, "{nyquist}");
  }

  #[test]
  fn speaker_layouts_follow_the_canonical_mix() {
    let mono = vec![vec![1.0]];
    let mut stereo = vec![vec![0.0], vec![0.0]];
    assert!(mix_speakers(&mono, &mut stereo, 1));
    near(stereo[0][0], 1.0);
    near(stereo[1][0], 1.0);

    let mut surround = vec![vec![0.0]; 6];
    assert!(mix_speakers(&mono, &mut surround, 1));
    near(surround[2][0], 1.0);
    near(surround[0][0], 0.0);

    let three = vec![vec![1.0], vec![2.0], vec![3.0]];
    let mut one = vec![vec![0.0]];
    assert!(!mix_speakers(&three, &mut one, 1));
    near(downmix_mono(&three, 1)[0], 1.0);

    let six = vec![
      vec![1.0],
      vec![2.0],
      vec![3.0],
      vec![4.0],
      vec![5.0],
      vec![6.0],
    ];
    let mut mixed = vec![vec![0.0]];
    assert!(mix_speakers(&six, &mut mixed, 1));
    let half = std::f32::consts::FRAC_1_SQRT_2;
    near(mixed[0][0], half * (1.0 + 2.0) + 3.0 + 0.5 * (5.0 + 6.0));
    near(downmix_mono(&[vec![1.0], vec![3.0]], 1)[0], 2.0);
  }

  #[test]
  fn iir_half_and_rejected_coefficients() {
    let input = vec![vec![1.0; 4]];
    let mut memory = vec![IirMemory {
      x: Vec::new(),
      y: Vec::new(),
    }];
    let (b, a) = normalize_iir(vec![0.5], vec![1.0]).unwrap();
    let output = iir_block(&input, &b, &a, &mut memory);
    near(output[0][0], 0.5);
    near(output[0][3], 0.5);
    assert!(matches!(
      normalize_iir(vec![0.0], vec![1.0]),
      Err(IirError::Feedforward)
    ));
    assert!(matches!(
      normalize_iir(vec![1.0], vec![0.0]),
      Err(IirError::Feedback)
    ));
    assert!(matches!(
      normalize_iir(vec![1.0; 21], vec![1.0]),
      Err(IirError::Length)
    ));
  }

  #[test]
  fn waveshaper_interpolates_the_curve() {
    let curve = [0.0, 1.0];
    near(shape_sample(0.0, &curve), 0.5);
    near(shape_sample(-1.0, &curve), 0.0);
    near(shape_sample(1.0, &curve), 1.0);
    let mut state = ShapeState::default();
    let output = shape_block(&[vec![0.0]], Some(&curve), 1, &mut state);
    near(output[0][0], 0.5);
  }

  #[test]
  fn convolver_reproduces_a_short_impulse_response() {
    let mut history = Vec::new();
    let output =
      convolve_block(&impulse(4), &[vec![1.0, 0.0, 0.5]], &mut history, 4);
    near(output[0][0], 1.0);
    near(output[0][1], 0.0);
    near(output[0][2], 0.5);
  }

  #[test]
  fn hard_knee_settles_and_ratio_one_is_identity() {
    let threshold = db_to_lin(-24.0);
    let shaped = compression_curve(1.0, -24.0, 0.0, 20.0);
    let expected = threshold + (1.0 - threshold) / 20.0;
    assert!((shaped - expected).abs() < 1e-9);
    let input = vec![vec![1.0; 128]];
    let mut state = CompressorState::default();
    let output = compress_block(
      &input, -24.0, 0.0, 20.0, 0.0, 0.0, 1.0, 48000.0, &mut state,
    );
    near(output[0][0], expected as f32);
    near(output[0][127], expected as f32);
    let quiet = vec![vec![0.5; 4]];
    let mut state = CompressorState::default();
    let identity =
      compress_block(&quiet, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 48000.0, &mut state);
    near(identity[0][3], 0.5);
    let knee_end = db_to_lin(-24.0 + 10.0);
    near(
      compression_curve(threshold, -24.0, 10.0, 20.0) as f32,
      threshold as f32,
    );
    let end = threshold + (knee_end - threshold) / 20.0;
    near(
      compression_curve(knee_end, -24.0, 10.0, 20.0) as f32,
      end as f32,
    );
    assert!(automatic_makeup_linear(-24.0, 0.0, 20.0) > 1.0);
  }

  #[test]
  fn analyser_orders_the_ring_and_an_impulse_has_flat_magnitude() {
    let mut state = analyser_reset(2048);
    analyser_process(&mut state, &[0.5; 128], 0.0);
    let time = time_domain(&state.ring, state.write, state.count);
    assert!(time[..2048 - 128].iter().all(|sample| *sample == 0.0));
    assert!(time[2048 - 128..].iter().all(|sample| *sample == 0.5));
    assert_eq!(byte_time_sample(0.0), 128);
    assert_eq!(byte_time_sample(1.0), 255);
    let mut impulse = vec![0.0; 32];
    impulse[0] = 1.0;
    let magnitudes = fft_magnitudes(&impulse);
    for magnitude in magnitudes {
      near(magnitude, 1.0 / 32.0);
    }
    let window = blackman(32);
    assert_eq!(window.len(), 32);
    assert!(window[0].abs() < 1e-6);
    assert!((window[16] - 1.0).abs() < 1e-4);
  }
}
