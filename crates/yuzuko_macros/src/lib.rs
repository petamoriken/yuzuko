//! `#[op]` turns a Rust function, or an inherent impl, into V8 callbacks.
//!
//! A function becomes one callback. An impl becomes a JavaScript class:
//! `#[constructor]`, `#[getter]`, `#[setter]`, `#[static_method]`, and methods
//! that take `&self`. The type must implement `v8::cppgc::GarbageCollected`.
//! Fast calls and async ops are not supported yet.

use proc_macro::TokenStream;
use quote::quote;
use syn::FnArg;
use syn::GenericArgument;
use syn::ItemFn;
use syn::ItemImpl;
use syn::PathArguments;
use syn::ReturnType;
use syn::Type;

mod class;

#[proc_macro_attribute]
pub fn op(attr: TokenStream, item: TokenStream) -> TokenStream {
  let item_for_impl = item.clone();
  if let Ok(function) = syn::parse::<ItemFn>(item) {
    return expand_function(function);
  }
  match syn::parse::<ItemImpl>(item_for_impl) {
    Ok(item) => class::expand(attr.into(), item).into(),
    Err(error) => error.to_compile_error().into(),
  }
}

fn expand_function(function: ItemFn) -> TokenStream {
  if function.sig.asyncness.is_some() {
    return syn::Error::new_spanned(
      function.sig.fn_token,
      "async ops are not supported yet",
    )
    .to_compile_error()
    .into();
  }

  let vis = &function.vis;
  let attrs = &function.attrs;
  let name = &function.sig.ident;
  let inner_name = syn::Ident::new(&format!("{name}_impl"), name.span());
  let mut inner = function.clone();
  inner.sig.ident = inner_name.clone();
  inner.vis = syn::Visibility::Public(syn::token::Pub::default());
  inner.attrs.clear();

  let mut js_index: i32 = 0;
  let mut bind_index: i32 = 0;
  let mut extracts = Vec::new();
  let mut call_args = Vec::new();
  let mut saw_plain = false;

  for arg in &function.sig.inputs {
    let FnArg::Typed(typed) = arg else {
      return syn::Error::new_spanned(arg, "ops cannot take self")
        .to_compile_error()
        .into();
    };
    let ty = &*typed.ty;
    if is_scope(ty) {
      if saw_plain || js_index > 0 {
        return syn::Error::new_spanned(
          ty,
          "the scope argument must come before JavaScript arguments",
        )
        .to_compile_error()
        .into();
      }
      call_args.push(quote! { scope });
      continue;
    }
    saw_plain = true;
    // `This` is taken from the receiver and does not consume a JS argument.
    // Its binding still needs its own name, or it collides with arg 0.
    let binding = syn::Ident::new(&format!("arg{bind_index}"), name.span());
    bind_index += 1;
    if is_this(ty) {
      extracts.push(quote! {
        let #binding = match ::yuzuko_core::op::this_id(scope, args.this()) {
          Ok(id) => ::yuzuko_core::op::This(id),
          Err(error) => {
            ::yuzuko_core::op::OpException::throw(error, scope);
            return;
          }
        };
      });
    } else {
      let index = js_index;
      extracts.push(quote! {
        let #binding = match <#ty as ::yuzuko_core::op::FromV8>::from_v8(
          scope,
          args.get(#index),
        ) {
          Ok(value) => value,
          Err(error) => {
            ::yuzuko_core::op::OpException::throw(error, scope);
            return;
          }
        };
      });
      js_index += 1;
    }
    call_args.push(quote! { #binding });
  }

  let call = quote! { #inner_name(#(#call_args),*) };
  let body = match &function.sig.output {
    ReturnType::Type(_, ty) if is_result(ty) => quote! {
      match #call {
        Ok(value) => ::yuzuko_core::op::ToV8::to_v8(value, scope, &mut ret),
        Err(error) => ::yuzuko_core::op::OpException::throw(error, scope),
      }
    },
    ReturnType::Default => quote! {
      #call;
    },
    _ => quote! {
      let value = #call;
      ::yuzuko_core::op::ToV8::to_v8(value, scope, &mut ret);
    },
  };

  let expanded = quote! {
    #inner

    #(#attrs)*
    #vis fn #name(
      scope: &mut ::v8::PinScope,
      args: ::v8::FunctionCallbackArguments,
      mut ret: ::v8::ReturnValue,
    ) {
      #(#extracts)*
      #body
    }
  };
  expanded.into()
}

pub(crate) fn is_scope(ty: &Type) -> bool {
  let Type::Reference(reference) = ty else {
    return false;
  };
  type_name(&reference.elem).contains("PinScope")
}

fn is_this(ty: &Type) -> bool {
  type_name(ty) == "This"
}

pub(crate) fn is_result(ty: &Type) -> bool {
  let Type::Path(path) = ty else {
    return false;
  };
  path
    .path
    .segments
    .last()
    .is_some_and(|segment| segment.ident == "Result")
}

pub(crate) fn type_name(ty: &Type) -> String {
  match ty {
    Type::Path(path) => path
      .path
      .segments
      .last()
      .map(|segment| {
        let ident = segment.ident.to_string();
        match &segment.arguments {
          PathArguments::AngleBracketed(args) => {
            let inner = args
              .args
              .iter()
              .filter_map(|arg| match arg {
                GenericArgument::Type(ty) => Some(type_name(ty)),
                _ => None,
              })
              .collect::<Vec<_>>()
              .join(",");
            if inner.is_empty() {
              ident
            } else {
              format!("{ident}<{inner}>")
            }
          }
          _ => ident,
        }
      })
      .unwrap_or_default(),
    _ => String::new(),
  }
}
