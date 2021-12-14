#![feature(register_tool)]
#![register_tool(lr)]
#![feature(custom_inner_attributes)]

// A 'global' attribute 

#![lr::ty(fn my_die(i32{v: false}) -> i32)]


// A 'local' attribute

#[lr::ty(fn(i32{v: false}) -> i32)]
fn my_exit(_x: i32) -> i32 {
    0
}

fn my_die(_x: i32) -> i32 {
    0
}

#[lr::ty(fn(i32{v: 0 <= v}) -> i32)]
pub fn only_nat1(x: i32) -> i32 {
    if x < 0 {
        my_exit(0)
    } else { 
        0
    }
}

#[lr::ty(fn(i32{v: 0 <= v}) -> i32)]
pub fn only_nat2(x: i32) -> i32 {
    if x < 0 {
        my_die(0)
    } else { 
        0
    }
}

