//! Attribute macros that declare plain-data types for Wish Engine.
//!
//! Use these through `wish_schema`, which re-exports them; the generated code
//! refers to that crate as `::wish_schema`.
//!
//! | Macro | Use it on |
//! |---|---|
//! | `#[component]` | data attached to entities |
//! | `#[resource]` | a single value stored once per world |
//! | `#[event]` | a message sent between systems |
//! | `#[schema]` | a struct or enum used as a field of the above |
//!
//! Each macro accepts a struct with named fields (or no fields) or a
//! fieldless enum, and:
//!
//! - sets the representation (`repr(C)` for structs, `repr(u8)` for enums);
//! - implements `Clone`, `Copy`, `Debug`, `PartialEq` and `Default` (and `Eq`
//!   for enums);
//! - implements `wish_schema::Plain`, taking sizes and offsets from the
//!   compiler, documentation from `///` comments, and the rest from
//!   `#[schema(...)]` options on fields.
//!
//! Field options:
//!
//! - `unit = "m/s"`: the unit the value is measured in;
//! - `range = 0.0..=50.0`: the inclusive range a numeric field accepts;
//! - `default = 5.0`: the starting value, as any Rust expression of the
//!   field's type; without it the field type's own default is used;
//! - `alias = "position"`: a name people are likely to try instead, used to
//!   make the "did you mean" hint exact.
//!
//! The default of an enum is its first variant.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{
    Attribute, Data, DataEnum, DataStruct, DeriveInput, Error, Expr, ExprLit, ExprRange, Fields,
    Ident, Lit, LitStr, Meta, RangeLimits, parse_macro_input, parse_quote,
};

/// Which macro is expanding.
#[derive(Clone, Copy)]
enum Role {
    Component,
    Resource,
    Event,
    Schema,
}

impl Role {
    /// The attribute's name, for error messages.
    fn attribute(self) -> &'static str {
        match self {
            Self::Component => "component",
            Self::Resource => "resource",
            Self::Event => "event",
            Self::Schema => "schema",
        }
    }

    /// The marker trait the type also implements, if any.
    fn marker(self) -> Option<TokenStream2> {
        match self {
            Self::Component => Some(quote!(::wish_schema::Component)),
            Self::Resource => Some(quote!(::wish_schema::Resource)),
            Self::Event => Some(quote!(::wish_schema::Event)),
            Self::Schema => None,
        }
    }
}

/// Declares a component: plain data attached to entities.
///
/// See the crate documentation for what the macro generates and for the
/// `#[schema(...)]` field options.
#[proc_macro_attribute]
pub fn component(args: TokenStream, item: TokenStream) -> TokenStream {
    expand(args, item, Role::Component)
}

/// Declares a resource: a single plain-data value stored once per world.
///
/// See the crate documentation for what the macro generates and for the
/// `#[schema(...)]` field options.
#[proc_macro_attribute]
pub fn resource(args: TokenStream, item: TokenStream) -> TokenStream {
    expand(args, item, Role::Resource)
}

/// Declares an event: a plain-data message sent between systems.
///
/// See the crate documentation for what the macro generates and for the
/// `#[schema(...)]` field options.
#[proc_macro_attribute]
pub fn event(args: TokenStream, item: TokenStream) -> TokenStream {
    expand(args, item, Role::Event)
}

/// Declares a plain-data struct or enum for use as a field of a component,
/// resource or event.
///
/// See the crate documentation for what the macro generates and for the
/// `#[schema(...)]` field options.
#[proc_macro_attribute]
pub fn schema(args: TokenStream, item: TokenStream) -> TokenStream {
    expand(args, item, Role::Schema)
}

fn expand(args: TokenStream, item: TokenStream, role: Role) -> TokenStream {
    let args = TokenStream2::from(args);
    let input = parse_macro_input!(item as DeriveInput);
    match expand_input(&args, input, role) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_input(
    args: &TokenStream2,
    mut input: DeriveInput,
    role: Role,
) -> syn::Result<TokenStream2> {
    let attribute = role.attribute();
    if !args.is_empty() {
        return Err(Error::new_spanned(
            args,
            format!("#[{attribute}] takes no arguments"),
        ));
    }
    if !input.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &input.generics,
            format!(
                "#[{attribute}] types cannot be generic: the engine has to describe one fixed layout"
            ),
        ));
    }

    let ident = input.ident.clone();
    let name = ident.to_string();
    let doc = doc_of(&input.attrs);
    let is_enum = matches!(input.data, Data::Enum(_));
    check_attributes(&input.attrs, attribute, is_enum)?;

    // Every field type must be plain data. Stating it once per impl, spanned
    // at the field's type, makes a non-plain field fail there and only there:
    // inside the impls the bound is assumed, so the generated bodies do not
    // repeat the error (identical errors at one span are reported once).
    let bounds: Vec<TokenStream2> = match &input.data {
        Data::Struct(data) => data
            .fields
            .iter()
            .map(|field| {
                let ty = &field.ty;
                quote_spanned!(ty.span()=> #ty: ::wish_schema::Plain)
            })
            .collect(),
        _ => Vec::new(),
    };
    let where_clause = if bounds.is_empty() {
        TokenStream2::new()
    } else {
        quote!(where #(#bounds),*)
    };

    let plain_impl = match &mut input.data {
        Data::Struct(data) => struct_impl(&name, &doc, data, attribute)?,
        Data::Enum(data) => enum_impl(&ident, &doc, data, attribute)?,
        Data::Union(data) => {
            return Err(Error::new_spanned(
                data.union_token,
                format!(
                    "#[{attribute}] cannot be used on a union; use a struct or a fieldless enum"
                ),
            ));
        }
    };

    if is_enum {
        input
            .attrs
            .push(parse_quote!(#[derive(Clone, Copy, Debug, PartialEq, Eq)]));
        input.attrs.push(parse_quote!(#[repr(u8)]));
    } else {
        input
            .attrs
            .push(parse_quote!(#[derive(Clone, Copy, Debug, PartialEq)]));
        input.attrs.push(parse_quote!(#[repr(C)]));
    }

    let marker = role.marker().map(|marker| {
        quote! {
            #[automatically_derived]
            impl #marker for #ident #where_clause {
                const NAME: &'static str = #name;
            }
        }
    });

    Ok(quote! {
        #input

        #[automatically_derived]
        #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
        unsafe impl ::wish_schema::Plain for #ident #where_clause {
            #plain_impl
        }

        #[automatically_derived]
        impl ::core::default::Default for #ident #where_clause {
            fn default() -> Self {
                <Self as ::wish_schema::Plain>::default_value()
            }
        }

        #marker

        const _: () = ::core::assert!(
            ::core::mem::align_of::<#ident>() <= 8,
            "types stored in a world can be aligned to at most 8 bytes",
        );
    })
}

/// Rejects attributes that would clash with what the macro adds.
fn check_attributes(attrs: &[Attribute], attribute: &str, is_enum: bool) -> syn::Result<()> {
    let provided: &[&str] = if is_enum {
        &["Clone", "Copy", "Debug", "PartialEq", "Eq", "Default"]
    } else {
        &["Clone", "Copy", "Debug", "PartialEq", "Default"]
    };
    for attr in attrs {
        if attr.path().is_ident("repr") {
            return Err(Error::new_spanned(
                attr,
                format!("#[{attribute}] sets the representation itself; remove this #[repr(...)]"),
            ));
        }
        if attr.path().is_ident("derive") {
            attr.parse_nested_meta(|meta| {
                match provided.iter().find(|name| meta.path.is_ident(name)) {
                    Some(name) => Err(meta.error(format!(
                        "#[{attribute}] already implements {}; remove `{name}` from the derive list",
                        provided.join(", ")
                    ))),
                    None => Ok(()),
                }
            })?;
        }
    }
    Ok(())
}

/// The text of an item's `///` comments, joined into one string.
fn doc_of(attrs: &[Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let Meta::NameValue(name_value) = &attr.meta
            && let Expr::Lit(ExprLit {
                lit: Lit::Str(text),
                ..
            }) = &name_value.value
        {
            let line = text.value();
            lines.push(
                line.strip_prefix(' ')
                    .unwrap_or(&line)
                    .trim_end()
                    .to_owned(),
            );
        }
    }
    lines.join("\n").trim().to_owned()
}

/// The `#[schema(...)]` options of one field.
#[derive(Default)]
struct FieldOptions {
    unit: Option<LitStr>,
    range: Option<(Expr, Expr)>,
    default: Option<Expr>,
    aliases: Vec<LitStr>,
}

fn field_options(attrs: &[Attribute]) -> syn::Result<FieldOptions> {
    let mut options = FieldOptions::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("schema")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("unit") {
                options.unit = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("alias") {
                options.aliases.push(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("default") {
                options.default = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("range") {
                let range: Expr = meta.value()?.parse()?;
                match range {
                    Expr::Range(ExprRange {
                        start: Some(start),
                        end: Some(end),
                        limits: RangeLimits::Closed(_),
                        ..
                    }) => {
                        options.range = Some((*start, *end));
                        Ok(())
                    }
                    other => Err(Error::new_spanned(
                        other,
                        "a range names both ends and includes them, for example `range = 0.0..=50.0`",
                    )),
                }
            } else {
                Err(meta.error(
                    "unknown option; the options are `unit = \"...\"`, `range = a..=b`, `default = value` and `alias = \"...\"`",
                ))
            }
        })?;
    }
    Ok(options)
}

/// The body of `impl Plain` for a struct.
fn struct_impl(
    name: &str,
    doc: &str,
    data: &mut DataStruct,
    attribute: &str,
) -> syn::Result<TokenStream2> {
    let fields = match &mut data.fields {
        Fields::Named(named) => Some(&mut named.named),
        Fields::Unit => None,
        Fields::Unnamed(unnamed) => {
            return Err(Error::new_spanned(
                unnamed,
                format!(
                    "#[{attribute}] needs named fields: write `struct Name {{ field: Type }}` so each value has a name in JSON"
                ),
            ));
        }
    };

    let mut schemas = Vec::new();
    let mut defaults = Vec::new();
    let mut encodes = Vec::new();
    let mut decodes = Vec::new();

    for field in fields.into_iter().flatten() {
        let options = field_options(&field.attrs)?;
        field.attrs.retain(|attr| !attr.path().is_ident("schema"));
        let Some(field_ident) = field.ident.clone() else {
            continue;
        };
        let field_name = field_ident.to_string();
        let field_doc = doc_of(&field.attrs);
        let ty = &field.ty;

        // Spanned at the field's type so that "not plain data" errors point there.
        let field_type_schema = quote_spanned!(ty.span()=> <#ty as ::wish_schema::Plain>::schema());
        let mut schema = quote! {
            ::wish_schema::FieldSchema::new(
                #field_name,
                #field_doc,
                ::core::mem::offset_of!(Self, #field_ident),
                #field_type_schema,
            )
            .with_default_of(&defaults.#field_ident)
        };
        if let Some(unit) = &options.unit {
            schema = quote!(#schema.with_unit(#unit));
        }
        if let Some((min, max)) = &options.range {
            schema = quote!(#schema.with_range((#min) as f64, (#max) as f64));
        }
        for alias in &options.aliases {
            schema = quote!(#schema.with_alias(#alias));
        }
        schemas.push(schema);

        defaults.push(match &options.default {
            Some(default) => quote_spanned!(default.span()=> #field_ident: #default),
            None => quote_spanned!(ty.span()=> #field_ident: <#ty as ::wish_schema::Plain>::default_value()),
        });
        encodes.push(quote! {
            ::wish_schema::__private::encode_field(out, ::core::mem::offset_of!(Self, #field_ident), &self.#field_ident);
        });
        decodes.push(quote! {
            #field_ident: ::wish_schema::__private::decode_field(bytes, ::core::mem::offset_of!(Self, #field_ident))?
        });
    }

    Ok(quote! {
        #[allow(unused_variables, clippy::unnecessary_cast)]
        fn schema() -> ::wish_schema::TypeSchema {
            let defaults = <Self as ::wish_schema::Plain>::default_value();
            ::wish_schema::TypeSchema::structure(
                #name,
                #doc,
                ::core::mem::size_of::<Self>(),
                ::core::mem::align_of::<Self>(),
                ::std::vec![#(#schemas),*],
            )
        }

        fn default_value() -> Self {
            Self { #(#defaults),* }
        }

        fn encode(&self, out: &mut [u8]) {
            ::wish_schema::__private::encode_begin(out);
            #(#encodes)*
        }

        fn decode(bytes: &[u8]) -> ::core::option::Option<Self> {
            if bytes.len() != ::core::mem::size_of::<Self>() {
                return ::core::option::Option::None;
            }
            ::core::option::Option::Some(Self { #(#decodes),* })
        }
    })
}

/// The body of `impl Plain` for a fieldless enum.
fn enum_impl(
    ident: &Ident,
    doc: &str,
    data: &DataEnum,
    attribute: &str,
) -> syn::Result<TokenStream2> {
    let name = ident.to_string();
    let Some(first) = data.variants.first() else {
        // An empty variant list has no position of its own: point at the name.
        return Err(Error::new_spanned(
            ident,
            format!("#[{attribute}] enums need at least one variant; the first one is the default"),
        ));
    };
    let first_ident = &first.ident;

    let mut schemas = Vec::new();
    let mut matches = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(Error::new_spanned(
                variant,
                format!(
                    "#[{attribute}] enum variants cannot hold data; put the data in separate fields next to the enum"
                ),
            ));
        }
        let variant_ident: &Ident = &variant.ident;
        let variant_name = variant_ident.to_string();
        let variant_doc = doc_of(&variant.attrs);
        schemas.push(quote! {
            ::wish_schema::VariantSchema::new(#variant_name, #variant_doc, Self::#variant_ident as u8)
        });
        matches.push(quote! {
            if byte == Self::#variant_ident as u8 {
                return ::core::option::Option::Some(Self::#variant_ident);
            }
        });
    }

    Ok(quote! {
        fn schema() -> ::wish_schema::TypeSchema {
            ::wish_schema::TypeSchema::enumeration(#name, #doc, ::std::vec![#(#schemas),*])
        }

        fn default_value() -> Self {
            Self::#first_ident
        }

        fn encode(&self, out: &mut [u8]) {
            if let [slot] = out {
                *slot = *self as u8;
            }
        }

        fn decode(bytes: &[u8]) -> ::core::option::Option<Self> {
            let &[byte] = bytes else {
                return ::core::option::Option::None;
            };
            #(#matches)*
            ::core::option::Option::None
        }
    })
}
