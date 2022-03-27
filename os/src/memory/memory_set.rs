use super::{frame_alloc, FrameTracker};
use super::{PTEFlags};
use super::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum,PageTable,PageTableEntry,};
use super::{VPNRange};
use crate::config::{MEMORY_END, MMIO, PAGE_SIZE, TRAMPOLINE};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::sync::Arc;
use core::arch::asm;
use spin::Mutex;
use lazy_static::*;
use bitflags::*;
use riscv::register::satp;
use crate::memory::address::StepByOne;

//创建内核地址空间的全局实例
//KERNEL_SPACE 在运行期间它第一次被用到时才会实际进行初始化，
//而它所占据的空间则是编译期被放在全局数据段中
lazy_static! {
    pub static ref KERNEL_SPACE: Arc<Mutex<MemorySet>> = Arc::new(unsafe {
        Mutex::new(MemorySet::new_kernel()
    )});
}

extern "C" {
    fn stext();
    fn etext();
    fn srodata();
    fn erodata();
    fn sdata();
    fn edata();
    fn sbss_with_stack();
    fn ebss();
    fn ekernel();
    fn strampoline();
}

pub struct MapArea {
    //一段虚拟页号的连续区间，表示该逻辑段在地址区间中的位置和长度
    vpn_range: VPNRange,
    data_frames: BTreeMap<VirtPageNum, FrameTracker>,
    map_type: MapType,
    map_perm: MapPermission,
}

// 描述该逻辑段内的所有虚拟页面映射到物理页帧的同一种方式，
//它是一个枚举类型，在内核当前的实现中支持两种方式
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum MapType {
    //恒等映射方式,主要是用在启用多级页表之后，内核仍能够在虚存地址
    //空间中访问一个特定的物理地址指向的物理内存
    Identical,
    //对于每个虚拟页面都有一个新分配的物理页帧与之对应，
    //虚地址与物理地址的映射关系是相对随机的
    Framed,
}

bitflags! {
    //控制该逻辑段的访问方式，它是页表项标志位 PTEFlags 的
    //一个子集，仅保留 U/R/W/X 四个标志位
    pub struct MapPermission: u8 {
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4;
    }
}

//地址空间是一系列有关联的不一定连续的逻辑段，这种关联一般是指这些
//逻辑段组成的虚拟内存空间与一个运行的程序，即这个运行的程序对代码和
//数据的直接访问范围限制在它关联的虚拟地址空间之内
pub struct MemorySet {
    //两部分合在一起构成了一个地址空间所需的所有物理页帧
    page_table: PageTable,//该地址空间的多级页表，挂着所有多级页表的节点所在的物理页帧
    areas: Vec<MapArea>,//挂着对应逻辑段中的数据所在的物理页帧
}

impl MapArea {
    //新建一个逻辑段结构体，传入的起始/终止虚拟地址会分别
    //被下取整/上取整为虚拟页号并传入迭代器vpn_range中
    pub fn new(
        start_va: VirtAddr,
        end_va: VirtAddr,
        map_type: MapType,
        map_perm: MapPermission,
    ) -> Self {
        let start_vpn: VirtPageNum = start_va.floor();
        let end_vpn: VirtPageNum = end_va.ceil();
        Self {
            vpn_range: VPNRange::new(start_vpn, end_vpn),
            data_frames: BTreeMap::new(),
            map_type,
            map_perm,
        }
    }

    //在虚拟页号 vpn 已经确定的情况下，它需要知道要将一个
    //怎么样的页表项插入多级页表
    pub fn map_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        let ppn: PhysPageNum;
        //页表项的物理页号则取决于当前逻辑段映射到物理内存的方式
        match self.map_type {
            //以恒等映射方式映射的时候，物理页号就等于虚拟页号
            MapType::Identical => {
                ppn = PhysPageNum(vpn.0);
            }
            //需要分配一个物理页帧让当前的虚拟页面可以映射过去，此时
            //页表项中的物理页号自然就是这个被分配的物理页帧的物理页号
            MapType::Framed => {
                let frame = frame_alloc().unwrap();
                ppn = frame.ppn;
                self.data_frames.insert(vpn, frame);
            }
        }
        //页表项的标志位来源于当前逻辑段的类型为 MapPermission 的
        //统一配置，只需将其转换为 PTEFlags
        let pte_flags = PTEFlags::from_bits(self.map_perm.bits).unwrap();
        //确定了页表项的标志位和物理页号之后，即可调用多级
        //页表PageTable的map接口来插入键值对
        page_table.map(vpn, ppn, pte_flags);
    }

    pub fn unmap_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        if self.map_type == MapType::Framed {
            //将虚拟页面被映射到的物理页帧FrameTracker从data_frames
            //中移除，这样这个物理页帧才能立即被回收以备后续分配
            self.data_frames.remove(&vpn);
        }
        page_table.unmap(vpn);//删除以传入的虚拟页号为键的键值对
    }

    //将当前逻辑段到物理内存的映射从传入的该逻辑段所属的地址空间的
    //多级页表中加入或删除。遍历逻辑段中的所有虚拟页面，并以每个
    //虚拟页面为单位依次在多级页表中进行键值对的插入或删除
    pub fn map(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.map_one(page_table, vpn);
        }
    }
    pub fn unmap(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.unmap_one(page_table, vpn);
        }
    }

    //将切片data中的数据拷贝到当前逻辑段实际被内核放置在的各物理页帧
    //上，从而在地址空间中通过该逻辑段就能访问这些数据
    pub fn copy_data(&mut self, page_table: &mut PageTable, data: &[u8]) {
        assert_eq!(self.map_type, MapType::Framed);
        let mut start: usize = 0;
        let mut current_vpn = self.vpn_range.get_start();
        let len = data.len();
        //遍历每一个需要拷贝数据的虚拟页面，在数据拷贝完成后调用step方法
        loop {
            //每个页面的数据拷贝需要确定源src和目标dst两个切片并直接使用copy_from_slice完成复制
            let src = &data[start..len.min(start + PAGE_SIZE)];
            let dst = &mut page_table
            //从传入的当前逻辑段所属的地址空间的多级页表中，
            //手动查找迭代到虚拟页号被映射到的物理页帧
                .translate(current_vpn)
                .unwrap()
                .ppn()
                .get_bytes_array()[..src.len()];//获取该物理页帧的字节数组型可变引用
            dst.copy_from_slice(src);
            start += PAGE_SIZE;
            if start >= len {
                break;
            }
            current_vpn.step();
        }
    }
}

impl MemorySet {
    //新建一个空的地址空间
    pub fn new_bare() -> Self {
        Self {
            page_table: PageTable::new(),
            areas: Vec::new(),
        }
    }

    //在当前地址空间插入一个新的逻辑段，如果是以Framed方式映射到物理内存，
    //还可以可选地在那些被映射到的物理页帧上写入一些初始化数据data
    fn push(&mut self, mut map_area: MapArea, data: Option<&[u8]>) {
        map_area.map(&mut self.page_table);
        if let Some(data) = data {
            map_area.copy_data(&mut self.page_table, data);
        }
        self.areas.push(map_area);
    }

    /// Mention that trampoline is not collected by areas.
    fn map_trampoline(&mut self) {
        extern "C"
        {
            fn strampoline();
        }
        self.page_table.map(
            VirtAddr::from(TRAMPOLINE).into(),
            PhysAddr::from(strampoline as usize).into(),
            PTEFlags::R | PTEFlags::X,
        );
    }
    
    //在当前地址空间插入一个 Framed 方式映射到物理内存的逻辑段
    //需保证同一地址空间内的任意两个逻辑段不能存在交集
    //同时也需维护地址空间的多级页表page_table记录的虚拟页号到页表项
    //的映射关系，用这个映射关系来找到向哪些物理页帧上拷贝初始数据
    pub fn insert_framed_area(
        &mut self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        permission: MapPermission,
    ) {
        self.push(
            MapArea::new(start_va, end_va, MapType::Framed, permission),
            None,
        );
    }

    /// Without kernel stacks.
    //生成内核的地址空间,映射跳板和地址空间中最低256GB中的内核逻辑段
    pub fn new_kernel() -> Self {
        let mut memory_set = Self::new_bare();
        // map trampoline
        memory_set.map_trampoline();//映射跳板
        // map kernel sections
        println!(".text [{:#x}, {:#x})", stext as usize, etext as usize);
        println!(".rodata [{:#x}, {:#x})", srodata as usize, erodata as usize);
        println!(".data [{:#x}, {:#x})", sdata as usize, edata as usize);
        println!(
            ".bss [{:#x}, {:#x})",
            sbss_with_stack as usize, ebss as usize
        );
        //从低地址到高地址依次创建5个逻辑段并通过push将它们插入到内核地址空间
        println!("mapping .text section");
        memory_set.push(
            MapArea::new(
                (stext as usize).into(),
                (etext as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::X,
            ),
            None,
        );
        println!("mapping .rodata section");
        memory_set.push(
            MapArea::new(
                (srodata as usize).into(),
                (erodata as usize).into(),
                MapType::Identical,
                MapPermission::R,
            ),
            None,
        );
        println!("mapping .data section");
        memory_set.push(
            MapArea::new(
                (sdata as usize).into(),
                (edata as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        println!("mapping .bss section");
        memory_set.push(
            MapArea::new(
                (sbss_with_stack as usize).into(),
                (ebss as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        println!("mapping physical memory");
        memory_set.push(
            MapArea::new(
                (ekernel as usize).into(),
                MEMORY_END.into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        println!("mapping memory-mapped registers");
        for pair in MMIO {
            memory_set.push(
                MapArea::new(
                    (*pair).0.into(),
                    ((*pair).0 + (*pair).1).into(),
                    MapType::Identical,
                    MapPermission::R | MapPermission::W,
                ),
                None,
            );
        }
        memory_set
    }

    //创建应用地址空间的时候，我们需要对get_app_data得到的ELF格式数据进行解析，
    //找到各个逻辑段所在位置和访问限制并插入进来，最终得到一个完整的应用地址空间
    /// Include sections in elf and trampoline,
    /// also returns user_sp_base and entry point.
    pub fn from_elf(elf_data: &[u8]) -> (Self, usize, usize) {
        let mut memory_set = Self::new_bare();
        //将跳板插入到应用地址空间
        memory_set.map_trampoline();
        // map program headers of elf, with U flag
        //解析传入的应用ELF数据并取出各个部分
        let elf = xmas_elf::ElfFile::new(elf_data).unwrap();
        let elf_header = elf.header;
        let magic = elf_header.pt1.magic;
        //取出ELF的魔数来判断它是不是一个合法的 ELF 
        assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");
        //得到program header的数目
        let ph_count = elf_header.pt2.ph_count();
        let mut max_end_vpn = VirtPageNum(0);
        //遍历所有的program header并将合适的区域加入到应用地址空间中
        for i in 0..ph_count {
            let ph = elf.program_header(i).unwrap();
            //确认 program header 的类型是 LOAD ，这表明它有被内核加载的必要
            if ph.get_type().unwrap() == xmas_elf::program::Type::Load {
                //ph.virtual_addr()和ph.mem_size()计算这一区域在应用地址空间中的位置
                let start_va: VirtAddr = (ph.virtual_addr() as usize).into();
                let end_va: VirtAddr = ((ph.virtual_addr() + ph.mem_size()) as usize).into();
                let mut map_perm = MapPermission::U;
                let ph_flags = ph.flags();
                //通过ph.flags()来确认这一区域访问方式的限制并将其转换为MapPermission类型
                if ph_flags.is_read() {
                    map_perm |= MapPermission::R;
                }
                if ph_flags.is_write() {
                    map_perm |= MapPermission::W;
                }
                if ph_flags.is_execute() {
                    map_perm |= MapPermission::X;
                }
                //创建逻辑段
                let map_area = MapArea::new(start_va, end_va, MapType::Framed, map_perm);
                max_end_vpn = map_area.vpn_range.get_end();
                memory_set.push(//push 到应用地址空间
                    map_area,
                    //需要完成数据拷贝，当前program header数据被存放的位置可以通过ph.offset()和ph.file_size()来找到
                    Some(&elf.input[ph.offset() as usize..(ph.offset() + ph.file_size()) as usize]),
                );
            }
        }
        //处理用户栈
        let max_end_va: VirtAddr = max_end_vpn.into();
        let mut user_stack_base: usize = max_end_va.into();
        user_stack_base += PAGE_SIZE;
        (
            memory_set,
            user_stack_base,
            elf.header.pt2.entry_point() as usize,
        )
    }

    //将无符号 64 位无符号整数写入当前CPU的satp CSR，从这一刻开始
    //SV39分页模式就被启用了，而且MMU会使用内核地址空间的多级页表进行地址转换
    pub fn activate(&self) {
        let satp = self.page_table.token();
        unsafe {
            satp::write(satp);
            asm!("sfence.vma");
        }
    }
}

//通过手动查内核多级页表的方式验证代码段和只读数据段不允许被写入，
//同时不允许从数据段上取指执行
pub fn remap_test() {
    let mut kernel_space = KERNEL_SPACE.lock();
    let mid_text: VirtAddr = ((stext as usize + etext as usize) / 2).into();
    let mid_rodata: VirtAddr = ((srodata as usize + erodata as usize) / 2).into();
    let mid_data: VirtAddr = ((sdata as usize + edata as usize) / 2).into();
    assert_eq!(
        kernel_space.page_table.translate(mid_text.floor()).unwrap().writable(),
        false
    );
    assert_eq!(
        kernel_space.page_table.translate(mid_rodata.floor()).unwrap().writable(),
        false,
    );
    assert_eq!(
        kernel_space.page_table.translate(mid_data.floor()).unwrap().executable(),
        false,
    );
    println!("remap_test passed!");
}
