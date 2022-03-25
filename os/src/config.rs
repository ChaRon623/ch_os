#[allow(unused)]

/// 动态内存分配中内核堆大小
pub const KERNEL_HEAP_SIZE: usize = 0x20_0000;
//可用内存大小设置为8Mb，与K210一致
pub const MEMORY_END: usize = 0x80800000;
/// 页内偏移的位宽12位
pub const PAGE_OFFSET       : usize = 12;
/// 每个页面的大小4Kb
pub const PAGE_SIZE         : usize = 1 << PAGE_OFFSET;
pub const PAGE_SIZE_BITS: usize = 0xc;
