//! Structured clone records for transferable streams.
//!
//! Tag `20` is one readable port, tag `21` is one writable port, and tag
//! `22` is the readable port followed by the writable port. The ports are
//! moved directly, so they are not entries in the caller's transfer list
//! and they are not recorded on `MessageEvent.ports`.

use yuzuko_winter::{
  StreamKind, StreamStatus, TransferError, export_readable, export_transform,
  export_writable, prepare_cross_readable, prepare_cross_writable,
  shell_transform, transfer_status,
};

const TAG_READABLE: u32 = 20;
const TAG_WRITABLE: u32 = 21;
const TAG_TRANSFORM: u32 = 22;

pub(crate) fn register_clone() {
  yuzuko_core::add_host_clone(write_stream, read_stream);
  yuzuko_core::add_host_transfer(probe_stream);
}

fn probe_stream(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> yuzuko_core::TransferStatus {
  match transfer_status(scope, object) {
    StreamStatus::Skip => yuzuko_core::TransferStatus::Skip,
    StreamStatus::Locked => yuzuko_core::TransferStatus::Detached,
    StreamStatus::Ready(_) => yuzuko_core::TransferStatus::Ready,
  }
}

fn write_stream(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> yuzuko_core::WriteResult {
  let kind = match transfer_status(scope, object) {
    StreamStatus::Skip => return yuzuko_core::WriteResult::Skip,
    StreamStatus::Locked => {
      yuzuko_core::reject_clone(scope);
      return yuzuko_core::WriteResult::Failed;
    }
    StreamStatus::Ready(kind) => kind,
  };
  if !yuzuko_core::is_structured_transfer(scope, object) {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::WriteResult::Failed;
  }
  match kind {
    StreamKind::Readable => write_one(scope, object, helper, true),
    StreamKind::Writable => write_one(scope, object, helper, false),
    StreamKind::Transform => write_transform(scope, object, helper),
  }
}

fn write_one(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
  readable: bool,
) -> yuzuko_core::WriteResult {
  let exported = if readable {
    export_readable(scope, object)
  } else {
    export_writable(scope, object)
  };
  let port = match exported {
    Ok(port) => port,
    Err(error) => return fail_write(scope, error),
  };
  let id = match move_port(scope, port) {
    Ok(id) => id,
    Err(error) => return fail_write(scope, error),
  };
  if readable {
    helper.write_uint32(TAG_READABLE);
  } else {
    helper.write_uint32(TAG_WRITABLE);
  }
  helper.write_uint64(id);
  yuzuko_core::WriteResult::Wrote
}

fn write_transform(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
  helper: &dyn v8::ValueSerializerHelper,
) -> yuzuko_core::WriteResult {
  let (readable_port, writable_port) = match export_transform(scope, object) {
    Ok(ports) => ports,
    Err(error) => return fail_write(scope, error),
  };
  let readable_id = match move_port(scope, readable_port) {
    Ok(id) => id,
    Err(error) => return fail_write(scope, error),
  };
  let writable_id = match move_port(scope, writable_port) {
    Ok(id) => id,
    Err(error) => return fail_write(scope, error),
  };
  helper.write_uint32(TAG_TRANSFORM);
  helper.write_uint64(readable_id);
  helper.write_uint64(writable_id);
  yuzuko_core::WriteResult::Wrote
}

fn move_port(
  scope: &mut v8::PinScope,
  port: v8::Global<v8::Object>,
) -> Result<u64, TransferError> {
  let port = v8::Local::new(scope, port);
  match crate::message::transfer_port(scope, port) {
    Ok(id) => Ok(id),
    Err(()) => Err(TransferError::Failed),
  }
}

fn fail_write(
  scope: &mut v8::PinScope,
  error: TransferError,
) -> yuzuko_core::WriteResult {
  match error {
    TransferError::Thrown(thrown) => {
      let thrown = v8::Local::new(scope, thrown);
      scope.throw_exception(thrown);
    }
    TransferError::Failed => yuzuko_core::reject_clone(scope),
  }
  yuzuko_core::WriteResult::Failed
}

fn read_stream(
  scope: &mut v8::PinScope,
  tag: u32,
  helper: &dyn v8::ValueDeserializerHelper,
) -> yuzuko_core::ReadResult {
  match tag {
    TAG_READABLE => read_one(scope, helper, true),
    TAG_WRITABLE => read_one(scope, helper, false),
    TAG_TRANSFORM => read_transform(scope, helper),
    _ => yuzuko_core::ReadResult::Skip,
  }
}

fn read_one(
  scope: &mut v8::PinScope,
  helper: &dyn v8::ValueDeserializerHelper,
  readable: bool,
) -> yuzuko_core::ReadResult {
  let Some(id) = read_id(helper) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let port = match crate::message::receive_transferred_port(scope, id) {
    Ok(port) => port,
    Err(()) => {
      yuzuko_core::reject_clone(scope);
      return yuzuko_core::ReadResult::Failed;
    }
  };
  let prepared = if readable {
    prepare_cross_readable(scope, port)
  } else {
    prepare_cross_writable(scope, port)
  };
  finish_read(scope, prepared)
}

fn read_transform(
  scope: &mut v8::PinScope,
  helper: &dyn v8::ValueDeserializerHelper,
) -> yuzuko_core::ReadResult {
  let Some(readable_id) = read_id(helper) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let Some(writable_id) = read_id(helper) else {
    yuzuko_core::reject_clone(scope);
    return yuzuko_core::ReadResult::Failed;
  };
  let readable_port =
    match crate::message::receive_transferred_port(scope, readable_id) {
      Ok(port) => port,
      Err(()) => {
        yuzuko_core::reject_clone(scope);
        return yuzuko_core::ReadResult::Failed;
      }
    };
  let writable_port =
    match crate::message::receive_transferred_port(scope, writable_id) {
      Ok(port) => port,
      Err(()) => {
        yuzuko_core::reject_clone(scope);
        return yuzuko_core::ReadResult::Failed;
      }
    };
  let readable = match prepare_cross_readable(scope, readable_port) {
    Ok(stream) => stream,
    Err(error) => return fail_read(scope, error),
  };
  let writable = match prepare_cross_writable(scope, writable_port) {
    Ok(stream) => stream,
    Err(error) => return fail_read(scope, error),
  };
  let readable = v8::Local::new(scope, &readable);
  let writable = v8::Local::new(scope, &writable);
  yuzuko_core::ReadResult::Done(shell_transform(scope, readable, writable))
}

fn read_id(helper: &dyn v8::ValueDeserializerHelper) -> Option<u64> {
  let mut id = 0u64;
  if helper.read_uint64(&mut id) {
    Some(id)
  } else {
    None
  }
}

fn finish_read(
  scope: &mut v8::PinScope,
  result: Result<v8::Global<v8::Object>, TransferError>,
) -> yuzuko_core::ReadResult {
  match result {
    Ok(object) => yuzuko_core::ReadResult::Done(object),
    Err(error) => fail_read(scope, error),
  }
}

fn fail_read(
  scope: &mut v8::PinScope,
  error: TransferError,
) -> yuzuko_core::ReadResult {
  match error {
    TransferError::Thrown(thrown) => {
      let thrown = v8::Local::new(scope, thrown);
      scope.throw_exception(thrown);
    }
    TransferError::Failed => yuzuko_core::reject_clone(scope),
  }
  yuzuko_core::ReadResult::Failed
}
