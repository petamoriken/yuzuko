//! Realtime playback for `AudioContext`.
//!
//! The graph renders on the isolate thread. The device callback only copies
//! the ring and must not call V8, and it must not notify while holding the
//! ring mutex. `YUZUKO_AUDIO_TIMER` skips the device so the timer fallback
//! can run on a machine that has a speaker.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const HIGH_SECONDS: f64 = 0.1;
const LOW_SECONDS: f64 = 0.02;
const OUTPUT_LATENCY: f64 = HIGH_SECONDS;

pub(crate) enum Pace {
  Immediate,
  Wait,
  Timer,
}

struct Ring {
  data: Vec<f32>,
  channels: usize,
  capacity: usize,
  read: usize,
  filled: usize,
}

impl Ring {
  fn new(capacity: usize, channels: usize) -> Self {
    let capacity = capacity.max(1);
    Self {
      data: vec![0.0; capacity * channels],
      channels,
      capacity,
      read: 0,
      filled: 0,
    }
  }

  fn push(&mut self, interleaved: &[f32]) {
    let channels = self.channels;
    if channels == 0 {
      return;
    }
    let mut frames = interleaved.len() / channels;
    let free = self.capacity.saturating_sub(self.filled);
    if frames > free {
      frames = free;
    }
    for frame in 0..frames {
      let dest = (self.read + self.filled) % self.capacity;
      let src = frame * channels;
      let base = dest * channels;
      self.data[base..base + channels]
        .copy_from_slice(&interleaved[src..src + channels]);
      self.filled += 1;
    }
  }

  fn pop_frame(&mut self, out: &mut [f32]) -> bool {
    let channels = self.channels;
    if self.filled == 0 || out.len() < channels || channels == 0 {
      return false;
    }
    let base = self.read * channels;
    out[..channels].copy_from_slice(&self.data[base..base + channels]);
    self.read = (self.read + 1) % self.capacity;
    self.filled -= 1;
    true
  }
}

struct Resampler {
  tail: Vec<f32>,
  cursor: f64,
}

impl Resampler {
  fn new() -> Self {
    Self {
      tail: Vec::new(),
      cursor: 0.0,
    }
  }
}

struct Device {
  stream: cpal::Stream,
  ring: Arc<Mutex<Ring>>,
  wake: Arc<Mutex<Option<yuzuko_core::Listen>>>,
  failed: Arc<AtomicBool>,
  playing: bool,
  primed: bool,
  channels: usize,
  rate: u32,
  high: usize,
  low: usize,
}

struct Play {
  device: Option<Device>,
  latency: f64,
  resampler: Resampler,
  timer: Option<i32>,
}

std::thread_local! {
  static PLAY: std::cell::RefCell<std::collections::HashMap<u64, Play>> =
    std::cell::RefCell::new(std::collections::HashMap::new());
}

pub(crate) fn clear() {
  PLAY.with(|slot| slot.borrow_mut().clear());
}

pub(crate) fn output_latency(id: u64) -> f64 {
  PLAY.with(|slot| {
    slot
      .borrow()
      .get(&id)
      .map(|play| play.latency)
      .unwrap_or(0.0)
  })
}

pub(crate) fn ensure(id: u64, rate: f64, quantum: u32) {
  PLAY.with(|slot| {
    let mut plays = slot.borrow_mut();
    if plays.contains_key(&id) {
      return;
    }
    let device = if timer_only() {
      None
    } else {
      open_device(rate, quantum)
    };
    let latency = if device.is_some() {
      OUTPUT_LATENCY
    } else {
      0.0
    };
    plays.insert(
      id,
      Play {
        device,
        latency,
        resampler: Resampler::new(),
        timer: None,
      },
    );
  });
}

pub(crate) fn submit(
  id: u64,
  block: &[Vec<f32>],
  quantum: usize,
  rate: f64,
) -> Pace {
  let failed = PLAY.with(|slot| {
    let mut plays = slot.borrow_mut();
    let Some(play) = plays.get_mut(&id) else {
      return false;
    };
    let Some(device) = play.device.as_ref() else {
      return false;
    };
    device.failed.load(Ordering::SeqCst)
  });
  if failed {
    retire_device(id);
    return Pace::Timer;
  }
  let pace = PLAY.with(|slot| {
    let mut plays = slot.borrow_mut();
    let Some(play) = plays.get_mut(&id) else {
      return Pace::Timer;
    };
    let Some(device) = play.device.as_mut() else {
      return Pace::Timer;
    };
    let interleaved = convert(
      &mut play.resampler,
      block,
      quantum,
      rate,
      device.rate,
      device.channels,
    );
    let filled = {
      let mut ring = lock_ring(&device.ring);
      ring.push(&interleaved);
      ring.filled
    };
    if filled >= device.high && !device.playing {
      match device.stream.play() {
        Ok(()) => {
          device.playing = true;
          device.primed = true;
        }
        Err(_) => return Pace::Timer,
      }
    }
    if device.failed.load(Ordering::SeqCst) {
      return Pace::Timer;
    }
    if filled < device.high {
      Pace::Immediate
    } else {
      Pace::Wait
    }
  });
  if matches!(pace, Pace::Timer) {
    retire_device(id);
  }
  pace
}

pub(crate) fn pause(id: u64) {
  cancel_timer(id);
  PLAY.with(|slot| {
    let mut plays = slot.borrow_mut();
    let Some(play) = plays.get_mut(&id) else {
      return;
    };
    let Some(device) = play.device.as_mut() else {
      return;
    };
    take_listen(&device.wake);
    if device.playing {
      let _ = device.stream.pause();
      device.playing = false;
    }
  });
}

pub(crate) fn resume(id: u64) {
  let failed = PLAY.with(|slot| {
    let mut plays = slot.borrow_mut();
    let Some(play) = plays.get_mut(&id) else {
      return false;
    };
    let Some(device) = play.device.as_mut() else {
      return false;
    };
    if !device.primed {
      return false;
    }
    match device.stream.play() {
      Ok(()) => {
        device.playing = true;
        false
      }
      Err(_) => true,
    }
  });
  if failed {
    retire_device(id);
  }
}

pub(crate) fn stop(id: u64) {
  cancel_timer(id);
  PLAY.with(|slot| {
    slot.borrow_mut().remove(&id);
  });
}

pub(crate) fn arm_timer(id: u64, timer: i32) {
  let previous = PLAY.with(|slot| {
    let mut plays = slot.borrow_mut();
    let Some(play) = plays.get_mut(&id) else {
      return Some(timer);
    };
    play.timer.replace(timer)
  });
  if let Some(previous) = previous {
    yuzuko_core::cancel_host(previous);
  }
}

pub(crate) fn arm_listen(id: u64, listen: yuzuko_core::Listen) {
  let slot = PLAY.with(|plays| {
    let plays = plays.borrow();
    let device = plays.get(&id).and_then(|play| play.device.as_ref())?;
    Some((
      Arc::clone(&device.wake),
      Arc::clone(&device.ring),
      Arc::clone(&device.failed),
      device.low,
    ))
  });
  let Some((wake, ring, failed, low)) = slot else {
    drop(listen);
    return;
  };
  {
    let mut slot = lock_wake(&wake);
    *slot = Some(listen);
  }
  let below = lock_ring(&ring).filled < low;
  if below || failed.load(Ordering::SeqCst) {
    notify(&wake);
  }
}

fn cancel_timer(id: u64) {
  let timer = PLAY.with(|slot| {
    slot
      .borrow_mut()
      .get_mut(&id)
      .and_then(|play| play.timer.take())
  });
  if let Some(timer) = timer {
    yuzuko_core::cancel_host(timer);
  }
}

fn retire_device(id: u64) {
  PLAY.with(|slot| {
    let mut plays = slot.borrow_mut();
    let Some(play) = plays.get_mut(&id) else {
      return;
    };
    play.device = None;
    play.latency = 0.0;
    play.resampler = Resampler::new();
  });
}

fn timer_only() -> bool {
  std::env::var_os("YUZUKO_AUDIO_TIMER").is_some()
}

fn open_device(rate: f64, quantum: u32) -> Option<Device> {
  let opened = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    try_open(rate, quantum)
  }));
  match opened {
    Ok(device) => device,
    Err(_) => None,
  }
}

fn try_open(rate: f64, quantum: u32) -> Option<Device> {
  let host = cpal::default_host();
  let device = host.default_output_device()?;
  let hz = rate.round().clamp(1.0, 1_000_000.0) as u32;
  let preferred = cpal::StreamConfig {
    channels: 2,
    sample_rate: hz,
    buffer_size: cpal::BufferSize::Default,
  };
  if let Some(opened) =
    build(&device, preferred, cpal::SampleFormat::F32, rate, quantum)
  {
    return Some(opened);
  }
  let supported = device.default_output_config().ok()?;
  if !convertible(supported.sample_format()) {
    return None;
  }
  let format = supported.sample_format();
  let config: cpal::StreamConfig = supported.into();
  build(&device, config, format, rate, quantum)
}

fn build(
  device: &cpal::Device,
  config: cpal::StreamConfig,
  format: cpal::SampleFormat,
  context_rate: f64,
  quantum: u32,
) -> Option<Device> {
  if !convertible(format) || config.channels == 0 {
    return None;
  }
  let channels = config.channels as usize;
  let rate = config.sample_rate;
  let high = frames_for(rate, HIGH_SECONDS).max(1);
  let low = frames_for(rate, LOW_SECONDS).clamp(1, high);
  let quantum_out = ((f64::from(quantum) * f64::from(rate) / context_rate)
    .ceil() as usize)
    .saturating_add(8);
  let capacity = high.saturating_add(quantum_out * 2).max(quantum_out + 8);
  let ring = Arc::new(Mutex::new(Ring::new(capacity, channels)));
  let wake = Arc::new(Mutex::new(None));
  let failed = Arc::new(AtomicBool::new(false));
  let ring_cb = Arc::clone(&ring);
  let wake_cb = Arc::clone(&wake);
  let wake_err = Arc::clone(&wake);
  let failed_cb = Arc::clone(&failed);
  let mut frame = vec![0.0f32; channels];
  let stream = device
    .build_output_stream_raw(
      config,
      format,
      move |data, _| {
        let below = {
          let mut ring = lock_ring(&ring_cb);
          write_output(data, &mut ring, &mut frame, low)
        };
        if below {
          notify(&wake_cb);
        }
      },
      move |_error| {
        failed_cb.store(true, Ordering::SeqCst);
        notify(&wake_err);
      },
      Some(Duration::from_secs(2)),
    )
    .ok()?;
  Some(Device {
    stream,
    ring,
    wake,
    failed,
    playing: false,
    primed: false,
    channels,
    rate,
    high,
    low,
  })
}

fn frames_for(rate: u32, seconds: f64) -> usize {
  (f64::from(rate) * seconds).round() as usize
}

fn convertible(format: cpal::SampleFormat) -> bool {
  matches!(
    format,
    cpal::SampleFormat::F32
      | cpal::SampleFormat::F64
      | cpal::SampleFormat::I16
      | cpal::SampleFormat::I32
      | cpal::SampleFormat::U16
      | cpal::SampleFormat::U8
  )
}

fn write_output(
  data: &mut cpal::Data,
  ring: &mut Ring,
  frame: &mut [f32],
  low: usize,
) -> bool {
  let channels = ring.channels;
  if channels == 0 || frame.len() < channels {
    data.bytes_mut().fill(0);
    return ring.filled < low;
  }
  let frames = data.len() / channels;
  if frames == 0 {
    data.bytes_mut().fill(0);
    return ring.filled < low;
  }
  let wrote = match data.sample_format() {
    cpal::SampleFormat::F32 => write_typed(data, ring, frame, |sample| sample),
    cpal::SampleFormat::F64 => {
      write_typed(data, ring, frame, |sample| f64::from(clamp_unit(sample)))
    }
    cpal::SampleFormat::I16 => write_typed(data, ring, frame, |sample| {
      (clamp_unit(sample) * f32::from(i16::MAX)) as i16
    }),
    cpal::SampleFormat::I32 => write_typed(data, ring, frame, |sample| {
      (f64::from(clamp_unit(sample)) * f64::from(i32::MAX)) as i32
    }),
    cpal::SampleFormat::U16 => write_typed(data, ring, frame, |sample| {
      ((clamp_unit(sample) * 0.5 + 0.5) * f32::from(u16::MAX)) as u16
    }),
    cpal::SampleFormat::U8 => write_typed(data, ring, frame, |sample| {
      ((clamp_unit(sample) * 0.5 + 0.5) * f32::from(u8::MAX)) as u8
    }),
    _ => false,
  };
  if !wrote {
    data.bytes_mut().fill(0);
  }
  ring.filled < low
}

fn write_typed<T, F>(
  data: &mut cpal::Data,
  ring: &mut Ring,
  frame: &mut [f32],
  convert: F,
) -> bool
where
  T: cpal::SizedSample,
  F: Fn(f32) -> T,
{
  let Some(out) = data.as_slice_mut::<T>() else {
    return false;
  };
  let channels = ring.channels;
  let mut index = 0;
  while index + channels <= out.len() {
    if ring.pop_frame(frame) {
      for channel in 0..channels {
        out[index + channel] = convert(frame[channel]);
      }
    } else {
      for sample in &mut out[index..] {
        *sample = T::EQUILIBRIUM;
      }
      break;
    }
    index += channels;
  }
  true
}

fn clamp_unit(sample: f32) -> f32 {
  sample.clamp(-1.0, 1.0)
}

fn notify(wake: &Mutex<Option<yuzuko_core::Listen>>) {
  let listen = lock_wake(wake).take();
  if let Some(listen) = listen {
    listen.notify();
  }
}

fn take_listen(wake: &Mutex<Option<yuzuko_core::Listen>>) {
  drop(lock_wake(wake).take());
}

fn lock_ring(ring: &Mutex<Ring>) -> std::sync::MutexGuard<'_, Ring> {
  ring.lock().unwrap_or_else(|error| error.into_inner())
}

fn lock_wake(
  wake: &Mutex<Option<yuzuko_core::Listen>>,
) -> std::sync::MutexGuard<'_, Option<yuzuko_core::Listen>> {
  wake.lock().unwrap_or_else(|error| error.into_inner())
}

fn convert(
  resampler: &mut Resampler,
  block: &[Vec<f32>],
  quantum: usize,
  in_rate: f64,
  out_rate: u32,
  out_channels: usize,
) -> Vec<f32> {
  let mixed = mix(block, quantum, out_channels);
  resample(
    resampler,
    &mixed,
    out_channels,
    in_rate,
    f64::from(out_rate),
  )
}

fn mix(block: &[Vec<f32>], quantum: usize, out_channels: usize) -> Vec<f32> {
  let mut out = vec![0.0; quantum * out_channels];
  if quantum == 0 || out_channels == 0 {
    return out;
  }
  let src = block.len();
  if src == 0 {
    return out;
  }
  if src == 1 {
    for frame in 0..quantum {
      let sample = block[0].get(frame).copied().unwrap_or(0.0);
      for channel in 0..out_channels {
        out[frame * out_channels + channel] = sample;
      }
    }
    return out;
  }
  if out_channels == 1 {
    for frame in 0..quantum {
      let mut sum = 0.0;
      for channel in block {
        sum += channel.get(frame).copied().unwrap_or(0.0);
      }
      out[frame] = sum / src as f32;
    }
    return out;
  }
  let shared = src.min(out_channels);
  for frame in 0..quantum {
    for channel in 0..shared {
      out[frame * out_channels + channel] =
        block[channel].get(frame).copied().unwrap_or(0.0);
    }
  }
  out
}

fn resample(
  state: &mut Resampler,
  input: &[f32],
  channels: usize,
  in_rate: f64,
  out_rate: f64,
) -> Vec<f32> {
  if channels == 0 || !in_rate.is_finite() || !out_rate.is_finite() {
    return Vec::new();
  }
  if (in_rate - out_rate).abs() < 0.5
    && state.tail.is_empty()
    && state.cursor == 0.0
  {
    return input.to_vec();
  }
  let mut source = Vec::with_capacity(state.tail.len() + input.len());
  source.extend_from_slice(&state.tail);
  source.extend_from_slice(input);
  let src_frames = source.len() / channels;
  if src_frames == 0 {
    return Vec::new();
  }
  let step = in_rate / out_rate;
  if !step.is_finite() || step <= 0.0 {
    state.tail.clear();
    state.cursor = 0.0;
    return Vec::new();
  }
  let mut out = Vec::new();
  let mut cursor = state.cursor;
  while cursor + 1.0 < src_frames as f64 {
    let index = cursor.floor() as usize;
    let frac = (cursor - index as f64) as f32;
    for channel in 0..channels {
      let left = source[index * channels + channel];
      let right = source[(index + 1) * channels + channel];
      out.push(left + (right - left) * frac);
    }
    cursor += step;
  }
  let keep = cursor.floor() as usize;
  state.cursor = cursor - keep as f64;
  let start = keep * channels;
  if start < source.len() {
    state.tail = source[start..].to_vec();
  } else {
    state.tail.clear();
    state.cursor = 0.0;
  }
  out
}

#[cfg(test)]
mod tests {
  use super::{Resampler, Ring, mix, resample};

  #[test]
  fn ring_keeps_frames_in_order() {
    let mut ring = Ring::new(2, 1);
    ring.push(&[1.0, 2.0, 3.0]);
    assert_eq!(ring.filled, 2);
    let mut frame = [0.0];
    assert!(ring.pop_frame(&mut frame));
    assert_eq!(frame[0], 1.0);
    assert!(ring.pop_frame(&mut frame));
    assert_eq!(frame[0], 2.0);
    assert!(!ring.pop_frame(&mut frame));
  }

  #[test]
  fn mix_duplicates_mono_and_averages_stereo() {
    let stereo = mix(&[vec![1.0, 2.0]], 2, 2);
    assert_eq!(stereo, vec![1.0, 1.0, 2.0, 2.0]);
    let mono = mix(&[vec![1.0, 3.0], vec![2.0, 4.0]], 2, 1);
    assert_eq!(mono, vec![1.5, 3.5]);
  }

  #[test]
  fn equal_rates_copy_samples() {
    let mut state = Resampler::new();
    let out =
      resample(&mut state, &[0.0, 1.0, 2.0, 3.0], 1, 48_000.0, 48_000.0);
    assert_eq!(out, vec![0.0, 1.0, 2.0, 3.0]);
  }

  #[test]
  fn double_rate_interpolates() {
    let mut state = Resampler::new();
    let out = resample(&mut state, &[0.0, 10.0, 20.0, 30.0], 1, 10.0, 20.0);
    assert_eq!(out, vec![0.0, 5.0, 10.0, 15.0, 20.0, 25.0]);
  }
}
