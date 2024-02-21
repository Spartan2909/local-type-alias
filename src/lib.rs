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

mod ast;
use ast::AugmentedImpl;

mod substitute;

use quote::ToTokens as _;

use syn::parse_macro_input;

/// Local type aliases.
///
/// See the [crate documentation][crate] for details.
#[proc_macro_attribute]
pub fn local_alias(
    _attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let input = parse_macro_input!(item as AugmentedImpl);
    let output = input.substitute();
    output.into_token_stream().into()
}
