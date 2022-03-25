mod heap_allocator;
mod frame_allocator;
mod address;
mod page_table;

pub use address::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum};
pub use frame_allocator::{frame_alloc, frame_dealloc, FrameTracker};
use page_table::PTEFlags;

pub fn init() {
	    heap_allocator::init_heap();
		//测试堆分配是否成功
		heap_allocator::heap_test();
		frame_allocator::init_frame_allocator();
		//测试物理页帧分配是否成功
		frame_allocator::frame_allocator_test();
}
