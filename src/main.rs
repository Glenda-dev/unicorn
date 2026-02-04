#![no_std]
#![no_main]
#![allow(dead_code)]

extern crate alloc;

mod device;
mod dma;
mod layout;

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => ({
        glenda::println!("Unicorn: {}", format_args!($($arg)*));
    })
}

#[unsafe(no_mangle)]
fn main() -> usize {
    log!("Starting Unicorn Device Driver Manager...");
    1
}
