pub const USER_STACK_SIZE: usize = 4096 * 2;
/// 动态内存分配中内核堆大小
pub const KERNEL_STACK_SIZE: usize = 4096 * 2;
pub const KERNEL_HEAP_SIZE: usize = 0x20_0000;

pub const PAGE_SIZE: usize = 0x1000;
pub const PAGE_SIZE_BITS: usize = 0xc;

pub const TRAMPOLINE: usize = usize::MAX - PAGE_SIZE + 1;
/// Clock frequency
//可用内存大小设置为8Mb，与K210一致
pub use crate::board::{CLOCK_FREQ, MEMORY_END};
pub const TRAP_CONTEXT: usize = TRAMPOLINE - PAGE_SIZE;
