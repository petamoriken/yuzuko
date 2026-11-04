//! `Yuzuko.demuxMp4`. The sample bytes stay as they are stored.

use yuzuko_core::op::{OpError, OpException};

pub(crate) fn install(scope: &mut v8::PinScope) {
  let yuzuko = yuzuko_core::namespace(scope, "Yuzuko");
  define_fn(scope, yuzuko, "demuxMp4", 1, demux_mp4);
}

fn define_fn(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  length: i32,
  callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
  let function = v8::Function::builder(callback)
    .length(length)
    .constructor_behavior(v8::ConstructorBehavior::Throw)
    .build(scope)
    .unwrap_or_else(|| panic!("create function {name}"));
  let key = v8::String::new(scope, name).unwrap();
  function.set_name(key);
  object.set(scope, key.into(), function.into());
}

fn demux_mp4(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  match movie_value(scope, args.get(0)) {
    Ok(value) => ret.set(value.into()),
    Err(error) => OpException::throw(error, scope),
  }
}

fn movie_value<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'_, v8::Value>,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  if detached(scope, value) {
    return Err(OpError::type_("The MP4 data is detached"));
  }
  let Some(bytes) = yuzuko_winter::buffer_source(scope, value) else {
    return Err(OpError::type_("The MP4 data is not a BufferSource"));
  };
  let movie = crate::mp4::parse(&bytes)
    .map_err(|error| OpError::dom("EncodingError", error.message()))?;
  to_movie(scope, movie).map_err(|_| {
    OpError::dom("EncodingError", "The MP4 samples could not be returned")
  })
}

fn detached(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> bool {
  if v8::Local::<v8::SharedArrayBuffer>::try_from(value).is_ok() {
    return false;
  }
  if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
    return view
      .buffer(scope)
      .is_some_and(|buffer| buffer.was_detached());
  }
  if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
    return buffer.was_detached();
  }
  false
}

fn to_movie<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  movie: crate::mp4::Movie,
) -> Result<v8::Local<'s, v8::Object>, ()> {
  let object = v8::Object::new(scope);
  let video = array_of(scope, movie.video, |scope, track| {
    let chunks = chunk_array(scope, track.samples, false)?;
    let value = v8::Object::new(scope);
    set_text(scope, value, "codec", &track.codec)?;
    set_bytes(scope, value, "description", track.description)?;
    set_number(scope, value, "codedWidth", track.coded_width as f64);
    set_number(scope, value, "codedHeight", track.coded_height as f64);
    yuzuko_core::set_value(scope, value, "chunks", chunks.into());
    Ok(value)
  })?;
  let audio = array_of(scope, movie.audio, |scope, track| {
    let chunks = chunk_array(scope, track.samples, true)?;
    let value = v8::Object::new(scope);
    set_text(scope, value, "codec", &track.codec)?;
    set_bytes(scope, value, "description", track.description)?;
    set_number(scope, value, "sampleRate", track.sample_rate as f64);
    set_number(scope, value, "numberOfChannels", track.channels as f64);
    yuzuko_core::set_value(scope, value, "chunks", chunks.into());
    Ok(value)
  })?;
  yuzuko_core::set_value(scope, object, "video", video.into());
  yuzuko_core::set_value(scope, object, "audio", audio.into());
  Ok(object)
}

fn array_of<'s, T>(
  scope: &mut v8::PinScope<'s, '_>,
  items: Vec<T>,
  mut one: impl FnMut(
    &mut v8::PinScope<'s, '_>,
    T,
  ) -> Result<v8::Local<'s, v8::Object>, ()>,
) -> Result<v8::Local<'s, v8::Array>, ()> {
  let length = i32::try_from(items.len()).map_err(|_| ())?;
  let array = v8::Array::new(scope, length);
  for (index, item) in items.into_iter().enumerate() {
    let value = one(scope, item)?;
    let index = u32::try_from(index).map_err(|_| ())?;
    if array.set_index(scope, index, value.into()) != Some(true) {
      return Err(());
    }
  }
  Ok(array)
}

fn chunk_array<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  samples: Vec<crate::mp4::Sample>,
  audio: bool,
) -> Result<v8::Local<'s, v8::Array>, ()> {
  let length = i32::try_from(samples.len()).map_err(|_| ())?;
  let array = v8::Array::new(scope, length);
  for (index, sample) in samples.into_iter().enumerate() {
    let duration = Some(sample.duration as f64);
    let object = if audio {
      let chunk = crate::classes::EncodedAudioChunk::from_parts(
        sample.key,
        sample.timestamp,
        duration,
        sample.data,
      );
      // Installed earlier in this realm, before `demuxMp4`.
      unsafe { yuzuko_core::op::wrap_instance(scope, chunk) }
    } else {
      let chunk = crate::classes::EncodedVideoChunk::from_parts(
        sample.key,
        sample.timestamp,
        duration,
        sample.data,
      );
      unsafe { yuzuko_core::op::wrap_instance(scope, chunk) }
    };
    let index = u32::try_from(index).map_err(|_| ())?;
    if array.set_index(scope, index, object.into()) != Some(true) {
      return Err(());
    }
  }
  Ok(array)
}

fn set_number(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: f64,
) {
  let number = v8::Number::new(scope, value);
  yuzuko_core::set_value(scope, object, name, number.into());
}

fn set_text(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: &str,
) -> Result<(), ()> {
  let Some(text) = v8::String::new(scope, value) else {
    return Err(());
  };
  yuzuko_core::set_value(scope, object, name, text.into());
  Ok(())
}

fn set_bytes(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  bytes: Vec<u8>,
) -> Result<(), ()> {
  let length = bytes.len();
  let buffer = yuzuko_winter::array_buffer(scope, bytes);
  let Some(view) = v8::Uint8Array::new(scope, buffer, 0, length) else {
    return Err(());
  };
  yuzuko_core::set_value(scope, object, name, view.into());
  Ok(())
}
