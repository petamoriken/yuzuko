//! `#[op] impl` builds a cppgc-backed JavaScript class.
//!
//! ```ignore
//! #[op]
//! impl Counter {
//!   #[constructor]
//!   fn new(start: Option<i32>) -> Counter { ... }
//!
//!   #[getter]
//!   fn value(&self) -> i32 { ... }
//!
//!   #[setter]
//!   fn value(&self, value: i32) { ... }
//!
//!   fn bump(&self, by: i32) -> i32 { ... }
//!
//!   #[static_method]
//!   fn answer() -> i32 { 42 }
//! }
//! ```
//!
//! `#[op(inherit = Parent)]` sets the prototype. The Rust struct is
//! `#[repr(C)]` with a first field named `base`. `#[rename("encodeInto")]`
//! sets the JavaScript name. A snake_case Rust name otherwise becomes
//! camelCase. `#[this]` passes the JavaScript receiver instead of unwrapping
//! it. `#[construct_hook]` runs after the instance is wrapped and is not
//! installed. The Rust type implements `v8::cppgc::GarbageCollected`.

use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::FnArg;
use syn::GenericArgument;
use syn::ImplItem;
use syn::ImplItemFn;
use syn::ItemImpl;
use syn::LitInt;
use syn::LitStr;
use syn::PathArguments;
use syn::Receiver;
use syn::ReturnType;
use syn::Token;
use syn::Type;
use syn::parse::ParseStream;
use syn::spanned::Spanned;

use crate::is_result;
use crate::is_scope;
use crate::type_name;

enum Kind {
  Constructor,
  Method,
  Getter,
  Setter,
  Static,
  ConstructHook,
}

struct Member {
  kind: Kind,
  js_name: String,
  rust_name: syn::Ident,
  callback: syn::Ident,
  inner: syn::Ident,
  signature: syn::Signature,
  block: syn::Block,
  vis: syn::Visibility,
  /// Web IDL operation length: required arguments before the first optional.
  length: i32,
  /// The operation returns a promise, so a bad `this` or too few arguments
  /// reject that promise instead of throwing.
  promise: bool,
}

struct ClassArgs {
  inherit: Option<Type>,
  name: Option<String>,
  tag: Option<u16>,
}

impl syn::parse::Parse for ClassArgs {
  fn parse(input: ParseStream) -> syn::Result<Self> {
    let mut inherit = None;
    let mut name = None;
    let mut tag = None;
    while !input.is_empty() {
      let ident: syn::Ident = input.parse()?;
      input.parse::<Token![=]>()?;
      if ident == "inherit" {
        inherit = Some(input.parse()?);
      } else if ident == "name" {
        let lit: LitStr = input.parse()?;
        name = Some(lit.value());
      } else if ident == "tag" {
        let lit: LitInt = input.parse()?;
        let value = lit.base10_parse::<u16>()?;
        if value == 0 || value >= 0x7fff {
          return Err(syn::Error::new(
            lit.span(),
            "cppgc tag must be in 1..=32766",
          ));
        }
        tag = Some(value);
      } else {
        return Err(syn::Error::new(
          ident.span(),
          "expected inherit, name, or tag",
        ));
      }
      if input.peek(Token![,]) {
        input.parse::<Token![,]>()?;
      }
    }
    Ok(Self { inherit, name, tag })
  }
}

pub(crate) fn expand(attr: TokenStream, item: ItemImpl) -> TokenStream {
  match expand_result(attr, item) {
    Ok(tokens) => tokens,
    Err(error) => error.to_compile_error(),
  }
}

fn expand_result(
  attr: TokenStream,
  mut item: ItemImpl,
) -> syn::Result<TokenStream> {
  if item.trait_.is_some() {
    return Err(syn::Error::new(
      item.impl_token.span(),
      "#[op] implements a class, not a trait",
    ));
  }
  if !item.generics.params.is_empty() {
    return Err(syn::Error::new(
      item.generics.span(),
      "#[op] classes cannot be generic",
    ));
  }
  let args: ClassArgs = syn::parse2(attr)?;
  let self_ty = item.self_ty.as_ref().clone();
  let class_name = args.name.unwrap_or_else(|| type_ident(&self_ty));
  let _ = args.tag;
  let inherit = args.inherit;

  let mut extras = Vec::new();
  let mut members = Vec::new();
  for impl_item in item.items.drain(..) {
    let ImplItem::Fn(method) = impl_item else {
      extras.push(impl_item);
      continue;
    };
    members.push(classify(method)?);
  }
  if members.is_empty() {
    return Err(syn::Error::new(
      item.impl_token.span(),
      "#[op] impl needs a constructor, method, or accessor",
    ));
  }
  if members
    .iter()
    .filter(|member| matches!(member.kind, Kind::Constructor))
    .count()
    > 1
  {
    return Err(syn::Error::new(
      item.impl_token.span(),
      "a class can have one #[constructor]",
    ));
  }

  let hook = members
    .iter()
    .find(|member| matches!(member.kind, Kind::ConstructHook));
  if hook.is_some()
    && !members
      .iter()
      .any(|member| matches!(member.kind, Kind::Constructor))
  {
    return Err(syn::Error::new(
      item.impl_token.span(),
      "#[construct_hook] requires a #[constructor]",
    ));
  }
  if members
    .iter()
    .filter(|member| matches!(member.kind, Kind::ConstructHook))
    .count()
    > 1
  {
    return Err(syn::Error::new(
      item.impl_token.span(),
      "a class can have one #[construct_hook]",
    ));
  }
  let hook_call = hook_call(hook);

  let mut callbacks = Vec::new();
  let mut inners = Vec::new();
  for member in &members {
    if matches!(member.kind, Kind::ConstructHook) {
      inners.push(plain_inner(member));
      continue;
    }
    let (inner, callback) = emit_member(&self_ty, member, &hook_call)?;
    inners.push(inner);
    callbacks.push(callback);
  }

  let constructor = members
    .iter()
    .find(|member| matches!(member.kind, Kind::Constructor));
  let constructor_length = constructor.map(|member| member.length).unwrap_or(0);
  let constructor_callback = match constructor {
    Some(member) => {
      let callback = &member.callback;
      quote! { Self::#callback }
    }
    None => quote! { ::yuzuko_core::op::illegal_constructor },
  };
  let inherit_stmt = inherit.as_ref().map(|parent| {
    quote! {
      let parent = #parent::template(scope);
      templ.inherit(parent);
      ::yuzuko_core::op::register_subclass::<Self, #parent>(
        ::std::mem::offset_of!(Self, base),
      );
    }
  });
  let inherit_object = inherit.as_ref().map(|parent| {
    quote! {
      let parent_ctor = #parent::template(scope).get_function(scope).unwrap();
      let _ = ctor.set_prototype(scope, parent_ctor.into());
    }
  });
  let method_sets = members.iter().filter_map(|member| {
    if !matches!(member.kind, Kind::Method) {
      return None;
    }
    let js_name = &member.js_name;
    let callback = &member.callback;
    let length = member.length;
    Some(quote! {
      let key = ::v8::String::new(scope, #js_name).unwrap();
      let function = ::v8::FunctionTemplate::builder(Self::#callback)
        .length(#length)
        .constructor_behavior(::v8::ConstructorBehavior::Throw)
        .build(scope);
      let function_name = ::v8::String::new(scope, #js_name).unwrap();
      function.set_class_name(function_name);
      proto.set(key.into(), function.into());
    })
  });
  let static_sets = members.iter().filter_map(|member| {
    if !matches!(member.kind, Kind::Static) {
      return None;
    }
    let js_name = &member.js_name;
    let callback = &member.callback;
    let length = member.length;
    Some(quote! {
      let key = ::v8::String::new(scope, #js_name).unwrap();
      let function = ::v8::FunctionTemplate::builder(Self::#callback)
        .length(#length)
        .constructor_behavior(::v8::ConstructorBehavior::Throw)
        .build(scope);
      let function_name = ::v8::String::new(scope, #js_name).unwrap();
      function.set_class_name(function_name);
      templ.set(key.into(), function.into());
    })
  });
  let method_renames = members.iter().filter_map(|member| {
    if !matches!(member.kind, Kind::Method) {
      return None;
    }
    let js_name = &member.js_name;
    Some(quote! {
      let key = ::v8::String::new(scope, #js_name).unwrap();
      if let Some(value) = proto.get(scope, key.into())
        && let Ok(function) =
          ::v8::Local::<::v8::Function>::try_from(value)
      {
        function.set_name(key);
      }
    })
  });
  let static_renames = members.iter().filter_map(|member| {
    if !matches!(member.kind, Kind::Static) {
      return None;
    }
    let js_name = &member.js_name;
    Some(quote! {
      let key = ::v8::String::new(scope, #js_name).unwrap();
      if let Some(value) = ctor.get(scope, key.into())
        && let Ok(function) =
          ::v8::Local::<::v8::Function>::try_from(value)
      {
        function.set_name(key);
      }
    })
  });
  let accessor_sets = accessor_quotes(&members)?;

  Ok(quote! {
    impl #self_ty {
      #(#extras)*
      #(#inners)*
      #(#callbacks)*

      pub fn template<'s>(
        scope: &mut ::v8::PinScope<'s, '_>,
      ) -> ::v8::Local<'s, ::v8::FunctionTemplate> {
        if let Some(existing) =
          ::yuzuko_core::op::cached_template::<Self>(scope)
        {
          return existing;
        }
        let templ = ::v8::FunctionTemplate::builder(#constructor_callback)
          .length(#constructor_length)
          .build(scope);
        let class_name = ::v8::String::new(scope, #class_name).unwrap();
        templ.set_class_name(class_name);
        #inherit_stmt
        templ.read_only_prototype();
        let proto = templ.prototype_template(scope);
        let tag_key = ::v8::Symbol::get_to_string_tag(scope);
        proto.set(tag_key.into(), class_name.into());
        #(#method_sets)*
        #(#static_sets)*
        ::yuzuko_core::op::cache_template::<Self>(scope, templ);
        templ
      }

      pub fn install(scope: &mut ::v8::PinScope) {
        let templ = Self::template(scope);
        let ctor = templ.get_function(scope).unwrap();
        let proto_key = ::v8::String::new(scope, "prototype").unwrap();
        let proto = ctor.get(scope, proto_key.into()).unwrap();
        let proto = ::v8::Local::<::v8::Object>::try_from(proto).unwrap();
        #inherit_object
        #(#method_renames)*
        #(#static_renames)*
        #(#accessor_sets)*
        let global = scope.get_current_context().global(scope);
        let key = ::v8::String::new(scope, #class_name).unwrap();
        let _ = global.define_own_property(
          scope,
          key.into(),
          ctor.into(),
          ::v8::PropertyAttribute::DONT_ENUM,
        );
      }
    }
  })
}

struct Property {
  js_name: String,
  getter: Option<syn::Ident>,
  setter: Option<syn::Ident>,
}

fn accessor_quotes(members: &[Member]) -> syn::Result<Vec<TokenStream>> {
  let mut properties = Vec::<Property>::new();
  for member in members {
    match member.kind {
      Kind::Getter => {
        let property = property_slot(&mut properties, &member.js_name);
        if property.getter.is_some() {
          return Err(syn::Error::new(
            member.rust_name.span(),
            "duplicate getter",
          ));
        }
        property.getter = Some(member.callback.clone());
      }
      Kind::Setter => {
        let property = property_slot(&mut properties, &member.js_name);
        if property.setter.is_some() {
          return Err(syn::Error::new(
            member.rust_name.span(),
            "duplicate setter",
          ));
        }
        property.setter = Some(member.callback.clone());
      }
      _ => {}
    }
  }
  let mut quotes = Vec::new();
  for property in properties {
    if property.getter.is_none() {
      return Err(syn::Error::new(
        proc_macro2::Span::call_site(),
        format!("#[setter] {} needs a #[getter]", property.js_name),
      ));
    }
    let js_name = property.js_name;
    let getter_label = format!("get {js_name}");
    let getter = property.getter.expect("getter");
    quotes.push(match property.setter {
      Some(setter) => {
        let setter_label = format!("set {js_name}");
        quote! {
          let key = ::v8::String::new(scope, #js_name).unwrap();
          let getter = ::v8::Function::builder(Self::#getter)
            .length(0)
            .constructor_behavior(::v8::ConstructorBehavior::Throw)
            .build(scope)
            .unwrap();
          let getter_name = ::v8::String::new(scope, #getter_label).unwrap();
          getter.set_name(getter_name);
          let setter = ::v8::Function::builder(Self::#setter)
            .length(1)
            .constructor_behavior(::v8::ConstructorBehavior::Throw)
            .build(scope)
            .unwrap();
          let setter_name = ::v8::String::new(scope, #setter_label).unwrap();
          setter.set_name(setter_name);
          let get: ::v8::Local<::v8::Value> = getter.into();
          let set: ::v8::Local<::v8::Value> = setter.into();
          let mut descriptor =
            ::v8::PropertyDescriptor::new_from_get_set(get, set);
          descriptor.set_enumerable(true);
          descriptor.set_configurable(true);
          let _ = proto.define_property(scope, key.into(), &descriptor);
        }
      }
      None => quote! {
        let key = ::v8::String::new(scope, #js_name).unwrap();
        let getter = ::v8::Function::builder(Self::#getter)
          .length(0)
          .constructor_behavior(::v8::ConstructorBehavior::Throw)
          .build(scope)
          .unwrap();
        let getter_name = ::v8::String::new(scope, #getter_label).unwrap();
        getter.set_name(getter_name);
        let get: ::v8::Local<::v8::Value> = getter.into();
        let set: ::v8::Local<::v8::Value> = ::v8::undefined(scope).into();
        let mut descriptor =
          ::v8::PropertyDescriptor::new_from_get_set(get, set);
        descriptor.set_enumerable(true);
        descriptor.set_configurable(true);
        let _ = proto.define_property(scope, key.into(), &descriptor);
      },
    });
  }
  Ok(quotes)
}

fn property_slot<'a>(
  properties: &'a mut Vec<Property>,
  js_name: &str,
) -> &'a mut Property {
  if let Some(index) = properties
    .iter()
    .position(|property| property.js_name == js_name)
  {
    return &mut properties[index];
  }
  properties.push(Property {
    js_name: js_name.to_string(),
    getter: None,
    setter: None,
  });
  properties.last_mut().expect("property")
}

fn classify(mut method: ImplItemFn) -> syn::Result<Member> {
  if method.sig.asyncness.is_some() {
    return Err(syn::Error::new(
      method.sig.fn_token.span(),
      "async ops are not supported yet",
    ));
  }
  let mut kind = None;
  let mut rename = None;
  let mut length_override = None;
  let mut promise = false;
  let mut kept = Vec::new();
  for attr in method.attrs.drain(..) {
    if attr.path().is_ident("constructor") {
      kind = Some(Kind::Constructor);
    } else if attr.path().is_ident("getter") {
      kind = Some(Kind::Getter);
    } else if attr.path().is_ident("setter") {
      kind = Some(Kind::Setter);
    } else if attr.path().is_ident("static_method") {
      kind = Some(Kind::Static);
    } else if attr.path().is_ident("method") {
      kind = Some(Kind::Method);
    } else if attr.path().is_ident("construct_hook") {
      kind = Some(Kind::ConstructHook);
    } else if attr.path().is_ident("rename") {
      let lit: LitStr = attr.parse_args()?;
      rename = Some(lit.value());
    } else if attr.path().is_ident("length") {
      let lit: LitInt = attr.parse_args()?;
      length_override = Some(lit.base10_parse::<i32>()?);
    } else if attr.path().is_ident("promise") {
      promise = true;
    } else {
      kept.push(attr);
    }
  }
  method.attrs = kept;
  let receiver = receiver_of(&method);
  let has_this = method.sig.inputs.iter().any(is_this_param);
  let kind = match (kind, receiver.is_some(), has_this) {
    (Some(kind), _, _) => kind,
    (None, true, _) | (None, false, true) => Kind::Method,
    (None, false, false) => {
      return Err(syn::Error::new(
        method.sig.ident.span(),
        "mark this function with #[constructor] or #[static_method]",
      ));
    }
  };
  match kind {
    Kind::Constructor | Kind::Static if receiver.is_some() => {
      return Err(syn::Error::new(
        method.sig.ident.span(),
        "constructors and static methods do not take self",
      ));
    }
    Kind::Method | Kind::Getter | Kind::Setter
      if receiver.is_none() && !has_this =>
    {
      return Err(syn::Error::new(
        method.sig.ident.span(),
        "methods and accessors take &self or a #[this] receiver",
      ));
    }
    Kind::ConstructHook if receiver.is_none() => {
      return Err(syn::Error::new(
        method.sig.ident.span(),
        "#[construct_hook] takes &self, the scope, and the wrapped object",
      ));
    }
    _ => {}
  }
  if let Some(receiver) = receiver
    && (receiver.reference.is_none() || receiver.mutability.is_some())
  {
    return Err(syn::Error::new(
      receiver.span(),
      "cppgc methods take &self; mutate through interior mutability",
    ));
  }
  let rust_name = method.sig.ident.clone();
  let js_name = rename.unwrap_or_else(|| camel_case(&rust_name.to_string()));
  let length = length_override.unwrap_or_else(|| js_length(&method.sig));
  let role = match kind {
    Kind::Constructor => "ctor",
    Kind::Method => "call",
    Kind::Getter => "get",
    Kind::Setter => "set",
    Kind::Static => "static",
    Kind::ConstructHook => "hook",
  };
  Ok(Member {
    kind,
    js_name,
    callback: format_ident!("op_{role}_{}", rust_name),
    inner: format_ident!("{rust_name}_{role}_impl"),
    rust_name,
    signature: method.sig,
    block: method.block,
    vis: method.vis,
    length,
    promise,
  })
}

/// Required arguments before the first `Option`, which stands for an
/// optional Web IDL argument. `scope`, `self`, `#[this]`, and the callback
/// arguments object are not JavaScript arguments.
fn js_length(signature: &syn::Signature) -> i32 {
  let mut length = 0i32;
  for arg in &signature.inputs {
    let FnArg::Typed(typed) = arg else {
      continue;
    };
    let ty = &*typed.ty;
    if is_scope(ty) || is_callback_args(ty) || is_this_param(arg) {
      continue;
    }
    if option_inner(ty).is_some() {
      break;
    }
    length += 1;
  }
  length
}

fn plain_inner(member: &Member) -> TokenStream {
  let mut inner_sig = member.signature.clone();
  inner_sig.ident = member.inner.clone();
  strip_param_attrs(&mut inner_sig);
  let vis = &member.vis;
  let block = &member.block;
  quote! { #vis #inner_sig #block }
}

fn hook_call(hook: Option<&Member>) -> TokenStream {
  let Some(hook) = hook else {
    return quote! {};
  };
  let inner = &hook.inner;
  let call = quote! {
    if let Some(wrapped) = unsafe {
      ::yuzuko_core::op::host_ref::<Self>(scope, object)
    } {
      Self::#inner(wrapped.get(), scope, object)
    }
  };
  if is_result_return(&hook.signature) {
    quote! {
      if let Some(wrapped) = unsafe {
        ::yuzuko_core::op::host_ref::<Self>(scope, object)
      } {
        if let Err(error) = Self::#inner(wrapped.get(), scope, object) {
          ::yuzuko_core::op::OpException::throw(error, scope);
          return;
        }
      }
    }
  } else {
    call
  }
}

fn emit_member(
  self_ty: &Type,
  member: &Member,
  hook_call: &TokenStream,
) -> syn::Result<(TokenStream, TokenStream)> {
  let has_self = member
    .signature
    .inputs
    .iter()
    .any(|arg| matches!(arg, FnArg::Receiver(_)));
  let (extracts, call_args, js_count) =
    extract_args(&member.rust_name, &member.signature, has_self)?;
  match member.kind {
    Kind::Getter if js_count != 0 => {
      return Err(syn::Error::new(
        member.rust_name.span(),
        "a getter takes no JavaScript arguments",
      ));
    }
    Kind::Setter if js_count != 1 => {
      return Err(syn::Error::new(
        member.rust_name.span(),
        "a setter takes one JavaScript argument",
      ));
    }
    _ => {}
  }
  if matches!(member.kind, Kind::Constructor) {
    let return_ty = match &member.signature.output {
      ReturnType::Type(_, ty) => ty.as_ref(),
      ReturnType::Default => {
        return Err(syn::Error::new(
          member.rust_name.span(),
          "a constructor returns Self",
        ));
      }
    };
    if !returns_self(return_ty, self_ty) {
      return Err(syn::Error::new(
        member.signature.output.span(),
        "a constructor returns Self or Result<Self, _>",
      ));
    }
  }

  let inner_ident = &member.inner;
  let mut inner_sig = member.signature.clone();
  inner_sig.ident = inner_ident.clone();
  strip_param_attrs(&mut inner_sig);
  let vis = &member.vis;
  let block = &member.block;
  let inner = quote! { #vis #inner_sig #block };
  let callback = &member.callback;
  let call = quote! { Self::#inner_ident(#(#call_args),*) };
  let unwrap_this = if has_self {
    unwrap_this(member.promise)
  } else {
    quote! {}
  };
  let arity = if matches!(member.kind, Kind::Setter | Kind::Constructor) {
    quote! {}
  } else {
    arity_check(member.promise, member.length)
  };

  let callback_tokens = match member.kind {
    Kind::Constructor => {
      let construct = if is_result_return(&member.signature) {
        quote! {
          let value = match #call {
            Ok(value) => value,
            Err(error) => {
              ::yuzuko_core::op::OpException::throw(error, scope);
              return;
            }
          };
        }
      } else {
        quote! { let value = #call; }
      };
      quote! {
        fn #callback(
          scope: &mut ::v8::PinScope,
          args: ::v8::FunctionCallbackArguments,
          mut ret: ::v8::ReturnValue,
        ) {
          if !args.is_construct_call() {
            ::yuzuko_core::op::OpException::throw(
              ::yuzuko_core::op::OpError::type_(
                "constructor must be called with new",
              ),
              scope,
            );
            return;
          }
          #(#extracts)*
          #construct
          let receiver = ::yuzuko_core::op::root(scope, args.this());
          let object = unsafe {
            ::yuzuko_core::op::wrap_receiver::<Self>(
              scope,
              Some(receiver),
              value,
            )
          };
          #hook_call
          ret.set(object.into());
        }
      }
    }
    Kind::Getter => {
      let body = return_body(&member.signature, &call);
      quote! {
        fn #callback(
          scope: &mut ::v8::PinScope,
          args: ::v8::FunctionCallbackArguments,
          mut ret: ::v8::ReturnValue,
        ) {
          let receiver = args.this();
          #unwrap_this
          #arity
          #(#extracts)*
          #body
        }
      }
    }
    Kind::Setter => {
      let body = if is_result_return(&member.signature) {
        quote! {
          if let Err(error) = #call {
            ::yuzuko_core::op::OpException::throw(error, scope);
          }
        }
      } else {
        quote! { #call; }
      };
      quote! {
        fn #callback(
          scope: &mut ::v8::PinScope,
          args: ::v8::FunctionCallbackArguments,
          mut _ret: ::v8::ReturnValue,
        ) {
          let receiver = args.this();
          #unwrap_this
          #(#extracts)*
          #body
        }
      }
    }
    Kind::ConstructHook => {
      return Err(syn::Error::new(
        member.rust_name.span(),
        "#[construct_hook] is not a JavaScript method",
      ));
    }
    Kind::Method | Kind::Static => {
      let body = return_body(&member.signature, &call);
      let open = if matches!(member.kind, Kind::Method) {
        quote! {
          let receiver = args.this();
          #unwrap_this
        }
      } else {
        quote! {}
      };
      quote! {
        fn #callback(
          scope: &mut ::v8::PinScope,
          args: ::v8::FunctionCallbackArguments,
          mut ret: ::v8::ReturnValue,
        ) {
          #open
          #arity
          #(#extracts)*
          #body
        }
      }
    }
  };
  Ok((inner, callback_tokens))
}

fn unwrap_this(promise: bool) -> TokenStream {
  let fail = if promise {
    reject_type("illegal invocation")
  } else {
    quote! {
      ::yuzuko_core::op::OpException::throw(
        ::yuzuko_core::op::OpError::type_("illegal invocation"),
        scope,
      );
      return;
    }
  };
  quote! {
    let wrapped = match unsafe {
      ::yuzuko_core::op::host_ref::<Self>(scope, receiver)
    } {
      Some(wrapped) => wrapped,
      None => {
        #fail
      }
    };
    let this = wrapped.get();
  }
}

fn arity_check(promise: bool, length: i32) -> TokenStream {
  if length <= 0 {
    return quote! {};
  }
  let fail = if promise {
    reject_type("Not enough arguments")
  } else {
    quote! {
      ::yuzuko_core::op::OpException::throw(
        ::yuzuko_core::op::OpError::type_("Not enough arguments"),
        scope,
      );
      return;
    }
  };
  quote! {
    if args.length() < #length {
      #fail
    }
  }
}

fn reject_type(message: &str) -> TokenStream {
  quote! {
    let resolver = ::v8::PromiseResolver::new(scope).unwrap();
    let message = ::v8::String::new(scope, #message).unwrap();
    let error = ::v8::Exception::type_error(scope, message);
    resolver.reject(scope, error);
    ret.set(resolver.get_promise(scope).into());
    return;
  }
}

fn return_body(signature: &syn::Signature, call: &TokenStream) -> TokenStream {
  match &signature.output {
    ReturnType::Type(_, ty) if is_result(ty) => quote! {
      match #call {
        Ok(value) => ::yuzuko_core::op::ToV8::to_v8(value, scope, &mut ret),
        Err(error) => ::yuzuko_core::op::OpException::throw(error, scope),
      }
    },
    ReturnType::Default => quote! { #call; },
    _ => quote! {
      let value = #call;
      ::yuzuko_core::op::ToV8::to_v8(value, scope, &mut ret);
    },
  }
}

fn extract_args(
  name: &syn::Ident,
  signature: &syn::Signature,
  has_self: bool,
) -> syn::Result<(Vec<TokenStream>, Vec<TokenStream>, i32)> {
  let mut extracts = Vec::new();
  let mut call_args = Vec::new();
  if has_self {
    call_args.push(quote! { this });
  }
  let mut js_index: i32 = 0;
  let mut bind_index: i32 = 0;
  let mut saw_js = false;
  for arg in &signature.inputs {
    if matches!(arg, FnArg::Receiver(_)) {
      continue;
    }
    let FnArg::Typed(typed) = arg else {
      continue;
    };
    let ty = &*typed.ty;
    if is_scope(ty) {
      if saw_js {
        return Err(syn::Error::new(
          ty.span(),
          "the scope argument must come before JavaScript arguments",
        ));
      }
      call_args.push(quote! { scope });
      continue;
    }
    if is_callback_args(ty) {
      if saw_js {
        return Err(syn::Error::new(
          ty.span(),
          "the arguments parameter must come before JavaScript arguments",
        ));
      }
      call_args.push(quote! { &args });
      continue;
    }
    if is_this_param(arg) {
      call_args.push(quote! { receiver });
      continue;
    }
    saw_js = true;
    let binding = format_ident!("arg{bind_index}");
    bind_index += 1;
    let source = quote! { args.get(#js_index) };
    extracts.push(extract_one(name, &binding, ty, &source)?);
    call_args.push(quote! { #binding });
    js_index += 1;
  }
  Ok((extracts, call_args, js_index))
}

fn extract_one(
  name: &syn::Ident,
  binding: &syn::Ident,
  ty: &Type,
  source: &TokenStream,
) -> syn::Result<TokenStream> {
  if let Some(inner) = option_inner(ty)
    && is_local(inner)
  {
    let expected = type_name(inner);
    return Ok(quote! {
        let #binding = if #source.is_undefined() {
          None
        } else {
          match ::std::convert::TryFrom::try_from(#source) {
            Ok(value) => Some(value),
            Err(_) => {
              ::yuzuko_core::op::OpException::throw(
                ::yuzuko_core::op::OpError::type_(
                  ::std::concat!("failed to convert argument to ", #expected),
                ),
                scope,
              );
              return;
            }
          }
      };
    });
  }
  if is_local(ty) {
    let expected = type_name(ty);
    return Ok(quote! {
      let #binding = match ::std::convert::TryFrom::try_from(#source) {
        Ok(value) => value,
        Err(_) => {
          ::yuzuko_core::op::OpException::throw(
            ::yuzuko_core::op::OpError::type_(
              ::std::concat!("failed to convert argument to ", #expected),
            ),
            scope,
          );
          return;
        }
      };
    });
  }
  let _ = name;
  Ok(quote! {
    let #binding = match <#ty as ::yuzuko_core::op::FromV8>::from_v8(
      scope,
      #source,
    ) {
      Ok(value) => value,
      Err(error) => {
        ::yuzuko_core::op::OpException::throw(error, scope);
        return;
      }
    };
  })
}

fn is_this_param(arg: &FnArg) -> bool {
  let FnArg::Typed(typed) = arg else {
    return false;
  };
  typed.attrs.iter().any(|attr| attr.path().is_ident("this"))
}

fn strip_param_attrs(signature: &mut syn::Signature) {
  for arg in signature.inputs.iter_mut() {
    let FnArg::Typed(typed) = arg else {
      continue;
    };
    typed.attrs.retain(|attr| !attr.path().is_ident("this"));
  }
}

fn is_callback_args(ty: &Type) -> bool {
  let Type::Reference(reference) = ty else {
    return false;
  };
  type_name(&reference.elem).contains("FunctionCallbackArguments")
}

fn receiver_of(method: &ImplItemFn) -> Option<&Receiver> {
  method.sig.inputs.iter().find_map(|arg| match arg {
    FnArg::Receiver(receiver) => Some(receiver),
    _ => None,
  })
}

fn is_result_return(signature: &syn::Signature) -> bool {
  match &signature.output {
    ReturnType::Type(_, ty) => is_result(ty),
    ReturnType::Default => false,
  }
}

fn returns_self(ty: &Type, self_ty: &Type) -> bool {
  if is_self(ty) || ty == self_ty {
    return true;
  }
  option_inner_result(ty)
    .is_some_and(|inner| is_self(inner) || inner == self_ty)
}

fn option_inner_result(ty: &Type) -> Option<&Type> {
  if !is_result(ty) {
    return None;
  }
  let Type::Path(path) = ty else {
    return None;
  };
  let args = path.path.segments.last()?;
  let PathArguments::AngleBracketed(args) = &args.arguments else {
    return None;
  };
  args.args.iter().find_map(|arg| match arg {
    GenericArgument::Type(ty) => Some(ty),
    _ => None,
  })
}

fn option_inner(ty: &Type) -> Option<&Type> {
  let Type::Path(path) = ty else {
    return None;
  };
  let segment = path.path.segments.last()?;
  if segment.ident != "Option" {
    return None;
  }
  let PathArguments::AngleBracketed(args) = &segment.arguments else {
    return None;
  };
  args.args.iter().find_map(|arg| match arg {
    GenericArgument::Type(ty) => Some(ty),
    _ => None,
  })
}

fn is_self(ty: &Type) -> bool {
  matches!(ty, Type::Path(path) if path.path.is_ident("Self"))
}

fn is_local(ty: &Type) -> bool {
  type_name(ty).starts_with("Local<")
}

fn type_ident(ty: &Type) -> String {
  let name = type_name(ty);
  if name.is_empty() {
    "Object".to_string()
  } else {
    name
  }
}

fn camel_case(name: &str) -> String {
  let mut out = String::new();
  let mut upper = false;
  for ch in name.chars() {
    if ch == '_' {
      upper = true;
    } else if upper {
      out.extend(ch.to_uppercase());
      upper = false;
    } else {
      out.push(ch);
    }
  }
  out
}
