#![no_std]
#![feature(linkage)]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]

extern crate alloc;
#[macro_use]
pub mod console;
mod lang_items;
mod syscall;

use buddy_system_allocator::LockedHeap;
use syscall::*;
const USER_HEAP_SIZE: usize = 16384;

static mut HEAP_SPACE: [u8; USER_HEAP_SIZE] = [0; USER_HEAP_SIZE];

#[global_allocator]
static HEAP: LockedHeap = LockedHeap::empty();

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}

//在应用中使能动态内存分配
#[no_mangle]
#[link_section = ".text.entry"]
pub extern "C" fn _start() -> ! {
    unsafe {
        HEAP.lock()
            .init(HEAP_SPACE.as_ptr() as usize, USER_HEAP_SIZE);
    }
    exit(main());
}

//等待任意一个子进程结束
pub fn wait(exit_code: &mut i32) -> isize {
    loop {
        match sys_waitpid(-1, exit_code as *mut _) {
            -2 => {
                yield_();
            }
            // -1 or a real pid
            exit_pid => return exit_pid,
        }
    }
}

//等待一个进程标识符的值为pid的子进程结束
pub fn waitpid(pid: usize, exit_code: &mut i32) -> isize {
    loop {
        //当 sys_waitpid 返回值为 -2 ，即要等待的子进程存在但它却尚未退出的时候，
        //我们调用 yield_ 主动交出 CPU 使用权，待下次 CPU 使用权被内核
        //交还给它的时候再次调用 sys_waitpid 查看要等待的子进程是否退出。
        match sys_waitpid(pid as isize, exit_code as *mut _) {
            -2 => {
                yield_();
            }
            // -1 or a real pid
            exit_pid => return exit_pid,
        }
    }
}

pub fn read(fd: usize, buf: &mut [u8]) -> isize {
    sys_read(fd, buf)
}

#[linkage = "weak"]
#[no_mangle]
fn main() -> i32 {
    panic!("Cannot find main!");
}

pub fn write(fd: usize, buf: &[u8]) -> isize {
    sys_write(fd, buf)
}
pub fn exit(exit_code: i32) -> ! {
    sys_exit(exit_code);
}
pub fn yield_() -> isize {
    sys_yield()
}
pub fn get_time() -> isize {
    sys_get_time()
}
pub fn getpid() -> isize {
    sys_getpid()
}
pub fn fork() -> isize {
    sys_fork()
}
pub fn exec(path: &str) -> isize {
    sys_exec(path)
}

pub fn sleep(period_ms: usize) {
    let start = sys_get_time();
    while sys_get_time() < start + period_ms as isize {
        sys_yield();
    }
}