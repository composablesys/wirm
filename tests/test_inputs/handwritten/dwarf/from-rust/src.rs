// Source for `from-rust.wasm`. See `README.md` for the build recipe.
//
// Three exports + one `#[inline(always)]` helper. The inline attribute forces
// the helper to be inlined into both callers even at `-Copt-level=0`, so the
// resulting DWARF carries `DW_TAG_inlined_subroutine` entries — the real-world
// DIE shape the hand-written fixtures don't cover.

#![no_std]

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[inline(always)]
fn double(x: i32) -> i32 {
    x + x
}

#[no_mangle]
pub extern "C" fn add(a: i32, b: i32) -> i32 {
    double(a) + b
}

#[no_mangle]
pub extern "C" fn triple(x: i32) -> i32 {
    double(x) + x
}
