pub const CLOCK_FREQ: usize = 12500000;
pub const MEMORY_END: usize = 0x80800000;

pub const MMIO: &[(usize, usize)] = &[(0x10001000, 0x1000)];

pub type BlockDeviceImpl = crate::drivers::block::VirtIOBlock;

