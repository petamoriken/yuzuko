//! `FileList`, `DataTransferItem`, and `DataTransferItemList`.
//!
//! Indexed handlers are installed before `GetFunction`. A later realm reuses
//! the isolate template, so the handlers are not installed again. `FileList`
//! and `DataTransferItemList` are iterable.

use std::cell::RefCell;

use yuzuko_core::op::{
  FromV8, Js, OpError, OpException, host_ref, traced, traced_local,
};
use yuzuko_macros::op;

use super::clipboard::{self, DataTransfer};
use crate::util::js_object;

pub(crate) fn prepare(scope: &mut v8::PinScope) {
  // `GetFunction` instantiates the template. `SetHandler` after that is a V8
  // fatal, and later realms reuse the isolate-cached template.
  if yuzuko_core::op::cached_template::<FileList>(scope).is_none() {
    let templ = FileList::template(scope);
    templ.instance_template(scope).set_indexed_property_handler(
      v8::IndexedPropertyHandlerConfiguration::new()
        .getter(file_list_get_index)
        .query(file_list_query_index)
        .setter(file_list_set_index)
        .deleter(file_list_delete_index)
        .enumerator(file_list_enumerate)
        .flags(v8::PropertyHandlerFlags::HAS_NO_SIDE_EFFECT),
    );
  }
  if yuzuko_core::op::cached_template::<DataTransferItemList>(scope).is_none() {
    let templ = DataTransferItemList::template(scope);
    templ.instance_template(scope).set_indexed_property_handler(
      v8::IndexedPropertyHandlerConfiguration::new()
        .getter(item_list_get_index)
        .query(item_list_query_index)
        .setter(item_list_set_index)
        .deleter(item_list_delete_index)
        .enumerator(item_list_enumerate),
    );
  }
}

pub(crate) fn install(scope: &mut v8::PinScope) {
  DataTransferItem::install(scope);
  DataTransferItemList::install(scope);
  FileList::install(scope);
  install_item_list_iterator(scope);
  install_file_list_iterator(scope);
}

pub(crate) fn items_object(
  scope: &mut v8::PinScope,
  this: v8::Local<v8::Object>,
  transfer: &DataTransfer,
) -> Js {
  if let Some(existing) = transfer.item_list_local(scope) {
    return js_object(scope, existing);
  }
  let list = DataTransferItemList {
    owner: traced(scope, this),
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, list) };
  transfer.remember_item_list(scope, object);
  js_object(scope, object)
}

pub(crate) fn files_object(
  scope: &mut v8::PinScope,
  transfer: &DataTransfer,
) -> Js {
  let files = transfer.file_locals(scope);
  let stored = files
    .iter()
    .map(|file| traced(scope, *file))
    .collect::<Vec<_>>();
  let list = FileList {
    files: RefCell::new(stored),
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, list) };
  js_object(scope, object)
}

fn item_wrapper<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  owner: v8::Local<'_, v8::Object>,
  id: u64,
) -> Option<v8::Local<'s, v8::Object>> {
  let cached = {
    let host = unsafe { host_ref::<DataTransfer>(scope, owner) }?;
    host.get().wrapper_local(scope, id)
  };
  if let Some(object) = cached {
    return Some(object);
  }
  let item = DataTransferItem {
    owner: traced(scope, owner),
    id,
  };
  let object = unsafe { yuzuko_core::op::wrap_instance(scope, item) };
  if let Some(host) = unsafe { host_ref::<DataTransfer>(scope, owner) } {
    host.get().remember_wrapper(scope, id, object);
  }
  Some(object)
}

fn wrapper_js(
  scope: &mut v8::PinScope,
  owner: v8::Local<v8::Object>,
  id: u64,
) -> Js {
  match item_wrapper(scope, owner, id) {
    Some(object) => js_object(scope, object),
    None => Js::null(scope),
  }
}

fn owner_of<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  reference: &v8::TracedReference<v8::Object>,
) -> Result<v8::Local<'s, v8::Object>, OpError> {
  traced_local(scope, reference)
    .ok_or_else(|| OpError::type_("illegal invocation"))
}

fn file_argument<'a>(
  scope: &mut v8::PinScope,
  value: v8::Local<'a, v8::Value>,
) -> Option<(v8::Local<'a, v8::Object>, String)> {
  let object = v8::Local::<v8::Object>::try_from(value).ok()?;
  let type_ = unsafe { host_ref::<yuzuko_winter::File>(scope, object) }?
    .get()
    .media_type();
  Some((object, type_))
}

/// One entry in a `DataTransfer`. `owner` is that store.
#[repr(C)]
struct DataTransferItem {
  owner: v8::TracedReference<v8::Object>,
  id: u64,
}

unsafe impl v8::cppgc::GarbageCollected for DataTransferItem {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.owner);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DataTransferItem"
  }
}

#[op]
impl DataTransferItem {
  #[getter]
  fn kind(&self, scope: &mut v8::PinScope) -> String {
    let Ok(owner) = owner_of(scope, &self.owner) else {
      return String::new();
    };
    let Some(transfer) = (unsafe { host_ref::<DataTransfer>(scope, owner) })
    else {
      return String::new();
    };
    transfer.get().kind_of(self.id)
  }

  #[getter]
  fn type_(&self, scope: &mut v8::PinScope) -> String {
    let Ok(owner) = owner_of(scope, &self.owner) else {
      return String::new();
    };
    let Some(transfer) = (unsafe { host_ref::<DataTransfer>(scope, owner) })
    else {
      return String::new();
    };
    transfer.get().type_of(self.id)
  }

  fn get_as_file(&self, scope: &mut v8::PinScope) -> Js {
    let Ok(owner) = owner_of(scope, &self.owner) else {
      return Js::null(scope);
    };
    let Some(transfer) = (unsafe { host_ref::<DataTransfer>(scope, owner) })
    else {
      return Js::null(scope);
    };
    match transfer.get().file_of(scope, self.id) {
      Some(object) => js_object(scope, object),
      None => Js::null(scope),
    }
  }

  #[length(1)]
  #[arity(0)]
  fn get_as_string(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<(), OpError> {
    if args.length() < 1 {
      return Ok(());
    }
    let callback = args.get(0);
    if callback.is_null() || callback.is_undefined() {
      return Ok(());
    }
    let Ok(function) = v8::Local::<v8::Function>::try_from(callback) else {
      return Err(OpError::type_(
        "DataTransferItem.getAsString requires a function",
      ));
    };
    let Ok(owner) = owner_of(scope, &self.owner) else {
      return Ok(());
    };
    let text = {
      let Some(transfer) = (unsafe { host_ref::<DataTransfer>(scope, owner) })
      else {
        return Ok(());
      };
      transfer.get().text_of(self.id)
    };
    let Some(text) = text else {
      return Ok(());
    };
    let function = v8::Global::new(scope, function);
    yuzuko_core::enqueue(move |scope| deliver_string(scope, function, text));
    Ok(())
  }
}

fn deliver_string(
  scope: &mut v8::PinScope,
  callback: v8::Global<v8::Function>,
  text: String,
) {
  let thrown = {
    v8::tc_scope!(let tc, scope);
    let function = v8::Local::new(tc, &callback);
    match v8::String::new(tc, &text) {
      Some(data) => {
        match function.call(tc, v8::undefined(tc).into(), &[data.into()]) {
          Some(_) => None,
          None => {
            let exception =
              tc.exception().unwrap_or_else(|| v8::undefined(tc).into());
            Some(v8::Global::new(tc, exception))
          }
        }
      }
      None => None,
    }
  };
  if let Some(exception) = thrown {
    yuzuko_dom::report_error(scope, v8::Local::new(scope, exception));
  }
}

/// The live item list of one `DataTransfer`.
#[repr(C)]
struct DataTransferItemList {
  owner: v8::TracedReference<v8::Object>,
}

unsafe impl v8::cppgc::GarbageCollected for DataTransferItemList {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    visitor.trace(&self.owner);
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"DataTransferItemList"
  }
}

#[op]
impl DataTransferItemList {
  #[getter]
  fn length(&self, scope: &mut v8::PinScope) -> Result<u32, OpError> {
    let owner = owner_of(scope, &self.owner)?;
    let Some(transfer) = (unsafe { host_ref::<DataTransfer>(scope, owner) })
    else {
      return Err(OpError::type_("illegal invocation"));
    };
    Ok(u32::try_from(transfer.get().len()).unwrap_or(u32::MAX))
  }

  #[length(1)]
  #[arity(0)]
  fn add(
    &self,
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
  ) -> Result<Js, OpError> {
    if args.length() < 1 {
      return Err(OpError::type_("DataTransferItemList.add requires data"));
    }
    let owner = owner_of(scope, &self.owner)?;
    if let Some((file, type_)) = file_argument(scope, args.get(0)) {
      let id = {
        let Some(transfer) =
          (unsafe { host_ref::<DataTransfer>(scope, owner) })
        else {
          return Err(OpError::type_("illegal invocation"));
        };
        if !transfer.get().writable() {
          return Ok(Js::null(scope));
        }
        transfer.get().append_file(scope, type_, file)
      };
      return Ok(wrapper_js(scope, owner, id));
    }
    if args.length() < 2 || args.get(1).is_undefined() {
      return Err(OpError::type_("DataTransferItemList.add requires a type"));
    }
    let data = String::from_v8(scope, args.get(0))?;
    let type_name = String::from_v8(scope, args.get(1))?;
    let id = {
      let Some(transfer) = (unsafe { host_ref::<DataTransfer>(scope, owner) })
      else {
        return Err(OpError::type_("illegal invocation"));
      };
      if !transfer.get().writable() {
        return Ok(Js::null(scope));
      }
      let (format, _) = clipboard::canonical_format(&type_name);
      if format.is_empty() {
        return Err(OpError::dom("SyntaxError", "The type is empty"));
      }
      if transfer.get().has_string_type(&format) {
        return Err(OpError::dom(
          "NotSupportedError",
          "The type is already present",
        ));
      }
      transfer.get().append_text(format, data)
    };
    Ok(wrapper_js(scope, owner, id))
  }

  fn clear(&self, scope: &mut v8::PinScope) -> Result<(), OpError> {
    let owner = owner_of(scope, &self.owner)?;
    let Some(transfer) = (unsafe { host_ref::<DataTransfer>(scope, owner) })
    else {
      return Err(OpError::type_("illegal invocation"));
    };
    if transfer.get().writable() {
      let _ = transfer.get().clear_items();
    }
    Ok(())
  }

  fn remove(
    &self,
    scope: &mut v8::PinScope,
    index: u32,
  ) -> Result<(), OpError> {
    let owner = owner_of(scope, &self.owner)?;
    let Some(transfer) = (unsafe { host_ref::<DataTransfer>(scope, owner) })
    else {
      return Err(OpError::type_("illegal invocation"));
    };
    if transfer.get().writable() {
      let _ = transfer
        .get()
        .remove_at(usize::try_from(index).unwrap_or(usize::MAX));
    }
    Ok(())
  }
}

fn item_list_at<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  index: u32,
) -> Option<v8::Local<'s, v8::Object>> {
  let (owner, id) = {
    let list = unsafe { host_ref::<DataTransferItemList>(scope, object) }?;
    let owner = traced_local(scope, &list.get().owner)?;
    let transfer = unsafe { host_ref::<DataTransfer>(scope, owner) }?;
    let id = transfer.get().id_at(index as usize)?;
    (owner, id)
  };
  item_wrapper(scope, owner, id)
}

fn item_list_len(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> u32 {
  let Some(list) = (unsafe { host_ref::<DataTransferItemList>(scope, object) })
  else {
    return 0;
  };
  let Some(owner) = traced_local(scope, &list.get().owner) else {
    return 0;
  };
  let Some(transfer) = (unsafe { host_ref::<DataTransfer>(scope, owner) })
  else {
    return 0;
  };
  u32::try_from(transfer.get().len()).unwrap_or(u32::MAX)
}

fn item_list_get_index(
  scope: &mut v8::PinScope,
  index: u32,
  args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Value>,
) -> v8::Intercepted {
  let Some(item) = item_list_at(scope, args.holder(), index) else {
    return v8::Intercepted::kNo;
  };
  ret.set(item.into());
  v8::Intercepted::kYes
}

fn item_list_query_index(
  scope: &mut v8::PinScope,
  index: u32,
  args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Integer>,
) -> v8::Intercepted {
  if index >= item_list_len(scope, args.holder()) {
    return v8::Intercepted::kNo;
  }
  // `PropertyAttribute::READ_ONLY`: enumerable and configurable, not writable.
  ret.set_int32(1);
  v8::Intercepted::kYes
}

fn item_list_reject_index(
  scope: &mut v8::PinScope,
  index: u32,
  args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
  if index >= item_list_len(scope, args.holder()) {
    return v8::Intercepted::kNo;
  }
  ret.set_bool(false);
  v8::Intercepted::kYes
}

fn item_list_set_index(
  scope: &mut v8::PinScope,
  index: u32,
  _value: v8::Local<v8::Value>,
  args: v8::PropertyCallbackArguments,
  ret: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
  item_list_reject_index(scope, index, args, ret)
}

fn item_list_delete_index(
  scope: &mut v8::PinScope,
  index: u32,
  args: v8::PropertyCallbackArguments,
  ret: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
  item_list_reject_index(scope, index, args, ret)
}

fn item_list_enumerate(
  scope: &mut v8::PinScope,
  args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Array>,
) {
  let count = item_list_len(scope, args.holder());
  let indices: Vec<v8::Local<v8::Value>> = (0..count)
    .map(|index| v8::Integer::new_from_unsigned(scope, index).into())
    .collect();
  ret.set(v8::Array::new_with_elements(scope, &indices));
}

/// Snapshot of the files in one `DataTransfer` read.
#[repr(C)]
struct FileList {
  files: RefCell<Vec<v8::TracedReference<v8::Object>>>,
}

unsafe impl v8::cppgc::GarbageCollected for FileList {
  fn trace(&self, visitor: &mut v8::cppgc::Visitor) {
    if let Ok(files) = self.files.try_borrow() {
      for file in files.iter() {
        visitor.trace(file);
      }
    }
  }

  fn get_name(&self) -> &'static std::ffi::CStr {
    c"FileList"
  }
}

#[op]
impl FileList {
  #[getter]
  fn length(&self) -> u32 {
    u32::try_from(self.files.borrow().len()).unwrap_or(u32::MAX)
  }

  fn item(&self, scope: &mut v8::PinScope, index: u32) -> Js {
    let local = {
      let files = self.files.borrow();
      let Some(reference) = files.get(index as usize) else {
        return Js::null(scope);
      };
      traced_local(scope, reference)
    };
    match local {
      Some(object) => js_object(scope, object),
      None => Js::null(scope),
    }
  }
}

fn file_list_count(
  scope: &mut v8::PinScope,
  object: v8::Local<v8::Object>,
) -> u32 {
  unsafe { host_ref::<FileList>(scope, object) }
    .map(|host| {
      u32::try_from(host.get().files.borrow().len()).unwrap_or(u32::MAX)
    })
    .unwrap_or(0)
}

fn file_list_at<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'_, v8::Object>,
  index: u32,
) -> Option<v8::Local<'s, v8::Object>> {
  let host = unsafe { host_ref::<FileList>(scope, object) }?;
  let files = host.get().files.borrow();
  let reference = files.get(index as usize)?;
  traced_local(scope, reference)
}

fn file_list_get_index(
  scope: &mut v8::PinScope,
  index: u32,
  args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Value>,
) -> v8::Intercepted {
  let Some(file) = file_list_at(scope, args.holder(), index) else {
    return v8::Intercepted::kNo;
  };
  ret.set(file.into());
  v8::Intercepted::kYes
}

fn file_list_query_index(
  scope: &mut v8::PinScope,
  index: u32,
  args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Integer>,
) -> v8::Intercepted {
  if index >= file_list_count(scope, args.holder()) {
    return v8::Intercepted::kNo;
  }
  ret.set_int32(1);
  v8::Intercepted::kYes
}

fn file_list_reject_index(
  scope: &mut v8::PinScope,
  index: u32,
  args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
  if index >= file_list_count(scope, args.holder()) {
    return v8::Intercepted::kNo;
  }
  ret.set_bool(false);
  v8::Intercepted::kYes
}

fn file_list_set_index(
  scope: &mut v8::PinScope,
  index: u32,
  _value: v8::Local<v8::Value>,
  args: v8::PropertyCallbackArguments,
  ret: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
  file_list_reject_index(scope, index, args, ret)
}

fn file_list_delete_index(
  scope: &mut v8::PinScope,
  index: u32,
  args: v8::PropertyCallbackArguments,
  ret: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
  file_list_reject_index(scope, index, args, ret)
}

fn file_list_enumerate(
  scope: &mut v8::PinScope,
  args: v8::PropertyCallbackArguments,
  mut ret: v8::ReturnValue<v8::Array>,
) {
  let count = file_list_count(scope, args.holder());
  let indices: Vec<v8::Local<v8::Value>> = (0..count)
    .map(|index| v8::Integer::new_from_unsigned(scope, index).into())
    .collect();
  ret.set(v8::Array::new_with_elements(scope, &indices));
}

fn install_item_list_iterator(scope: &mut v8::PinScope) {
  let templ = DataTransferItemList::template(scope);
  let Some(ctor) = templ.get_function(scope) else {
    return;
  };
  let Some(key) = v8::String::new(scope, "prototype") else {
    return;
  };
  let Some(prototype) = ctor.get(scope, key.into()) else {
    return;
  };
  let Ok(prototype) = v8::Local::<v8::Object>::try_from(prototype) else {
    return;
  };
  let Some(function) = v8::Function::new(scope, item_list_iterator) else {
    return;
  };
  let symbol = v8::Symbol::get_iterator(scope);
  let _ = prototype.define_own_property(
    scope,
    symbol.into(),
    function.into(),
    v8::PropertyAttribute::DONT_ENUM,
  );
}

fn item_list_iterator(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let (owner, ids) = {
    let Some(list) =
      (unsafe { host_ref::<DataTransferItemList>(scope, args.this()) })
    else {
      OpException::throw(OpError::type_("illegal invocation"), scope);
      return;
    };
    let Some(owner) = traced_local(scope, &list.get().owner) else {
      OpException::throw(OpError::type_("illegal invocation"), scope);
      return;
    };
    let Some(transfer) = (unsafe { host_ref::<DataTransfer>(scope, owner) })
    else {
      OpException::throw(OpError::type_("illegal invocation"), scope);
      return;
    };
    let count = transfer.get().len();
    let ids = (0..count)
      .filter_map(|index| transfer.get().id_at(index))
      .collect::<Vec<_>>();
    (owner, ids)
  };
  let values = ids
    .iter()
    .filter_map(|id| item_wrapper(scope, owner, *id))
    .collect::<Vec<_>>();
  let array = v8::Array::new(scope, values.len() as i32);
  for (index, object) in values.iter().enumerate() {
    yuzuko_core::define_element(
      scope,
      array.into(),
      index as u32,
      (*object).into(),
    );
  }
  return_array_iterator(scope, array, &mut ret);
}

fn return_array_iterator(
  scope: &mut v8::PinScope,
  array: v8::Local<v8::Array>,
  ret: &mut v8::ReturnValue,
) {
  let symbol = v8::Symbol::get_iterator(scope);
  if let Some(next) = array.get(scope, symbol.into())
    && let Ok(next) = v8::Local::<v8::Function>::try_from(next)
    && let Some(iterator) = next.call(scope, array.into(), &[])
  {
    ret.set(iterator);
  }
}

fn install_file_list_iterator(scope: &mut v8::PinScope) {
  let templ = FileList::template(scope);
  let Some(ctor) = templ.get_function(scope) else {
    return;
  };
  let Some(key) = v8::String::new(scope, "prototype") else {
    return;
  };
  let Some(prototype) = ctor.get(scope, key.into()) else {
    return;
  };
  let Ok(prototype) = v8::Local::<v8::Object>::try_from(prototype) else {
    return;
  };
  let Some(function) = v8::Function::new(scope, file_list_iterator) else {
    return;
  };
  let symbol = v8::Symbol::get_iterator(scope);
  let _ = prototype.define_own_property(
    scope,
    symbol.into(),
    function.into(),
    v8::PropertyAttribute::DONT_ENUM,
  );
}

fn file_list_iterator(
  scope: &mut v8::PinScope,
  args: v8::FunctionCallbackArguments,
  mut ret: v8::ReturnValue,
) {
  let Some(host) = (unsafe { host_ref::<FileList>(scope, args.this()) }) else {
    OpException::throw(OpError::type_("illegal invocation"), scope);
    return;
  };
  let locals = {
    let files = host.get().files.borrow();
    files
      .iter()
      .filter_map(|reference| traced_local(scope, reference))
      .collect::<Vec<_>>()
  };
  let array = v8::Array::new(scope, locals.len() as i32);
  for (index, object) in locals.iter().enumerate() {
    yuzuko_core::define_element(
      scope,
      array.into(),
      index as u32,
      (*object).into(),
    );
  }
  return_array_iterator(scope, array, &mut ret);
}
