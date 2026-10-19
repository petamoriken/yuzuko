//! `ImageDecoder`, `ImageTrackList`, and `ImageTrack`.
//!
//! Still images use the same pure-Rust decoders as `createImageBitmap`.
//! An animated file contributes its first frame.

use std::cell::{Cell, RefCell};

use yuzuko_core::op::{Js, OpError, host_ref, new_dom_exception, traced};
use yuzuko_core::rust_string;
use yuzuko_image::{
  ImageDecodeError, PixelError, Raster, decode_image, orient,
};
use yuzuko_macros::op;
use yuzuko_winter::buffer_source;

use crate::classes::VideoFrame;

pub struct ImageDecoder {
  mime: String,
  to_srgb: bool,
  closed: Cell<bool>,
  selected: Cell<bool>,
  /// Every encoded byte has arrived. This is not "the decode finished".
  data_complete: Cell<bool>,
  ready: Cell<bool>,
  settled: Cell<bool>,
  generation: Cell<u64>,
  bytes: RefCell<Vec<u8>>,
  raster: RefCell<Option<Raster>>,
  error_name: RefCell<Option<&'static str>>,
  error_message: RefCell<Option<String>>,
  completed: RefCell<Option<v8::TracedReference<v8::Promise>>>,
  tracks_ready: RefCell<Option<v8::TracedReference<v8::Promise>>>,
  tracks: RefCell<Option<v8::TracedReference<v8::Object>>>,
  track: RefCell<Option<v8::TracedReference<v8::Object>>>,
  completed_resolver: RefCell<Option<v8::Global<v8::PromiseResolver>>>,
  tracks_resolver: RefCell<Option<v8::Global<v8::PromiseResolver>>>,
  waiters: RefCell<Vec<Waiter>>,
  stream: RefCell<Option<v8::Global<v8::Object>>>,
  self_object: RefCell<Option<v8::TracedReference<v8::Object>>>,
}

struct Waiter {
  index: u32,
  resolver: v8::Global<v8::PromiseResolver>,
}

unsafe impl v8::cppgc::GarbageCollected for ImageDecoder {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    trace_promise(&self.completed, visitor);
    trace_promise(&self.tracks_ready, visitor);
    trace_object(&self.tracks, visitor);
    trace_object(&self.track, visitor);
    trace_object(&self.self_object, visitor);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ImageDecoder"
  }
}

pub struct ImageTrackList {
  decoder: v8::TracedReference<v8::Object>,
}

unsafe impl v8::cppgc::GarbageCollected for ImageTrackList {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.decoder);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ImageTrackList"
  }
}

pub struct ImageTrack {
  decoder: v8::TracedReference<v8::Object>,
}

unsafe impl v8::cppgc::GarbageCollected for ImageTrack {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.decoder);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"ImageTrack"
  }
}

#[op]
impl ImageDecoder {
  #[constructor]
  #[length(1)]
  fn new(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Self, OpError> {
    let init = dictionary(scope, args.get(0))?;
    let mime = required_string(scope, init, "type")?;
    let Some(data) = member(scope, init, "data") else {
      return Err(OpError::type_("data is required"));
    };
    let data = yuzuko_core::op::root(scope, data);
    let to_srgb = color_space_conversion(scope, init)?;
    let _ = optional_bool(scope, init, "preferAnimation")?;
    let _ = optional_ulong(scope, init, "desiredWidth")?;
    let _ = optional_ulong(scope, init, "desiredHeight")?;
    if detached_source(scope, data) {
      return Err(OpError::type_("data is detached"));
    }
    let bytes = buffer_source(scope, data);
    let stream = if bytes.is_none() && is_readable_stream(scope, data) {
      let object = v8::Local::<v8::Object>::try_from(data)
        .map_err(|_| OpError::type_("data is not a ReadableStream"))?;
      Some(v8::Global::new(scope, object))
    } else if bytes.is_none() {
      return Err(OpError::type_(
        "data is not a BufferSource or ReadableStream",
      ));
    } else {
      None
    };
    transfer_buffers(scope, init)?;
    let supported = supported_type(&mime);
    Ok(Self {
      mime,
      to_srgb,
      closed: Cell::new(false),
      selected: Cell::new(true),
      data_complete: Cell::new(bytes.is_some()),
      ready: Cell::new(false),
      settled: Cell::new(false),
      generation: Cell::new(0),
      bytes: RefCell::new(bytes.unwrap_or_default()),
      raster: RefCell::new(None),
      error_name: RefCell::new(if supported {
        None
      } else {
        Some("NotSupportedError")
      }),
      error_message: RefCell::new(if supported {
        None
      } else {
        Some("The image type is not supported".to_string())
      }),
      completed: RefCell::new(None),
      tracks_ready: RefCell::new(None),
      tracks: RefCell::new(None),
      track: RefCell::new(None),
      completed_resolver: RefCell::new(None),
      tracks_resolver: RefCell::new(None),
      waiters: RefCell::new(Vec::new()),
      stream: RefCell::new(stream),
      self_object: RefCell::new(None),
    })
  }

  #[construct_hook]
  fn finish(
    &self,
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
  ) -> Result<(), OpError> {
    let completed = promise_pair(scope)?;
    let tracks_ready = promise_pair(scope)?;
    *self.completed.borrow_mut() = Some(traced(scope, completed.0));
    *self.completed_resolver.borrow_mut() =
      Some(v8::Global::new(scope, completed.1));
    *self.tracks_ready.borrow_mut() = Some(traced(scope, tracks_ready.0));
    *self.tracks_resolver.borrow_mut() =
      Some(v8::Global::new(scope, tracks_ready.1));

    let list_host = ImageTrackList::new(scope, object);
    let list = unsafe { yuzuko_core::op::wrap_instance(scope, list_host) };
    let track_host = ImageTrack::new(scope, object);
    let track = unsafe { yuzuko_core::op::wrap_instance(scope, track_host) };
    *self.tracks.borrow_mut() = Some(traced(scope, list));
    *self.track.borrow_mut() = Some(traced(scope, track));
    *self.self_object.borrow_mut() = Some(traced(scope, object));

    if self.error_name.borrow().is_some() {
      self.stream.borrow_mut().take();
      // Reject after this turn so the caller can attach to `completed`.
      defer(scope, object, on_deferred_settle);
      return Ok(());
    }
    if let Some(stream) = self.stream.borrow_mut().take() {
      let stream = v8::Local::new(scope, stream);
      pull_stream(scope, object, stream.into());
    } else {
      self.spawn_decode(scope, object);
    }
    Ok(())
  }

  #[getter]
  #[rename("type")]
  fn image_type(&self) -> String {
    self.mime.clone()
  }

  #[getter]
  fn complete(&self) -> bool {
    self.data_complete.get()
  }

  #[getter]
  fn completed(&self, scope: &mut v8::PinScope) -> Js {
    stored_promise(scope, &self.completed)
  }

  #[getter]
  fn tracks(&self, scope: &mut v8::PinScope) -> Js {
    stored_object(scope, &self.tracks)
  }

  fn decode(
    &self,
    scope: &mut v8::PinScope,
    options: Option<v8::Local<v8::Value>>,
  ) -> Js {
    let index = match frame_index(scope, options) {
      Ok(index) => index,
      Err(error) => return rejected_error(scope, error),
    };
    if self.closed.get() {
      return rejected_error(scope, closed_error());
    }
    if let Some(name) = *self.error_name.borrow() {
      let message = self.error_message.borrow().clone().unwrap_or_default();
      return rejected_error(scope, OpError::dom(name, message));
    }
    if self.ready.get() {
      return self.finish_decode(scope, index);
    }
    let Some(resolver) = v8::PromiseResolver::new(scope) else {
      return Js::null(scope);
    };
    let promise = resolver.get_promise(scope);
    self.waiters.borrow_mut().push(Waiter {
      index,
      resolver: v8::Global::new(scope, resolver),
    });
    let promise: v8::Local<v8::Value> = promise.into();
    Js(v8::Global::new(scope, promise))
  }

  #[static_method]
  #[length(1)]
  fn is_type_supported(scope: &mut v8::PinScope, mime: String) -> Js {
    let Some(resolver) = v8::PromiseResolver::new(scope) else {
      return Js::null(scope);
    };
    let value: v8::Local<v8::Value> =
      v8::Boolean::new(scope, supported_type(&mime)).into();
    resolver.resolve(scope, value);
    let promise: v8::Local<v8::Value> = resolver.get_promise(scope).into();
    Js(v8::Global::new(scope, promise))
  }

  fn reset(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    if self.closed.get() {
      return Err(closed_error());
    }
    self.generation.set(self.generation.get().wrapping_add(1));
    self.reject_waiters(scope, abort_error());
    if self.data_complete.get()
      && self.raster.borrow().is_none()
      && self.error_name.borrow().is_none()
    {
      if let Some(object) = self
        .self_object
        .borrow()
        .as_ref()
        .and_then(|reference| yuzuko_core::op::traced_local(scope, reference))
      {
        self.spawn_decode(scope, object);
      }
    }
    Ok(())
  }

  fn close(&self, scope: &mut v8::PinScope) {
    if self.closed.get() {
      return;
    }
    self.closed.set(true);
    self.generation.set(self.generation.get().wrapping_add(1));
    self.bytes.borrow_mut().clear();
    *self.raster.borrow_mut() = None;
    self.reject_waiters(scope, closed_error());
    if !self.settled.get() {
      self.fail("InvalidStateError", "The decoder is closed");
      self.settle_metadata(scope);
    }
  }
}

impl ImageDecoder {
  fn spawn_decode(
    &self,
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
  ) {
    let bytes = self.bytes.borrow().clone();
    let to_srgb = self.to_srgb;
    let generation = self.generation.get();
    let handle = v8::Global::new(scope, object);
    yuzuko_core::spawn(
      move || decode_still(bytes, to_srgb),
      move |scope, result| {
        let object = v8::Local::new(scope, handle);
        let Some(host) = (unsafe { host_ref::<ImageDecoder>(scope, object) })
        else {
          return;
        };
        host.get().on_decoded(scope, object, generation, result);
      },
    );
  }

  fn on_decoded(
    &self,
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
    generation: u64,
    result: Result<Result<Raster, String>, ()>,
  ) {
    if self.closed.get() || generation != self.generation.get() {
      return;
    }
    match result {
      Ok(Ok(raster)) => {
        *self.raster.borrow_mut() = Some(raster);
        self.ready.set(true);
        if let Some(track) =
          self.track.borrow().as_ref().and_then(|reference| {
            yuzuko_core::op::traced_local(scope, reference)
          })
        {
          if let Some(list) =
            self.tracks.borrow().as_ref().and_then(|reference| {
              yuzuko_core::op::traced_local(scope, reference)
            })
          {
            let _ = list.set_index(scope, 0, track.into());
          }
        }
        self.settle_metadata(scope);
        self.flush_waiters(scope);
      }
      Ok(Err(message)) => {
        self.fail("EncodingError", &message);
        self.settle_metadata(scope);
        self.reject_waiters(scope, OpError::dom("EncodingError", message));
      }
      Err(()) => {
        let message = "The source image could not be decoded".to_string();
        self.fail("EncodingError", &message);
        self.settle_metadata(scope);
        self.reject_waiters(scope, OpError::dom("EncodingError", message));
      }
    }
    let _ = object;
  }

  fn finish_decode(&self, scope: &mut v8::PinScope, index: u32) -> Js {
    if !self.selected.get() {
      return rejected_error(
        scope,
        OpError::dom("InvalidStateError", "No track is selected"),
      );
    }
    if index != 0 {
      return rejected_error(
        scope,
        OpError::range("The frame index is out of range"),
      );
    }
    let Some(raster) = self.raster.borrow().clone() else {
      return rejected_error(
        scope,
        OpError::dom("EncodingError", "The source image could not be decoded"),
      );
    };
    resolve_frame(scope, raster)
  }

  fn flush_waiters(&self, scope: &mut v8::PinScope) {
    let waiters = std::mem::take(&mut *self.waiters.borrow_mut());
    for waiter in waiters {
      let resolver = v8::Local::new(scope, waiter.resolver);
      if self.closed.get() {
        reject_with(scope, resolver, closed_error());
        continue;
      }
      if !self.selected.get() {
        reject_with(
          scope,
          resolver,
          OpError::dom("InvalidStateError", "No track is selected"),
        );
        continue;
      }
      if waiter.index != 0 {
        reject_with(
          scope,
          resolver,
          OpError::range("The frame index is out of range"),
        );
        continue;
      }
      let Some(raster) = self.raster.borrow().clone() else {
        reject_with(
          scope,
          resolver,
          OpError::dom(
            "EncodingError",
            "The source image could not be decoded",
          ),
        );
        continue;
      };
      let frame = unsafe {
        yuzuko_core::op::wrap_instance(scope, VideoFrame::from_raster(raster))
      };
      let result = decode_result(scope, frame);
      resolver.resolve(scope, result.into());
    }
  }

  fn reject_waiters(&self, scope: &mut v8::PinScope, error: OpError) {
    let waiters = std::mem::take(&mut *self.waiters.borrow_mut());
    for waiter in waiters {
      let resolver = v8::Local::new(scope, waiter.resolver);
      reject_with(scope, resolver, error.clone_error());
    }
  }

  fn settle_metadata(&self, scope: &mut v8::PinScope) {
    if self.settled.get() {
      return;
    }
    self.settled.set(true);
    let undefined: v8::Local<v8::Value> = v8::undefined(scope).into();
    if let Some(name) = *self.error_name.borrow() {
      let message = self.error_message.borrow().clone().unwrap_or_default();
      let error = if name == "TypeError" {
        OpError::type_(message)
      } else {
        OpError::dom(name, message)
      };
      if let Some(resolver) = self.completed_resolver.borrow_mut().take() {
        reject_with(
          scope,
          v8::Local::new(scope, resolver),
          error.clone_error(),
        );
      }
      if let Some(resolver) = self.tracks_resolver.borrow_mut().take() {
        reject_with(scope, v8::Local::new(scope, resolver), error);
      }
      return;
    }
    if let Some(resolver) = self.completed_resolver.borrow_mut().take() {
      v8::Local::new(scope, resolver).resolve(scope, undefined);
    }
    if let Some(resolver) = self.tracks_resolver.borrow_mut().take() {
      v8::Local::new(scope, resolver).resolve(scope, undefined);
    }
  }

  fn fail(&self, name: &'static str, message: &str) {
    *self.error_name.borrow_mut() = Some(name);
    *self.error_message.borrow_mut() = Some(message.to_string());
  }

  fn append_chunk(&self, bytes: &[u8]) {
    if !self.closed.get() {
      self.bytes.borrow_mut().extend_from_slice(bytes);
    }
  }

  fn note_complete(
    &self,
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
  ) {
    if self.closed.get() || self.data_complete.get() {
      return;
    }
    self.data_complete.set(true);
    if self.error_name.borrow().is_none() {
      self.spawn_decode(scope, object);
    }
  }
}

#[op]
impl ImageTrackList {
  #[getter]
  fn ready(&self, scope: &mut v8::PinScope) -> Js {
    let Some(decoder) = open_decoder(scope, &self.decoder) else {
      return Js::null(scope);
    };
    decoder.get().tracks_promise(scope)
  }

  #[getter]
  fn length(&self, scope: &mut v8::PinScope) -> u32 {
    let Some(decoder) = open_decoder(scope, &self.decoder) else {
      return 0;
    };
    u32::from(
      decoder.get().ready.get() && decoder.get().error_name.borrow().is_none(),
    )
  }

  #[getter]
  fn selected_index(&self, scope: &mut v8::PinScope) -> i32 {
    let Some(decoder) = open_decoder(scope, &self.decoder) else {
      return -1;
    };
    let decoder = decoder.get();
    if decoder.ready.get()
      && decoder.selected.get()
      && decoder.error_name.borrow().is_none()
    {
      0
    } else {
      -1
    }
  }

  #[getter]
  fn selected_track(&self, scope: &mut v8::PinScope) -> Js {
    let Some(decoder) = open_decoder(scope, &self.decoder) else {
      return Js::null(scope);
    };
    let decoder = decoder.get();
    if !decoder.ready.get()
      || !decoder.selected.get()
      || decoder.error_name.borrow().is_some()
    {
      Js::null(scope)
    } else {
      stored_object(scope, &decoder.track)
    }
  }
}

impl ImageTrackList {
  fn new(scope: &mut v8::PinScope, decoder: v8::Local<v8::Object>) -> Self {
    Self {
      decoder: traced(scope, decoder),
    }
  }
}

#[op]
impl ImageTrack {
  #[getter]
  fn animated(&self) -> bool {
    false
  }

  #[getter]
  fn frame_count(&self) -> u32 {
    1
  }

  #[getter]
  fn repetition_count(&self) -> f64 {
    0.0
  }

  #[getter]
  fn selected(&self, scope: &mut v8::PinScope) -> bool {
    open_decoder(scope, &self.decoder)
      .is_some_and(|decoder| decoder.get().selected.get())
  }

  #[setter]
  fn selected(
    &self,
    scope: &mut v8::PinScope,
    value: bool,
  ) -> Result<(), OpError> {
    let Some(object) = yuzuko_core::op::traced_local(scope, &self.decoder)
    else {
      return Err(closed_error());
    };
    let Some(decoder) = (unsafe { host_ref::<ImageDecoder>(scope, object) })
    else {
      return Err(closed_error());
    };
    let decoder = decoder.get();
    if decoder.closed.get() {
      return Err(closed_error());
    }
    decoder.selected.set(value);
    Ok(())
  }
}

impl ImageTrack {
  fn new(scope: &mut v8::PinScope, decoder: v8::Local<v8::Object>) -> Self {
    Self {
      decoder: traced(scope, decoder),
    }
  }
}

impl ImageDecoder {
  fn tracks_promise(&self, scope: &mut v8::PinScope) -> Js {
    stored_promise(scope, &self.tracks_ready)
  }
}

fn decode_still(bytes: Vec<u8>, to_srgb: bool) -> Result<Raster, String> {
  let decoded = decode_image(&bytes, to_srgb).map_err(decode_message)?;
  orient(decoded.raster, decoded.orientation).map_err(pixel_message)
}

fn decode_message(error: ImageDecodeError) -> String {
  match error {
    ImageDecodeError::TooLarge => "The image is too large",
    ImageDecodeError::Unsupported | ImageDecodeError::Corrupt => {
      "The source image could not be decoded"
    }
  }
  .to_string()
}

fn pixel_message(error: PixelError) -> String {
  match error {
    PixelError::TooLarge => "The image is too large",
    PixelError::Empty => "The source image could not be decoded",
  }
  .to_string()
}

fn supported_type(mime: &str) -> bool {
  matches!(
    essence(mime).as_str(),
    "image/png" | "image/jpeg" | "image/webp" | "image/jxl"
  )
}

fn essence(mime: &str) -> String {
  let mime = mime.trim().to_ascii_lowercase();
  mime.split([';', ' ']).next().unwrap_or("").to_string()
}

fn defer(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return;
  };
  let undefined: v8::Local<v8::Value> = v8::undefined(scope).into();
  resolver.resolve(scope, undefined);
  let promise = resolver.get_promise(scope);
  let data: v8::Local<v8::Value> = object.into();
  let Some(function) = v8::Function::builder(callback).data(data).build(scope)
  else {
    return;
  };
  let _ = promise.then(scope, function);
}

fn on_deferred_settle(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Ok(object) = v8::Local::<v8::Object>::try_from(args.data()) else {
    return;
  };
  let Some(host) = (unsafe { host_ref::<ImageDecoder>(scope, object) }) else {
    return;
  };
  host.get().settle_metadata(scope);
}

fn pull_stream(
  scope: &mut v8::PinScope,
  decoder: v8::Local<v8::Object>,
  stream: v8::Local<v8::Value>,
) {
  let Ok(stream) = v8::Local::<v8::Object>::try_from(stream) else {
    fail_object(scope, decoder, "TypeError", "data is not a ReadableStream");
    return;
  };
  let reader = match call_method(scope, stream, "getReader", &[]) {
    Ok(value) => value,
    Err(error) => {
      fail_thrown(scope, decoder, error);
      return;
    }
  };
  let Ok(reader) = v8::Local::<v8::Object>::try_from(reader) else {
    fail_object(
      scope,
      decoder,
      "TypeError",
      "getReader did not return an object",
    );
    return;
  };
  read_chunk(scope, decoder, reader);
}

fn read_chunk(
  scope: &mut v8::PinScope,
  decoder: v8::Local<v8::Object>,
  reader: v8::Local<v8::Object>,
) {
  let result = match call_method(scope, reader, "read", &[]) {
    Ok(value) => value,
    Err(error) => {
      fail_thrown(scope, decoder, error);
      return;
    }
  };
  let Ok(promise) = v8::Local::<v8::Promise>::try_from(result) else {
    fail_object(scope, decoder, "TypeError", "read did not return a promise");
    return;
  };
  let pack = v8::Array::new(scope, 2);
  let _ = pack.set_index(scope, 0, decoder.into());
  let _ = pack.set_index(scope, 1, reader.into());
  let pack: v8::Local<v8::Value> = pack.into();
  let Some(ok) = v8::Function::builder(on_chunk).data(pack).build(scope) else {
    return;
  };
  let Some(err) = v8::Function::builder(on_stream_error)
    .data(pack)
    .build(scope)
  else {
    return;
  };
  let _ = promise.then2(scope, ok, err);
}

fn on_chunk(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some((decoder, reader)) = pack_of(scope, args.data()) else {
    return;
  };
  let Some(host) = (unsafe { host_ref::<ImageDecoder>(scope, decoder) }) else {
    return;
  };
  if host.get().closed.get() {
    return;
  }
  let Ok(result) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
    host
      .get()
      .fail("EncodingError", "The image data could not be read");
    host.get().settle_metadata(scope);
    return;
  };
  let done = member(scope, result, "done").is_some_and(|value| value.is_true());
  if done {
    host.get().note_complete(scope, decoder);
    return;
  }
  let Some(value) = member(scope, result, "value") else {
    host
      .get()
      .fail("TypeError", "A stream chunk is not a BufferSource");
    host.get().settle_metadata(scope);
    host.get().reject_waiters(
      scope,
      OpError::type_("A stream chunk is not a BufferSource"),
    );
    return;
  };
  let Some(bytes) = buffer_source(scope, value) else {
    host
      .get()
      .fail("TypeError", "A stream chunk is not a BufferSource");
    host.get().settle_metadata(scope);
    host.get().reject_waiters(
      scope,
      OpError::type_("A stream chunk is not a BufferSource"),
    );
    return;
  };
  host.get().append_chunk(&bytes);
  read_chunk(scope, decoder, reader);
}

fn on_stream_error(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut _ret: v8::ReturnValue,
) {
  let Some((decoder, _)) = pack_of(scope, args.data()) else {
    return;
  };
  fail_thrown(scope, decoder, args.get(0));
}

fn pack_of<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  data: v8::Local<'_, v8::Value>,
) -> Option<(v8::Local<'s, v8::Object>, v8::Local<'s, v8::Object>)> {
  let pack = v8::Local::<v8::Array>::try_from(data).ok()?;
  let decoder = pack.get_index(scope, 0)?;
  let reader = pack.get_index(scope, 1)?;
  Some((
    v8::Local::<v8::Object>::try_from(decoder).ok()?,
    v8::Local::<v8::Object>::try_from(reader).ok()?,
  ))
}

fn fail_object(
  scope: &mut v8::PinScope,
  decoder: v8::Local<v8::Object>,
  name: &'static str,
  message: &str,
) {
  let Some(host) = (unsafe { host_ref::<ImageDecoder>(scope, decoder) }) else {
    return;
  };
  if name == "TypeError" {
    host.get().fail("TypeError", message);
  } else {
    host.get().fail(name, message);
  }
  host.get().settle_metadata(scope);
}

fn fail_thrown(
  scope: &mut v8::PinScope,
  decoder: v8::Local<v8::Object>,
  value: v8::Local<v8::Value>,
) {
  let Some(host) = (unsafe { host_ref::<ImageDecoder>(scope, decoder) }) else {
    return;
  };
  let message = rust_string(scope, value);
  host.get().fail("EncodingError", &message);
  host.get().settle_metadata(scope);
  host
    .get()
    .reject_waiters(scope, OpError::dom("EncodingError", message));
}

fn resolve_frame(scope: &mut v8::PinScope, raster: Raster) -> Js {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Js::null(scope);
  };
  let frame = unsafe {
    yuzuko_core::op::wrap_instance(scope, VideoFrame::from_raster(raster))
  };
  let result = decode_result(scope, frame);
  resolver.resolve(scope, result.into());
  let promise: v8::Local<v8::Value> = resolver.get_promise(scope).into();
  Js(v8::Global::new(scope, promise))
}

fn decode_result<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  frame: v8::Local<'_, v8::Object>,
) -> v8::Local<'s, v8::Object> {
  let result = v8::Object::new(scope);
  let image = v8::String::new(scope, "image").unwrap();
  let complete = v8::String::new(scope, "complete").unwrap();
  let frame: v8::Local<v8::Value> = frame.into();
  let _ = result.set(scope, image.into(), frame);
  let _ =
    result.set(scope, complete.into(), v8::Boolean::new(scope, true).into());
  result
}

fn open_decoder(
  scope: &mut v8::PinScope<'_, '_>,
  reference: &v8::TracedReference<v8::Object>,
) -> Option<yuzuko_core::op::HostRef<ImageDecoder>> {
  let object = yuzuko_core::op::traced_local(scope, reference)?;
  unsafe { host_ref::<ImageDecoder>(scope, object) }
}

fn stored_promise(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Promise>>>,
) -> Js {
  let promise = slot
    .borrow()
    .as_ref()
    .and_then(|reference| yuzuko_core::op::traced_local(scope, reference));
  match promise {
    Some(promise) => {
      let value: v8::Local<v8::Value> = promise.into();
      Js(v8::Global::new(scope, value))
    }
    None => Js::null(scope),
  }
}

fn stored_object(
  scope: &mut v8::PinScope,
  slot: &RefCell<Option<v8::TracedReference<v8::Object>>>,
) -> Js {
  let object = slot
    .borrow()
    .as_ref()
    .and_then(|reference| yuzuko_core::op::traced_local(scope, reference));
  match object {
    Some(object) => {
      let value: v8::Local<v8::Value> = object.into();
      Js(v8::Global::new(scope, value))
    }
    None => Js::null(scope),
  }
}

fn promise_pair<'s>(
  scope: &mut v8::PinScope<'s, '_>,
) -> Result<
  (
    v8::Local<'s, v8::Promise>,
    v8::Local<'s, v8::PromiseResolver>,
  ),
  OpError,
> {
  let resolver = v8::PromiseResolver::new(scope)
    .ok_or_else(|| OpError::type_("Promise is unavailable"))?;
  Ok((resolver.get_promise(scope), resolver))
}

fn rejected_error(scope: &mut v8::PinScope, error: OpError) -> Js {
  let Some(resolver) = v8::PromiseResolver::new(scope) else {
    return Js::null(scope);
  };
  reject_with(scope, resolver, error);
  let promise: v8::Local<v8::Value> = resolver.get_promise(scope).into();
  Js(v8::Global::new(scope, promise))
}

fn reject_with(
  scope: &mut v8::PinScope,
  resolver: v8::Local<v8::PromiseResolver>,
  error: OpError,
) {
  let value = match &error {
    OpError::Type(message) => {
      let message = v8::String::new(scope, message).unwrap();
      v8::Exception::type_error(scope, message)
    }
    OpError::Range(message) => {
      let message = v8::String::new(scope, message).unwrap();
      v8::Exception::range_error(scope, message)
    }
    OpError::Dom { name, message } => new_dom_exception(scope, name, message)
      .map(|object| object.into())
      .unwrap_or_else(|| {
        let message = v8::String::new(scope, message).unwrap();
        v8::Exception::type_error(scope, message)
      }),
    OpError::Thrown(_) => {
      let message =
        v8::String::new(scope, "The image data could not be read").unwrap();
      v8::Exception::type_error(scope, message)
    }
  };
  resolver.reject(scope, value);
}

trait CloneError {
  fn clone_error(&self) -> OpError;
}

impl CloneError for OpError {
  fn clone_error(&self) -> OpError {
    match self {
      Self::Type(message) => Self::Type(message.clone()),
      Self::Range(message) => Self::Range(message.clone()),
      Self::Dom { name, message } => Self::Dom {
        name: name.clone(),
        message: message.clone(),
      },
      Self::Thrown(_) => Self::type_("The image data could not be read"),
    }
  }
}

fn closed_error() -> OpError {
  OpError::dom("InvalidStateError", "The decoder is closed")
}

fn abort_error() -> OpError {
  OpError::dom("AbortError", "The decode was reset")
}

fn is_readable_stream(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> bool {
  let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
    return false;
  };
  let global = scope.get_current_context().global(scope);
  let Some(key) = v8::String::new(scope, "ReadableStream") else {
    return false;
  };
  let Some(ctor) = global.get(scope, key.into()) else {
    return false;
  };
  let Ok(ctor) = v8::Local::<v8::Object>::try_from(ctor) else {
    return false;
  };
  object.instance_of(scope, ctor).unwrap_or(false)
}

fn detached_source(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> bool {
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    return buffer.was_detached();
  }
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    return view
      .buffer(scope)
      .is_some_and(|buffer| buffer.was_detached());
  }
  false
}

fn transfer_buffers(
  scope: &mut v8::PinScope,
  init: v8::Local<v8::Object>,
) -> Result<(), OpError> {
  let Some(value) = member(scope, init, "transfer") else {
    return Ok(());
  };
  let array = v8::Local::<v8::Array>::try_from(value)
    .map_err(|_| OpError::type_("transfer is not a sequence"))?;
  let mut buffers = Vec::new();
  for index in 0..array.length() {
    let Some(item) = array.get_index(scope, index) else {
      continue;
    };
    let buffer =
      v8::Local::<v8::ArrayBuffer>::try_from(item).map_err(|_| {
        OpError::type_("A transfer list entry is not an ArrayBuffer")
      })?;
    if buffer.was_detached() {
      return Err(OpError::dom(
        "DataCloneError",
        "The ArrayBuffer is detached",
      ));
    }
    if !buffer.is_detachable() {
      return Err(OpError::dom(
        "DataCloneError",
        "The ArrayBuffer cannot be detached",
      ));
    }
    buffers.push(v8::Global::new(scope, buffer));
  }
  for buffer in buffers {
    let buffer = v8::Local::new(scope, buffer);
    let _ = buffer.detach(None);
  }
  Ok(())
}

fn color_space_conversion(
  scope: &mut v8::PinScope,
  init: v8::Local<v8::Object>,
) -> Result<bool, OpError> {
  let Some(value) = member(scope, init, "colorSpaceConversion") else {
    return Ok(true);
  };
  match rust_string(scope, value).as_str() {
    "default" => Ok(true),
    "none" => Ok(false),
    _ => Err(OpError::type_("colorSpaceConversion is not supported")),
  }
}

fn frame_index(
  scope: &mut v8::PinScope,
  options: Option<v8::Local<v8::Value>>,
) -> Result<u32, OpError> {
  let Some(options) = options else {
    return Ok(0);
  };
  if options.is_undefined() {
    return Ok(0);
  }
  let object = dictionary(scope, options)?;
  Ok(optional_ulong(scope, object, "frameIndex")?.unwrap_or(0))
}

fn optional_ulong(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<u32>, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Ok(None);
  };
  let number = value
    .number_value(scope)
    .ok_or_else(|| OpError::type_(format!("{name} is not a number")))?;
  if !number.is_finite() || number < 0.0 || number > f64::from(u32::MAX) {
    return Err(OpError::type_(format!("{name} is out of range")));
  }
  Ok(Some(number as u32))
}

fn optional_bool(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<Option<bool>, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Ok(None);
  };
  if !value.is_boolean() {
    return Err(OpError::type_(format!("{name} is not a boolean")));
  }
  Ok(Some(value.boolean_value(scope)))
}

fn call_method<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
  args: &[v8::Local<'_, v8::Value>],
) -> Result<v8::Local<'s, v8::Value>, v8::Local<'s, v8::Value>> {
  let key = v8::String::new(scope, name).unwrap();
  let method = object.get(scope, key.into()).unwrap();
  let function = v8::Local::<v8::Function>::try_from(method).map_err(|_| {
    let message =
      v8::String::new(scope, "The stream method is missing").unwrap();
    v8::Exception::type_error(scope, message)
  })?;
  v8::tc_scope!(let tc, scope);
  match function.call(tc, object.into(), args) {
    Some(value) => Ok(yuzuko_core::op::root(tc, value)),
    None => Err(yuzuko_core::op::root(
      tc,
      tc.exception().unwrap_or_else(|| v8::undefined(tc).into()),
    )),
  }
}

fn dictionary<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'_, v8::Value>,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  let value = yuzuko_core::op::root(scope, value);
  v8::Local::<v8::Object>::try_from(value)
    .map_err(|_| OpError::type_("ImageDecoderInit is not an object"))
}

fn member<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
  let key = v8::String::new(scope, name)?;
  let value = object.get(scope, key.into())?;
  if value.is_undefined() {
    None
  } else {
    Some(value)
  }
}

fn required_string(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
) -> Result<String, OpError> {
  let Some(value) = member(scope, object, name) else {
    return Err(OpError::type_(format!("{name} is required")));
  };
  Ok(rust_string(scope, value))
}

fn trace_promise(
  slot: &RefCell<Option<v8::TracedReference<v8::Promise>>>,
  visitor: &mut v8::cppgc::Visitor,
) {
  if let Ok(slot) = slot.try_borrow() {
    visitor.trace(&*slot);
  }
}

fn trace_object(
  slot: &RefCell<Option<v8::TracedReference<v8::Object>>>,
  visitor: &mut v8::cppgc::Visitor,
) {
  if let Ok(slot) = slot.try_borrow() {
    visitor.trace(&*slot);
  }
}
