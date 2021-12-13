# TODO

(A "scratch" file to keep track of things.)


- `check_crate` creates the annotations using `SpecCollector::collect`

gather_DefIds

```rust
// RJ: HEREHERE jog over `body` using visit_terminator to print out all TERMINATORS/Call DefId ?
// https://doc.rust-lang.org/beta/nightly-rustc/rustc_middle/mir/visit/trait.Visitor.html#method.visit_terminator
let used_DefIds = gather_DefIds(body);
println!("Used DefIds: {:?}", used_DefIds);



```

## Specs (Internal)

1. Refactor `gather_defIds` to whole file/crate using visitor as done in

  * `tcx.hir().visit_all_item_likes(&mut collector)`

2. Put centralized specs in hand-generated hash-map

3. Link hand-generated hash-map with `def-id` spec to get `die00.rs` working 

4. Dump `system::exit()` into the hash-map too?


## Specs (External)

To support external specs, e.g. for `system::exit()` we need

* A way to turn rust-types -> liquid-types (?)

*After* we support *internal* specs, we can sup
How to support specs for functions _without_ code e.g. library functions like 

```rust
  pub fn exit(code: i32) -> !
```

https://doc.rust-lang.org/std/process/fn.exit.html

