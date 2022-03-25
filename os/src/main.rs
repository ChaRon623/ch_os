#![no_std]
#![no_main]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]

use core::arch::global_asm;

//use crate::{config::TRAMPOLINE, process::default_handlers::{def_dump_core, def_ignore, def_terminate_self}};

global_asm!(include_str!("entry.asm"));

//引入alloc库的依赖
#[macro_use]
extern crate alloc;
extern crate bitflags;//Rust中常用比特标志位的crate 

#[macro_use]
//mod sync;
mod console;
mod lang_items;
mod sbi;
mod memory;
mod config;

fn clear_bss() {
    extern "C" {
        fn sbss();
        fn ebss();
    }
    unsafe {
        core::slice::from_raw_parts_mut(sbss as usize as *mut u8, ebss as usize - sbss as usize)
            .fill(0);
    }
}

//use lazy_static::*;
//use sync::UPIntrFreeCell;

//lazy_static! {
//    pub static ref DEV_NON_BLOCKING_ACCESS: UPIntrFreeCell<bool> = unsafe { UPIntrFreeCell::new(false) };
//}


#[no_mangle]
pub fn rust_main() -> ! {
    clear_bss();
    println!("Hello, world!");
    
    memory::init();
    panic!("Shutdown machine!");
}
