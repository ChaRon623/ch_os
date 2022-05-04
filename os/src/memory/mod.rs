mod heap_allocator;
mod frame_allocator;
mod address;
mod page_table;
mod memory_set;
use crate::println;

pub use address::{PhysAddr, StepByOne, PhysPageNum, VirtAddr, VirtPageNum};
use address::{ VPNRange};

pub use page_table::{
	translated_byte_buffer,
	translated_refmut, 
	translated_str, 
	PageTableEntry,
	UserBuffer, 
	UserBufferIterator,
	PageTable,
};

pub use frame_allocator::{
	frame_alloc,
	FrameTracker,
	frame_dealloc,
};

use page_table::{
	PTEFlags,
};
pub use memory_set::{kernel_token, MapPermission, MemorySet, KERNEL_SPACE};
pub use memory_set::remap_test;

pub fn init() {
	    heap_allocator::init_heap();
		//测试堆分配是否成功
		heap_allocator::heap_test();
		frame_allocator::init_frame_allocator();
		//测试物理页帧分配是否成功
		frame_allocator::frame_allocator_test();
		//创建内核地址空间并让CPU开启分页模式，MMU在地址转换的时候使用内核的多级页表
		KERNEL_SPACE.exclusive_access().activate();
		//检查内核地址空间的多级页表是否被正确设置
		memory_set::remap_test();
		println!("Memory managment initialized.");
}
