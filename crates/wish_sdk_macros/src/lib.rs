//! `#[system]`: turns a function into a game system (use it through
//! `wish_sdk`, whose `export!` lists the systems).
//!
//! ```ignore
//! #[system(after = "move_player")]
//! fn collect(coins: Query<(Entity, &Coin)>, mut score: ResMut<Score>) { ... }
//! ```
//!
//! The function keeps its name and body. Next to it (in the type namespace,
//! so the names do not clash) the macro declares a marker type of the same
//! name that knows the system's name, ordering and parameters; `export!`
//! uses it. References in parameter types (`&Coin`, `&mut Transform`) are
//! markers saying "read" and "write": the macro gives them the `'static`
//! lifetime, and the query hands out references that borrow from itself.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::visit_mut::VisitMut;
use syn::{Error, FnArg, ItemFn, LitStr, parse_macro_input, parse_quote};

/// Marks a function as a game system. Options: `before = "name"` and
/// `after = "name"` (each may be given more than once).
#[proc_macro_attribute]
pub fn system(args: TokenStream, item: TokenStream) -> TokenStream {
    let mut before = Vec::new();
    let mut after = Vec::new();
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("before") {
            before.push(meta.value()?.parse::<LitStr>()?);
            Ok(())
        } else if meta.path.is_ident("after") {
            after.push(meta.value()?.parse::<LitStr>()?);
            Ok(())
        } else {
            Err(meta.error(
                "unknown option; #[system] takes `before = \"name\"` and `after = \"name\"`",
            ))
        }
    });
    parse_macro_input!(args with parser);
    let function = parse_macro_input!(item as ItemFn);
    match expand(function, &before, &after) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Gives every elided reference lifetime `'static`.
struct StaticRefs;

impl VisitMut for StaticRefs {
    fn visit_type_reference_mut(&mut self, reference: &mut syn::TypeReference) {
        if reference.lifetime.is_none() {
            reference.lifetime = Some(parse_quote!('static));
        }
        syn::visit_mut::visit_type_reference_mut(self, reference);
    }
}

fn expand(mut function: ItemFn, before: &[LitStr], after: &[LitStr]) -> syn::Result<TokenStream2> {
    let signature = &function.sig;
    if !signature.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &signature.generics,
            "systems cannot be generic: the engine calls one fixed function",
        ));
    }
    if let Some(asyncness) = &signature.asyncness {
        return Err(Error::new_spanned(
            asyncness,
            "systems cannot be async: a system runs to the end within its tick",
        ));
    }
    if !matches!(signature.output, syn::ReturnType::Default) {
        return Err(Error::new_spanned(
            &signature.output,
            "systems return nothing; to stop with an error, panic (the engine reports it with the line)",
        ));
    }
    let name = signature.ident.clone();
    let name_text = name.to_string();
    let mut types = Vec::new();
    for input in &mut function.sig.inputs {
        match input {
            FnArg::Receiver(receiver) => {
                return Err(Error::new_spanned(
                    receiver,
                    "systems are free functions, not methods",
                ));
            }
            FnArg::Typed(typed) => {
                StaticRefs.visit_type_mut(&mut typed.ty);
                types.push((*typed.ty).clone());
            }
        }
    }
    let args: Vec<_> = (0..types.len()).map(|i| format_ident!("__p{i}")).collect();
    let vis = &function.vis;
    Ok(quote! {
        #function

        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        #vis struct #name {}

        #[automatically_derived]
        impl ::wish_sdk::__private::SystemDef for #name {
            const NAME: &'static str = #name_text;

            fn describe(
                types: &mut ::wish_sdk::__private::TypeTable,
            ) -> ::wish_sdk::__private::Value {
                ::wish_sdk::__private::system_json(
                    #name_text,
                    &[#(#before),*],
                    &[#(#after),*],
                    ::std::vec![#( <#types as ::wish_sdk::SystemParam>::describe(types) ),*],
                )
            }

            fn run(frame: &mut ::wish_sdk::__private::FrameReader) {
                #( let #args = <#types as ::wish_sdk::SystemParam>::fetch(frame.next_section()); )*
                #name(#(#args),*);
            }
        }
    })
}
