#![feature(register_tool)]
#![register_tool(lr)]

#[lr::ty(fn<n: int>(l: i32@n; ref<l>) -> i32; l: i32 @ n)]
pub fn inc(x: &mut i32) -> i32 {
    *x += 1;
    0
}
