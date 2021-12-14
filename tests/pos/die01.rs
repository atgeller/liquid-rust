#![feature(register_tool)]
#![register_tool(lr)]

use std::process::exit;

#[lr::ty(fn(i32{v: 0 <= v}) -> i32)]
pub fn only_nat(x: i32) -> i32 {
    if x < 0 {
        exit(0)
    } else { 
        0
    }
}
