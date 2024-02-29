//! `local-type-aliases` allows for the creation of scoped type aliases in an
//!`impl` block.
//!
//! ## Examples
//! ```rust
//! # use local_type_alias::local_alias;
//! #
//! # use std::ops::Add;
//! #
//! # struct MyType<T>(T);
//! #
//! #[local_alias]
//! impl<T> MyType<T>
//! where
//!     alias!(X = i32):,
//!     X: for<'a> Add<&'a T>,
//! {
//!     // ...
//! }
//! ```
//!
//! ```rust
//! # use local_type_alias::local_alias;
//! #
//! # struct MyType<T>(T);
//! #
//! #[local_alias]
//! impl<T> MyType<T>
//! where
//!     alias!(X = [u8; 4]):,
//!     alias!(Y = *mut X):,
//!     alias!(Z = fn(X) -> Y):,
//!     Z: PartialEq<fn([u8; 4]) -> *mut [u8; 4]>,
//! {
//!     // ...
//! }
//! ```

mod substitute;

use quote::ToTokens as _;

use syn::parse_macro_input;
use syn::visit_mut::VisitMut as _;
use syn::Item;

/// Local type aliases.
///
/// See the [crate documentation][crate] for details.
#[proc_macro_attribute]
pub fn local_alias(
    args: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut input = parse_macro_input!(item as Item);

    let mut in_macros = false;
    let options_parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("macros") {
            in_macros = true;
            Ok(())
        } else {
            Err(meta.error("unsupported local alias option"))
        }
    });

    parse_macro_input!(args with options_parser);

    let mut visitor = match substitute::Visitor::new(in_macros, &mut input) {
        Ok(visitor) => visitor,
        Err(error) => return error.into_compile_error().into(),
    };

    visitor.visit_item_mut(&mut input);

    input.into_token_stream().into()
}
