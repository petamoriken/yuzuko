//! `Yuzuko.demuxMp4`. The sample bytes stay as they are stored.

use yuzuko_core::op::{OpError, OpException};

pub(crate) fn install(scope: &mut v8::PinScope) {
  let yuzuko = yuzuko_core::namespace(scope, "Yuzuko");
  define_fn(scope, yuzuko, "demuxMp4", 1, demux_mp4);
}

use yuzuko_core::define_function as define_fn;

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
  if crate::classes::detached_buffer(scope, value) {
    return Err(OpError::type_("The MP4 data is detached"));
  }
  let Some(bytes) = yuzuko_winter::buffer_source(scope, value)? else {
    return Err(OpError::type_("The MP4 data is not a BufferSource"));
  };
  let movie = crate::mp4::parse(&bytes)
    .map_err(|error| OpError::dom("EncodingError", error.message()))?;
  to_movie(scope, movie).map_err(|_| {
    OpError::dom("EncodingError", "The MP4 samples could not be returned")
  })
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
    define_data(scope, value, "chunks", chunks.into());
    Ok(value)
  })?;
  let audio = array_of(scope, movie.audio, |scope, track| {
    let chunks = chunk_array(scope, track.samples, true)?;
    let value = v8::Object::new(scope);
    set_text(scope, value, "codec", &track.codec)?;
    set_bytes(scope, value, "description", track.description)?;
    set_number(scope, value, "sampleRate", track.sample_rate as f64);
    set_number(scope, value, "numberOfChannels", track.channels as f64);
    define_data(scope, value, "chunks", chunks.into());
    Ok(value)
  })?;
  define_data(scope, object, "video", video.into());
  define_data(scope, object, "audio", audio.into());
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
    if yuzuko_core::define_element(scope, array.into(), index, value.into())
      != Some(true)
    {
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
    if yuzuko_core::define_element(scope, array.into(), index, object.into())
      != Some(true)
    {
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
  define_data(scope, object, name, number.into());
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
  define_data(scope, object, name, text.into());
  Ok(())
}

fn set_bytes(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  bytes: Vec<u8>,
) -> Result<(), ()> {
  let Some(view) = yuzuko_core::uint8_array(scope, bytes) else {
    return Err(());
  };
  define_data(scope, object, name, view.into());
  Ok(())
}

fn define_data(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  name: &str,
  value: v8::Local<v8::Value>,
) {
  let key = v8::String::new(scope, name).unwrap();
  let _ = object.create_data_property(scope, key.into(), value);
}
