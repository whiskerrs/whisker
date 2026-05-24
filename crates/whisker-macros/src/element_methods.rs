//! `#[whisker::element_methods]` — generates an `impl Trait for
//! ElementRef<T>` block that turns each declared trait method into
//! a call to [`ElementRef::invoke`]. Phase 7-Φ.H.2.3.
//!
//! ## Shape contract
//!
//! Input is a trait declaration whose methods follow the
//! WhiskerValue-only `-sys` shape (same as `#[whisker::platform_module]`):
//!
//! ```ignore
//! #[whisker::element_methods(Video)]
//! pub trait VideoExt {
//!     fn play(&self, args: Vec<WhiskerValue>) -> WhiskerValue;
//!     fn seek(&self, args: Vec<WhiskerValue>) -> WhiskerValue;
//! }
//! ```
//!
//! Output: the trait passed through unchanged, plus an `impl` block
//! that wires it onto `whisker::ElementRef<Video>`:
//!
//! ```ignore
//! impl VideoExt for ::whisker::ElementRef<Video> {
//!     fn play(&self, args: Vec<WhiskerValue>) -> WhiskerValue {
//!         self.invoke("play", args)
//!     }
//!     fn seek(&self, args: Vec<WhiskerValue>) -> WhiskerValue {
//!         self.invoke("seek", args)
//!     }
//! }
//! ```
//!
//! With the trait in scope (`use whisker_video::*`), authors write
//! `video.play(vec![])` / `video.seek(vec![WhiskerValue::Float(30.0)])`
//! on a `let video: ElementRef<Video> = element_ref();`.
//!
//! ## Why an extension trait, not an inherent impl
//!
//! `ElementRef<T>` is defined in `whisker-driver` (not the module
//! author's crate), so Rust's orphan rules forbid inherent impls
//! on `ElementRef<Video>` from the user crate. The
//! extension-trait pattern (`trait VideoExt + impl VideoExt for
//! ElementRef<Video>`) is the standard workaround.
//!
//! ## Typed wrappers
//!
//! The `Vec<WhiskerValue>` shape is intentionally raw — module
//! authors hand-write typed wrappers on top:
//!
//! ```ignore
//! pub trait VideoControls {
//!     fn play(&self);
//!     fn seek(&self, position: f64);
//! }
//! impl VideoControls for ElementRef<Video> {
//!     fn play(&self) { let _ = VideoExt::play(self, vec![]); }
//!     fn seek(&self, position: f64) {
//!         let _ = VideoExt::seek(self, vec![WhiskerValue::Float(position)]);
//!     }
//! }
//! ```
//!
//! Same discipline as `#[whisker::platform_module]` — the proc macro
//! is the predictable `-sys` layer; ergonomics live in the
//! hand-written wrapper.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{parse2, FnArg, ItemTrait, ReturnType, TraitItem, TraitItemFn, Type};

pub fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    // Attribute parses as a single ident — the marker type the
    // element binds to. `#[whisker::element_methods(Video)]`.
    let target_type: Type = match parse2(attr.clone()) {
        Ok(t) => t,
        Err(e) => {
            return quote_spanned! { e.span() =>
                compile_error!(concat!(
                    "`#[whisker::element_methods(...)]` expects a type \
                     argument (the marker type for the native element), e.g.\n",
                    "    #[whisker::element_methods(Video)]\n",
                    "    pub trait VideoExt { ... }\n",
                ));
            };
        }
    };

    let input: ItemTrait = match parse2(item.clone()) {
        Ok(t) => t,
        Err(e) => {
            return quote_spanned! { e.span() =>
                compile_error!(concat!(
                    "`#[whisker::element_methods]` expects a trait declaration, e.g.\n",
                    "    #[whisker::element_methods(Video)]\n",
                    "    pub trait VideoExt {\n",
                    "        fn play(&self, args: Vec<WhiskerValue>) -> WhiskerValue;\n",
                    "    }\n",
                ));
            };
        }
    };

    let trait_name = &input.ident;
    let mut impl_methods = Vec::new();

    for it in &input.items {
        let TraitItem::Fn(TraitItemFn { sig, .. }) = it else {
            continue;
        };
        let method_ident = &sig.ident;
        let method_name_str = method_ident.to_string();

        // Inputs must start with `&self`. Reject other shapes with
        // a clear diagnostic — the macro can't generate a sensible
        // `ElementRef`-method call without it.
        let first_arg = sig.inputs.first();
        let starts_with_self =
            matches!(first_arg, Some(FnArg::Receiver(r)) if r.reference.is_some());
        if !starts_with_self {
            return quote_spanned! { sig.span() =>
                compile_error!(concat!(
                    "`#[whisker::element_methods]` trait methods must \
                     start with `&self` (the ElementRef the call dispatches against)"
                ));
            };
        }

        // The remaining args are passed through to `invoke` as the
        // second positional. We collect each arg's pattern (likely
        // `args: Vec<WhiskerValue>` but we don't enforce — keep
        // the door open for typed convenience methods authors might
        // declare and forward themselves).
        let forward_args = sig.inputs.iter().skip(1).map(|a| match a {
            FnArg::Typed(pt) => {
                let pat = &pt.pat;
                quote! { #pat }
            }
            FnArg::Receiver(_) => quote! {},
        });
        let forward_args2 = forward_args.clone();

        let return_ty = match &sig.output {
            ReturnType::Default => quote! { () },
            ReturnType::Type(_, t) => quote! { #t },
        };

        // The shape we emit assumes one extra positional arg of
        // type `Vec<WhiskerValue>`. For the "no args at all"
        // overload it'd just pass `vec![]`. We keep it strict: the
        // user MUST declare exactly `(&self, args: Vec<WhiskerValue>)`
        // for now. Multi-arg variants can land later if a real use
        // case demands them.
        let extra_args: Vec<_> = sig.inputs.iter().skip(1).collect();
        if extra_args.len() != 1 {
            return quote_spanned! { sig.span() =>
                compile_error!(concat!(
                    "`#[whisker::element_methods]` trait methods must take \
                     exactly one extra arg (`args: Vec<WhiskerValue>`) — \
                     same shape as `#[whisker::platform_module]`. Typed wrappers \
                     belong in a hand-written extension trait layered on top."
                ));
            };
        }

        // Reconstruct the impl method body. `self.invoke(...)`
        // returns `WhiskerValue`; if the user declared a different
        // return type we still emit the call and let the compiler
        // type-check it against `return_ty`.
        let _ = forward_args2;
        impl_methods.push(quote! {
            fn #method_ident(&self, #(#extra_args),*) -> #return_ty {
                self.invoke(#method_name_str, #(#forward_args),*)
            }
        });
    }

    // The full crate path `::whisker::ElementRef<#target_type>`
    // requires the consumer to depend on `whisker` directly —
    // module crates already do (via `use whisker::prelude::*;`).
    quote! {
        #input

        impl #trait_name for ::whisker::ElementRef<#target_type> {
            #(#impl_methods)*
        }
    }
}
