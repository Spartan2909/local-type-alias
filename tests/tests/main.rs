use local_type_alias::local_alias;

use std::ops::Add;

macro_rules! identity {
    ($($tt:tt)*) => {
        $($tt)*
    };
}

#[allow(dead_code)]
struct Test1;

#[local_alias(macros)]
#[alias(type X = i32)]
impl Test1
where
    identity!({
        {
            X
        }
    }): for<'a> Add<&'a i32>,
    for<'a> <X as Add<&'a X>>::Output: Eq,
{
}

#[local_alias(macros)]
#[alias(
    type X = [u8; 4],
    type Y = *mut X,
    type Z = fn(X) -> Y,
    trait A = PartialEq<fn([u8; 4]) -> *mut X>,
)]
#[allow(dead_code)]
struct Test2
where
    Z: A;
