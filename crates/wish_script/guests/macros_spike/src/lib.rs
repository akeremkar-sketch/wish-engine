//! A stand-in for `wish_sdk_macros`: `#[system]` keeps the function and adds
//! a marker, which is enough to make the compiler load a procedural macro.

use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn system(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut out = item;
    let marker: TokenStream = "#[used] static WISH_SYSTEM_MARK: u8 = 1;"
        .parse()
        .unwrap_or_default();
    out.extend(marker);
    out
}
