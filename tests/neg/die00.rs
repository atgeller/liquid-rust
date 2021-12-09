#![feature(register_tool)]
#![register_tool(lr)]

#[lr::ty(fn(i32{v: false}) -> i32)]
fn my_exit(_x: i32) -> i32 {
    0
}

#[lr::ty(fn(i32{v: 0 <= v}) -> i32)]
pub fn only_nat(x: i32) -> i32 {
    if x <= 0 {
        my_exit(0)
    } else { 
        0
    }
}
