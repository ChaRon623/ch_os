use alloc::vec::Vec;
use lazy_static::*;
use crate::memory::{MapPermission, VirtAddr,KERNEL_SPACE};
use crate::config::{KERNEL_STACK_SIZE, PAGE_SIZE, TRAMPOLINE};
use crate::sync::UPSafeCell;

pub struct PidHandle(pub usize);

pub struct PidAllocator {
    current: usize,
    recycled: Vec<usize>,
}

impl PidAllocator {
    pub fn new() -> Self {
        PidAllocator {
            current: 0,
            recycled: Vec::new(),
        }
    }
    pub fn alloc(&mut self) -> PidHandle {
        if let Some(pid) = self.recycled.pop() {
            PidHandle(pid)
        } else {
            self.current += 1;
            PidHandle(self.current - 1)
        }
    }
    pub fn dealloc(&mut self, pid: usize) {
        assert!(pid < self.current);
        assert!(
            self.recycled.iter().find(|ppid| **ppid == pid).is_none(),
            "pid {} has been deallocated!", pid
        );
        self.recycled.push(pid);
    }
}

lazy_static! {
    static ref PID_ALLOCATOR : UPSafeCell<PidAllocator> = unsafe {
        UPSafeCell::new(PidAllocator::new())
    };
}

//分配出去一个将usize包装之后的PidHandle
pub fn pid_alloc() -> PidHandle {
    PID_ALLOCATOR.exclusive_access().alloc()
}

//允许编译器进行自动的资源回收
impl Drop for PidHandle {
    fn drop(&mut self) {
        PID_ALLOCATOR.exclusive_access().dealloc(self.0);
    }
}

/// Return (bottom, top) of a kernel stack in kernel space.
pub fn kernel_stack_position(app_id: usize) -> (usize, usize) {
    let top = TRAMPOLINE - app_id * (KERNEL_STACK_SIZE + PAGE_SIZE);
    let bottom = top - KERNEL_STACK_SIZE;
    (bottom, top)
}

//之前将每个应用的内核栈按照应用编号从小到大的顺序将它们作为逻辑段从高地址到
//低地址放在内核地址空间中，且两两之间保留一个守护页面使得我们能够尽可能
//早的发现内核栈溢出问题。从本章开始，我们将应用编号替换为进程标识符。
pub struct KernelStack {
    pid: usize,
}

impl KernelStack {
    //从一个PidHandle，也就是一个已分配的进程标识符中对应生成一个内核栈KernelStack
	    pub fn new(pid_handle: &PidHandle) -> Self {
			let pid = pid_handle.0;
            //根据进程标识符计算内核栈在内核地址空间中的位置
			let (kernel_stack_bottom, kernel_stack_top) = kernel_stack_position(pid);
			//将一个逻辑段插入内核地址空间 KERNEL_SPACE 中
            KERNEL_SPACE
			    .exclusive_access()
				.insert_framed_area(
			        kernel_stack_bottom.into(),
				    kernel_stack_top.into(),
					MapPermission::R | MapPermission::W,
				);
			KernelStack {
				pid: pid_handle.0,
			}
	   }
       //将一个类型为T的变量压入内核栈顶并返回其裸指针
       #[allow(unused)]
       pub fn push_on_top<T>(&self, value: T) -> *mut T where
		    T: Sized, {
            //获取当前内核栈顶在内核地址空间中的地址
			let kernel_stack_top = self.get_top();
			let ptr_mut = (kernel_stack_top - core::mem::size_of::<T>()) as *mut T;
			unsafe { *ptr_mut = value; }
			ptr_mut
		}
		pub fn get_top(&self) -> usize {
		    let (_, kernel_stack_top) = kernel_stack_position(self.pid);
	        kernel_stack_top
		}
}

impl Drop for KernelStack {
	    fn drop(&mut self) {
		let (kernel_stack_bottom, _) = kernel_stack_position(self.pid);
		let kernel_stack_bottom_va: VirtAddr = kernel_stack_bottom.into();
		KERNEL_SPACE
			.exclusive_access()          
            .remove_area_with_start_vpn(kernel_stack_bottom_va.into());
	    }
}
