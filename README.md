# Local type aliases

`local-type-alias` provides an attribute macro for creating scoped type and trait aliases in an item.

## Examples

```rust
#[local_alias]
#[alias(type X = i32)]
struct MyType<T>
where
    X: for<'a> Add<&'a T>,
{
    value: T,
}
```

```rust
#[local_alias]
#[alias(
    type X = [T; 4],
    type Y = *mut X,
    type Z = fn(X) -> Y,
    trait A = PartialEq<fn([u8; 4]) -> *mut [u8; 4]>,
)]
impl<T> MyType<T>
where
    Z: A,
{
    // ...
}
```

```rust
#[local_alias(macros)]
#[alias(type TotallyNotAString = String)]
struct MyType {
   // This expands to `value: my_macro!(String),`
   value: my_macro!({{TotallyNotAString}}),
}
```
