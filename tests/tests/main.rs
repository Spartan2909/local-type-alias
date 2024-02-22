use local_type_alias::local_alias;

use std::ops::Add;

#[allow(dead_code)]
struct Test;

#[local_alias]
impl Test
where
    alias!(X = i32):,
    X: for<'a> Add<&'a i32>,
{
}

#[local_alias]
impl Test
where
    alias!(X = [u8; 4]):,
    alias!(Y = *mut X):,
    alias!(Z = fn(X) -> Y):,
    Z: PartialEq<fn([u8; 4]) -> *mut [u8; 4]>,
{
}
