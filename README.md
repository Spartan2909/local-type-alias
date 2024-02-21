# Local type aliases

`local-type-alias` provides an attribute macro for creating scoped type aliases in an `impl` block.

## Examples

```rust
#[local_alias]
impl<T> MyType<T>
where
    alias!(X = i32):,
    X: for<'a> Add<&'a T>,
{
    // ...
}
```

```rust
#[local_alias]
impl<T> MyType<T>
where
    alias!(X = [T; 4]):,
    alias!(Y = *mut X):,
    alias!(Z = fn(X) -> Y):,
    Z: PartialEq<fn([u8; 4]) -> *mut [u8; 4]>,
{
    // ...
}
```
