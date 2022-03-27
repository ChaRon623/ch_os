#[allow(unused)]

pub const USER_STACK_SIZE: usize = 4096 * 2;
/// 动态内存分配中内核堆大小
pub const KERNEL_HEAP_SIZE: usize = 0x20_0000;
//可用内存大小设置为8Mb，与K210一致
pub const MEMORY_END: usize = 0x80800000;
/// 页内偏移的位宽12位
pub const PAGE_OFFSET       : usize = 12;
/// 每个页面的大小4Kb
pub const PAGE_SIZE         : usize = 1 << PAGE_OFFSET;
pub const PAGE_SIZE_BITS: usize = 0xc;
//pub use crate::board::{CLOCK_FREQ, MMIO};
pub const TRAMPOLINE: usize = usize::MAX - PAGE_SIZE + 1;

/// Device memory mapped IO for qemu
pub const MMIO: &[(usize, usize)] = &[
	(0x1000_0000, 0x1000),
	(0x1000_1000, 0x1000),
	(0xC00_0000, 0x40_0000),
];
