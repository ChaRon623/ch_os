use super::PageTableEntry;
use crate::config::{PAGE_SIZE, PAGE_SIZE_BITS};
use core::fmt::{self, Debug, Formatter};

//SV39支持的物理地址位宽为56位
const PA_WIDTH_SV39: usize = 56;
const PPN_WIDTH_SV39: usize = PA_WIDTH_SV39 - PAGE_SIZE_BITS;

const VA_WIDTH_SV39: usize = 39;
const VPN_WIDTH_SV39: usize = VA_WIDTH_SV39 - PAGE_SIZE_BITS;

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysAddr(pub usize);//物理地址

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtAddr(pub usize);//虚拟地址

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysPageNum(pub usize);//物理页号

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtPageNum(pub usize);//虚拟页号

/// Debugging

impl Debug for VirtAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("VA:{:#x}", self.0))
    }
}
impl Debug for VirtPageNum {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("VPN:{:#x}", self.0))
    }
}
impl Debug for PhysAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PA:{:#x}", self.0))
    }
}
impl Debug for PhysPageNum {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PPN:{:#x}", self.0))
    }
}

//元组式结构体转换为物理地址
impl From<usize> for PhysAddr {
    fn from(v: usize) -> Self { Self(v & ( (1 << PA_WIDTH_SV39) - 1 )) }
}
impl From<usize> for PhysPageNum {
    fn from(v: usize) -> Self { Self(v & ( (1 << PPN_WIDTH_SV39) - 1 )) }
}

impl From<usize> for VirtAddr {
    fn from(v: usize) -> Self {
        Self(v & ((1 << VA_WIDTH_SV39) - 1))
    }
}
impl From<usize> for VirtPageNum {
    fn from(v: usize) -> Self {
        Self(v & ((1 << VPN_WIDTH_SV39) - 1))
    }
}

impl From<PhysAddr> for usize {
    fn from(v: PhysAddr) -> Self {
        v.0
    }
}
impl From<PhysPageNum> for usize {
    fn from(v: PhysPageNum) -> Self {
        v.0
    }
}
impl From<VirtAddr> for usize {
    fn from(v: VirtAddr) -> Self {
        v.0
    }
}
impl From<VirtPageNum> for usize {
    fn from(v: VirtPageNum) -> Self {
        v.0
    }
}

//物理地址需要保证它与页面大小对齐才能通过右移转换为物理页号
impl From<PhysAddr> for PhysPageNum {
    fn from(v: PhysAddr) -> Self {
        assert_eq!(v.page_offset(), 0);
        v.floor()
    }
}

//物理页号到物理地址的转换只需左移12位即可
impl From<PhysPageNum> for PhysAddr {
    fn from(v: PhysPageNum) -> Self {
        Self(v.0 << PAGE_SIZE_BITS)
    }
}

impl From<VirtAddr> for VirtPageNum {
    fn from(v: VirtAddr) -> Self {
        assert_eq!(v.page_offset(), 0);
        v.floor()
    }
}
impl From<VirtPageNum> for VirtAddr {
    fn from(v: VirtPageNum) -> Self {
        Self(v.0 << PAGE_SIZE_BITS)
    }
}

//地址和页号的转换
impl PhysAddr {
    pub fn page_offset(&self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }
    //对于不对齐的情况，物理地址不能通过 From/Into 转换为物理页号，
    //而是需要通过它自己的 floor 或 ceil 方法来进行下取整或上取整的转换
    pub fn floor(&self) -> PhysPageNum {
        PhysPageNum(self.0 / PAGE_SIZE)
    }
    pub fn ceil(&self) -> PhysPageNum {
        PhysPageNum((self.0 - 1 + PAGE_SIZE) / PAGE_SIZE)
    }
	///Check page aligned 
    pub fn aligned(&self) -> bool {
        self.page_offset() == 0
    }
}

impl VirtAddr {
    pub fn floor(&self) -> VirtPageNum {
        VirtPageNum(self.0 / PAGE_SIZE)
    }
    pub fn ceil(&self) -> VirtPageNum {
        VirtPageNum((self.0 - 1 + PAGE_SIZE) / PAGE_SIZE)
    }
    pub fn page_offset(&self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }
    pub fn aligned(&self) -> bool {
        self.page_offset() == 0
    }
}

//内核中访问物理页帧的方法
//构造可变引用来直接访问物理页号 PhysPageNum 对应的物理页帧，
//不同的引用类型对应于物理页帧上的一种不同的内存布局
//具体实现：先把物理页号转为物理地址PhysAddr，然后再转成usize形式的物理地址
//然后直接将它转为裸指针用来访问物理地址指向的物理内存
impl PhysPageNum {
    //返回一个页表项定长数组的可变引用，代表多级页表中的一个节点
    pub fn get_pte_array(&self) -> &'static mut [PageTableEntry] {
        let pa: PhysAddr = (*self).into();
        unsafe { core::slice::from_raw_parts_mut(pa.0 as *mut PageTableEntry, 512) }
    }
    //返回一个字节数组的可变引用，可以以字节为粒度对物理页帧上的数据进行访问
    pub fn get_bytes_array(&self) -> &'static mut [u8] {
        let pa: PhysAddr = (*self).into();
        unsafe { core::slice::from_raw_parts_mut(pa.0 as *mut u8, 4096) }
    }
    //泛型函数，可以获取一个恰好放在一个物理页帧开头的类型为T的数据的可变引用
    pub fn get_mut<T>(&self) -> &'static mut T {
        let pa: PhysAddr = (*self).into();
        pa.get_mut()
    }
}

impl VirtPageNum {
    //取出虚拟页号的三级页索引，并按照从高到低的顺序返回
    pub fn indexes(&self) -> [usize; 3] {
        let mut vpn = self.0;
        let mut idx = [0usize; 3];
        for i in (0..3).rev() {
            idx[i] = vpn & 511;
            vpn >>= 9;
        }
        idx
    }
}

impl PhysAddr {
    ///Get mutable reference to `PhysAddr` value 
    pub fn get_mut<T>(&self) -> &'static mut T {
        unsafe { (self.0 as *mut T).as_mut().unwrap() }
    }
}

pub trait StepByOne {
    fn step(&mut self);
}

impl StepByOne for VirtPageNum {
    fn step(&mut self) {
        self.0 += 1;
    }
}

#[derive(Copy, Clone)]
pub struct SimpleRange<T> where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug, {
		    l: T,
			r: T,
}

impl<T> SimpleRange<T> where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug, {
	pub fn new(start: T, end: T) -> Self {
		assert!(start <= end, "start {:?} > end {:?}!", start, end);
		Self { l: start, r: end }
	}
	pub fn get_start(&self) -> T { self.l }
	pub fn get_end(&self) -> T { self.r }
}

impl<T> IntoIterator for SimpleRange<T> where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug, {
	type Item = T;
	type IntoIter = SimpleRangeIterator<T>;
	fn into_iter(self) -> Self::IntoIter {
		SimpleRangeIterator::new(self.l, self.r)
	}
}	

pub struct SimpleRangeIterator<T> where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug, {
	current: T,
	end: T,
}

impl<T> SimpleRangeIterator<T> where
	T: StepByOne + Copy + PartialEq + PartialOrd + Debug, {
	pub fn new(l: T, r: T) -> Self {
		Self { current: l, end: r, }
	}
}

impl<T> Iterator for SimpleRangeIterator<T> where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug, {
	type Item = T;
	fn next(&mut self) -> Option<Self::Item> {
		if self.current == self.end {
			None
		} else {
		     let t = self.current;
	         self.current.step();
	         Some(t)			
	    }
    }
}

pub type VPNRange = SimpleRange<VirtPageNum>;
