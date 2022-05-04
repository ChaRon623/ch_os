use bitflags::*;
use super::{frame_alloc, FrameTracker, PhysAddr, PhysPageNum, StepByOne, VirtAddr, VirtPageNum};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

bitflags! {
    //bitflags将一个u8封装成一个标志位的集合类型，支持一些常见的集合运算
    pub struct PTEFlags: u8 {
        const V = 1 << 0;
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4;
        const G = 1 << 5;
        const A = 1 << 6;
        const D = 1 << 7;
    }
}

//让编译器自动为PageTableEntry实现Copy/Clone Trait，
//使这个类型以值语义赋值/传参的时候不会发生所有权转移，而是拷贝一份新的副本
#[derive(Copy, Clone)]
#[repr(C)]
pub struct PageTableEntry {
    pub bits: usize,
}

impl PageTableEntry {
    //从一个物理页号 PhysPageNum和一个页表项标志位 PTEFlags
    //生成一个页表项 PageTableEntry 实例
    pub fn new(ppn: PhysPageNum, flags: PTEFlags) -> Self {
        PageTableEntry {
            bits: ppn.0 << 10 | flags.bits as usize,
        }
    }
    //生成一个全零的页表项，这里隐含着该页表项的V标志位为0,因此不合法
    pub fn empty() -> Self {
        PageTableEntry { bits: 0 }
    }
    //从一个页表项中取出物理页号
    pub fn ppn(&self) -> PhysPageNum {
        (self.bits >> 10 & ((1usize << 44) - 1)).into()
    }
    //从一个页表项中取出标志位
    pub fn flags(&self) -> PTEFlags {
        PTEFlags::from_bits(self.bits as u8).unwrap()
    }
    //判断一个页表项的 V/R/W/X 标志位是否为 1
    //相当于判断两个集合的交集是否为空集
    pub fn is_valid(&self) -> bool {
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }

    pub fn readable(&self) -> bool {
        (self.flags() & PTEFlags::R) != PTEFlags::empty()
    }
    pub fn writable(&self) -> bool {
        (self.flags() & PTEFlags::W) != PTEFlags::empty()
    }

    pub fn executable(&self) -> bool {
        (self.flags() & PTEFlags::X) != PTEFlags::empty()
    }
}

//SV39多级页表是以节点为单位进行管理的，
//每个节点恰好存储在一个物理页帧中，它的位置可以用一个物理页号来表示
//每个应用的地址空间都对应一个不同的多级页表，这也就意味这不同页表的起始
//地址（即页表根节点的地址）是不一样的。因此 PageTable 要保存它根节点的
//物理页号 root_ppn 作为页表唯一的区分标志。
pub struct PageTable {
    root_ppn: PhysPageNum,
    //向量frames以FrameTracker的形式保存了页表所有的节点（包括根节点）所在的物理页帧
    frames: Vec<FrameTracker>,
}

/// Assume that it won't oom when creating/mapping.
impl PageTable {
    //新建一个PageTable的时候，它只需有一个根节点。为此我们需要分配一个物理
    //页帧FrameTracker并挂在向量frames下，然后更新根节点的物理页号root_ppn
    pub fn new() -> Self {
        let frame = frame_alloc().unwrap();
        PageTable {
            root_ppn: frame.ppn,
            frames: vec![frame],
        }
    }

    //在多级页表中找到一个虚拟页号对应的页表项的可变引用
    //如果在遍历的过程中发现有节点尚未创建则会新建一个节点
    fn find_pte_create(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;//当前节点的物理页号，最开始指向多级页表的根节点
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            //取出当前节点的页表项数组，并根据当前级页索引找到对应的页表项
            let pte = &mut ppn.get_pte_array()[*idx];
            if i == 2 {
                //如果当前节点是一个叶节点，那么直接返回这个页表项的可变引用，否则尝试向下走
                result = Some(pte);
                break;
            }
            //有节点尚未创建则会新建一个节点
            //更新作为下级节点指针的页表项，并将新分配的物理页帧移动到向量frames中方便后续的自动回收
            if !pte.is_valid() {
                let frame = frame_alloc().unwrap();
                //将标志位 V 置 1，不然硬件在查多级页表的时候，会认为这个页表项不合法，
                //从而触发 Page Fault 而不能向下走
                *pte = PageTableEntry::new(frame.ppn, PTEFlags::V);
                self.frames.push(frame);
            }
            ppn = pte.ppn();
        }
        result
    }

    //与上一个函数的不同在于当找不到合法叶子节点的时候不会新建叶子节点而是
    //直接返回 None 即查找失败
    fn find_pte(&self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.get_pte_array()[*idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                return None;
            }
            ppn = pte.ppn();
        }
        result
    }

    //多级页表并不是被创建出来之后就不再变化的，为了MMU能够通过地址转换
    //正确找到应用地址空间中的数据实际被内核放在内存中位置，操作系统需要动态
    //维护一个虚拟页号到页表项的映射，支持插入/删除键值对
    #[allow(unused)]
    pub fn map(&mut self, vpn: VirtPageNum, ppn: PhysPageNum, flags: PTEFlags) {
        //根据虚拟页号找到页表项，然后修改或者直接清空其内容即可
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(!pte.is_valid(), "vpn {:?} is mapped before mapping", vpn);
        *pte = PageTableEntry::new(ppn, flags | PTEFlags::V);
    }
    #[allow(unused)]
    pub fn unmap(&mut self, vpn: VirtPageNum) {
        let pte = self.find_pte(vpn).unwrap();
        assert!(pte.is_valid(), "vpn {:?} is invalid before unmapping", vpn);
        *pte = PageTableEntry::empty();
    }

    //提供一种类似 MMU 操作的手动查页表的方法
    /// Temporarily used to get arguments from user space.
    pub fn from_token(satp: usize) -> Self {
        Self {
            //临时创建一个专用来手动查页表的 PageTable
            //从传入的satp token中得到多级页表根节点的物理页号
            root_ppn: PhysPageNum::from(satp & ((1usize << 44) - 1)),
            //frames字段为空，也即不实际控制任何资源
            frames: Vec::new(),
        }
    }

    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        //如果能够找到页表项，那么它会将页表项拷贝一份并返回，否则就返回一个 None
        self.find_pte(vpn).map(|pte| *pte)
    }

    pub fn translate_va(&self, va: VirtAddr) -> Option<PhysAddr> {
        self.find_pte(va.clone().floor()).map(|pte| {
            let aligned_pa: PhysAddr = pte.ppn().into();
            let offset = va.page_offset();
            let aligned_pa_usize: usize = aligned_pa.into();
            (aligned_pa_usize + offset).into()
        })
    }

    //构造一个无符号64位无符号整数，使得其分页模式为SV39，
    //且将当前多级页表的根节点所在的物理页号填充进去
    pub fn token(&self) -> usize {
        8usize << 60 | self.root_ppn.0
    }
}

/// translate a pointer to a mutable u8 Vec through page table
pub fn translated_byte_buffer(token: usize, ptr: *const u8, len: usize) -> Vec<&'static mut [u8]> {
    let page_table = PageTable::from_token(token);
    let mut start = ptr as usize;
    let end = start + len;
    let mut v = Vec::new();
    while start < end {
        let start_va = VirtAddr::from(start);
        let mut vpn = start_va.floor();
        let ppn = page_table.translate(vpn).unwrap().ppn();
        vpn.step();
        let mut end_va: VirtAddr = vpn.into();
        end_va = end_va.min(VirtAddr::from(end));
        if end_va.page_offset() == 0 {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..]);
        } else {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..end_va.page_offset()]);
        }
        start = end_va.into();
    }
    v
}

//从内核地址空间之外的某个应用的用户态地址空间中拿到一个字符串，其原理就是针对应用的字符串
//中字符的用户态虚拟地址，查页表，找到对应的内核虚拟地址，逐字节地构造字符串，直到发现一个\0为止
pub fn translated_str(token: usize, ptr: *const u8) -> String {
    let page_table = PageTable::from_token(token);
    let mut string = String::new();
    let mut va = ptr as usize;
    loop {
        let ch: u8 = *(page_table
            .translate_va(VirtAddr::from(va))
            .unwrap()
            .get_mut());
        if ch == 0 {
            break;
        }
        string.push(ch as char);
        va += 1;
    }
    string
}

///translate a generic through page table and return a mutable reference
pub fn translated_refmut<T>(token: usize, ptr: *mut T) -> &'static mut T {
    let page_table = PageTable::from_token(token);
    let va = ptr as usize;
    page_table
        .translate_va(VirtAddr::from(va))
        .unwrap()
        .get_mut()
}

pub struct UserBuffer {
    pub buffers: Vec<&'static mut [u8]>,
}

impl UserBuffer {
    pub fn new(buffers: Vec<&'static mut [u8]>) -> Self {
        Self { buffers }
    }
    pub fn len(&self) -> usize {
        let mut total: usize = 0;
        for b in self.buffers.iter() {
            total += b.len();
        }
        total
    }
}

impl IntoIterator for UserBuffer {
    type Item = *mut u8;
    type IntoIter = UserBufferIterator;
    fn into_iter(self) -> Self::IntoIter {
        UserBufferIterator {
            buffers: self.buffers,
            current_buffer: 0,
            current_idx: 0,
        }
    }
}

pub struct UserBufferIterator {
    buffers: Vec<&'static mut [u8]>,
    current_buffer: usize,
    current_idx: usize,
}

impl Iterator for UserBufferIterator {
    type Item = *mut u8;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_buffer >= self.buffers.len() {
            None
        } else {
            let r = &mut self.buffers[self.current_buffer][self.current_idx] as *mut _;
            if self.current_idx + 1 == self.buffers[self.current_buffer].len() {
                self.current_idx = 0;
                self.current_buffer += 1;
            } else {
                self.current_idx += 1;
            }
            Some(r)
        }
    }
}

