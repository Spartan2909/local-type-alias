mod ast;
use ast::AugmentedImpl;

mod substitute;

use quote::ToTokens as _;

use syn::parse_macro_input;

#[proc_macro_attribute]
pub fn local_alias(
    _attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let input = parse_macro_input!(item as AugmentedImpl);
    let output = input.substitute();
    output.into_token_stream().into()
}
