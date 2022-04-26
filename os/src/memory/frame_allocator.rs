use super::{PhysAddr, PhysPageNum};
use crate::config::MEMORY_END;
use alloc::vec::Vec;
use core::fmt::{self, Debug, Formatter};
use crate::sync::UPSafeCell;
use lazy_static::*;
use crate::println;

/// manage a frame which has the same lifecycle as the tracker
pub struct FrameTracker {
    pub ppn: PhysPageNum,
}

impl FrameTracker {
    //由于这个物理页帧之前可能被分配过并用做其他用途，
    //在这里直接将这个物理页帧上的所有字节清零
    pub fn new(ppn: PhysPageNum) -> Self {
        let bytes_array = ppn.get_bytes_array();
        for i in bytes_array {
            *i = 0;
        }
        Self { ppn }
    }
}

impl Debug for FrameTracker {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("FrameTracker:PPN={:#x}", self.ppn.0))
    }
}

//当一个FrameTracker生命周期结束被编译器回收的时候，
//需要将它控制的物理页帧回收到FRAME_ALLOCATOR中
impl Drop for FrameTracker {
    fn drop(&mut self) {
        frame_dealloc(self.ppn);
    }
}

//创建一个物理页帧管理器的实例，以物理页号为单位进行物理页帧的分配和回收。
trait FrameAllocator {
    fn new() -> Self;    
    fn alloc(&mut self) -> Option<PhysPageNum>;
    fn dealloc(&mut self, ppn: PhysPageNum);
}

//最简单的栈式物理页帧管理策略
pub struct StackFrameAllocator {
    current: usize,  //空闲内存的起始物理页号
    end: usize,      //空闲内存use spin::Mutex;的结束物理页号
    recycled: Vec<usize>,//向量recycled以后入先出的方式保存了被回收的物理页
}

impl StackFrameAllocator {
    //在使用之前需要调用init方法将自身的[current, end)初始化为可用物理页号区间
    pub fn init(&mut self, l: PhysPageNum, r: PhysPageNum) {
        self.current = l.0;
        self.end = r.0;
        println!("last {} Physical Frames.", self.end - self.current);
    }
}

impl FrameAllocator for StackFrameAllocator {
    fn new() -> Self {
        //初始化：将区间两端均设为0，然后创建一个新的向量
        Self {
            current : 0,
            end     : 0,
            recycled   : Vec::new()
        }
    }

    fn alloc(&mut self) -> Option<PhysPageNum> {
        //检查栈recycled内有没有之前回收的物理页号，如果有的话直接弹出栈顶并返回
        if let Some(ppn) = self.recycled.pop() {
            Some(ppn.into())
        } else {
            //否则的话我们只能从之前从未分配过的物理页号区间[current,end)上进行分配
            //极端情况下可能出现内存耗尽分配失败，返回None
            if self.current == self.end {
                None
            } else {
                self.current += 1;//将管理器内部维护的current加1代表current已被分配
                Some((self.current - 1).into())//使用into方法将usize转换成物理页号PhysPageNum
            }
        }
    }
    fn dealloc(&mut self, ppn: PhysPageNum) {
        let ppn = ppn.0;
        //检查回收页面的合法性
        //1.该页面之前一定被分配出去过，因此它的物理页号一定<current
        //2.该页面未处在回收状态，即它的物理页号不能在栈recycled中找到
        if ppn >= self.current || self.recycled
            .iter()//获取栈上内容的迭代器
            .find(|&v| {*v == ppn})//寻找一个与输入物理页号相同的元素
            .is_some() {//若找到说明内核其他部分实现有误，直接报错退出
            panic!("Frame ppn={:#x} has not been allocated!", ppn);
        }
        // 将其压入recycled栈
        self.recycled.push(ppn);
        
    }
}

//使用UPSafeCell<T>来包裹栈式物理页帧分配器
type FrameAllocatorImpl = StackFrameAllocator;
lazy_static! {
    /// Lazy initialized instance of the frame allocator implementation. Currently using StackFrameAllocator.
    pub static ref FRAME_ALLOCATOR: UPSafeCell<FrameAllocatorImpl> =
    unsafe { UPSafeCell::new(FrameAllocatorImpl::new()) };
}

//将物理页帧全局管理器FRAME_ALLOCATOR初始化
pub fn init_frame_allocator() {
    extern "C" {
        fn ekernel();
    }
    //调用物理地址 PhysAddr 的 floor/ceil 方法分别下/上取整获得可用的物理页号区间
    FRAME_ALLOCATOR
        .exclusive_access()
        .init(PhysAddr::from(ekernel as usize).ceil(), PhysAddr::from(MEMORY_END).floor());
}

//其他内核模块调用的分配/回收物理页帧的接口
//RALL思想：将一个物理页帧的生命周期绑定到一个 FrameTracker 变量上，当一个
//FrameTracker被创建的时候，我们需要从FRAME_ALLOCATOR中分配一个物理页帧
pub fn frame_alloc() -> Option<FrameTracker> {
    FRAME_ALLOCATOR
        .exclusive_access()
        .alloc()
        .map(FrameTracker::new)
}

pub fn frame_dealloc(ppn: PhysPageNum) {
    FRAME_ALLOCATOR
        .exclusive_access()
        .dealloc(ppn);
}

//从其他内核模块的视角看来，物理页帧分配的接口是调用frame_alloc函数得到一个
//FrameTracker（如果物理内存还有剩余），它就代表了一个物理页帧，当它的生命
//周期结束之后它所控制的物理页帧将被自动回收
#[allow(unused)]
pub fn frame_allocator_test() {
    let mut v: Vec<FrameTracker> = Vec::new();
    for i in 0..5 {
        let frame = frame_alloc().unwrap();
        println!("{:?}", frame);
        v.push(frame);
    }
    v.clear();
    for i in 0..5 {
        let frame = frame_alloc().unwrap();
        println!("{:?}", frame);
        v.push(frame);
    }
    drop(v);
    println!("frame_allocator_test passed!");
}

