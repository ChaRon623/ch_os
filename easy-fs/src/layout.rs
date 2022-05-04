use super::{get_block_cache, BlockDevice, BLOCK_SZ};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::{Debug, Formatter, Result};

const EFS_MAGIC: u32 = 0x3b800001;
const INODE_DIRECT_COUNT: usize = 28;
const NAME_LENGTH_LIMIT: usize = 27;//目录项 Dirent 最大允许保存长度为 27 的文件/目录名
const INODE_INDIRECT1_COUNT: usize = BLOCK_SZ / 4;
const INODE_INDIRECT2_COUNT: usize = INODE_INDIRECT1_COUNT * INODE_INDIRECT1_COUNT;
const DIRECT_BOUND: usize = INODE_DIRECT_COUNT;
const INDIRECT1_BOUND: usize = DIRECT_BOUND + INODE_INDIRECT1_COUNT;
#[allow(unused)]
const INDIRECT2_BOUND: usize = INDIRECT1_BOUND + INODE_INDIRECT2_COUNT;

//磁盘上的数据结构，存放在磁盘上编号为 0 的块的起始处
#[repr(C)]
pub struct SuperBlock {
    magic: u32,//用于文件系统合法性验证的魔数
    pub total_blocks: u32,//文件系统的总块数
    pub inode_bitmap_blocks: u32,//索引节点位图，长度为若干个块。它记录了后面的索引节点区域中有哪些索引节点已经被分配出去使用了，而哪些还尚未被分配出去。
    pub inode_area_blocks: u32,//索引节点区域，长度为若干个块。其中的每个块都存储了若干个索引节点。
    pub data_bitmap_blocks: u32,//数据块位图，长度为若干个块。它记录了后面的数据块区域中有哪些数据块已经被分配出去使用了，而哪些还尚未被分配出去。
    pub data_area_blocks: u32,//数据块区域，其中的每一个已经分配出去的块保存了文件或目录中的具体数据内容。
}

impl Debug for SuperBlock {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.debug_struct("SuperBlock")
            .field("total_blocks", &self.total_blocks)
            .field("inode_bitmap_blocks", &self.inode_bitmap_blocks)
            .field("inode_area_blocks", &self.inode_area_blocks)
            .field("data_bitmap_blocks", &self.data_bitmap_blocks)
            .field("data_area_blocks", &self.data_area_blocks)
            .finish()
    }
}

impl SuperBlock {
    //创建一个 easy-fs 的时候对超级块进行初始化
    pub fn initialize(
        &mut self,
        total_blocks: u32,
        inode_bitmap_blocks: u32,
        inode_area_blocks: u32,
        data_bitmap_blocks: u32,
        data_area_blocks: u32,
    ) {
        *self = Self {
            magic: EFS_MAGIC,
            total_blocks,
            inode_bitmap_blocks,
            inode_area_blocks,
            data_bitmap_blocks,
            data_area_blocks,
        }
    }
    //通过魔数判断超级块所在的文件系统是否合法
    pub fn is_valid(&self) -> bool {
        self.magic == EFS_MAGIC
    }
}

#[derive(PartialEq)]
pub enum DiskInodeType {
    File,
    Directory,
}

type IndirectBlock = [u32; BLOCK_SZ / 4];
//作为一个文件而言，它的内容在文件系统看来没有任何既定的格式，
//都只是一个字节序列。因此每个保存内容的数据块都只是一个字节数组
type DataBlock = [u8; BLOCK_SZ];

//每个文件/目录在磁盘上均以一个 DiskInode 的形式存储
#[repr(C)]
pub struct DiskInode {
    pub size: u32,//文件/目录内容的字节数
    pub direct: [u32; INODE_DIRECT_COUNT],//直接索引
    pub indirect1: u32,//一级间接索引
    pub indirect2: u32,//二级间接索引
    type_: DiskInodeType,//表示索引节点的类型, 目前仅支持文件File和目录Directory两种类型
}

impl DiskInode {
    //初始化一个 DiskInode 为一个文件或目录
    pub fn initialize(&mut self, type_: DiskInodeType) {
        self.size = 0;
        self.direct.iter_mut().for_each(|v| *v = 0);
        //indirect1/2 均被初始化为 0 。因为最开始文件内容的大小为 0 字节，并不会用到一级/二级索引
        self.indirect1 = 0;
        self.indirect2 = 0;
        self.type_ = type_;
    }
    //确认 DiskInode 的类型为文件还是目录
    pub fn is_dir(&self) -> bool {
        self.type_ == DiskInodeType::Directory
    }
    #[allow(unused)]
    pub fn is_file(&self) -> bool {
        self.type_ == DiskInodeType::File
    }
    /// 计算为了容纳自身 size 字节的内容需要多少个数据块
    pub fn data_blocks(&self) -> u32 {
        Self::_data_blocks(self.size)
    }
    fn _data_blocks(size: u32) -> u32 {
        (size + BLOCK_SZ as u32 - 1) / BLOCK_SZ as u32
    }
    ///包含数据块，还需要统计索引块。计算的方法也很简单，先调用 
    //data_blocks 得到需要多少数据块，再根据数据块数目所处的区间统计索引块即可
    pub fn total_blocks(size: u32) -> u32 {
        let data_blocks = Self::_data_blocks(size) as usize;
        let mut total = data_blocks as usize;
        // indirect1
        if data_blocks > INODE_DIRECT_COUNT {
            total += 1;
        }
        // indirect2
        if data_blocks > INDIRECT1_BOUND {
            total += 1;
            // sub indirect1
            total +=
                (data_blocks - INDIRECT1_BOUND + INODE_INDIRECT1_COUNT - 1) / INODE_INDIRECT1_COUNT;
        }
        total as u32
    }
    //计算将一个 DiskInode 的 size 扩容到 new_size 需要额外多少个数据和索引块
    pub fn blocks_num_needed(&self, new_size: u32) -> u32 {
        assert!(new_size >= self.size);
        Self::total_blocks(new_size) - Self::total_blocks(self.size)
    }

    //从索引中查到它自身用于保存文件内容的第 block_id 个数据块的块编号，这样后续才能对这个数据块进行访问
    pub fn get_block_id(&self, inner_id: u32, block_device: &Arc<dyn BlockDevice>) -> u32 {
        let inner_id = inner_id as usize;
        if inner_id < INODE_DIRECT_COUNT {
            self.direct[inner_id]
        } else if inner_id < INDIRECT1_BOUND {
            //对一个索引块进行操作的时候，我们将其解析为磁盘数据结构 IndirectBlock ，实质上就是
            //一个 u32 数组，每个都指向一个下一级索引块或者数据块
            get_block_cache(self.indirect1 as usize, Arc::clone(block_device))
                .lock()
                .read(0, |indirect_block: &IndirectBlock| {
                    indirect_block[inner_id - INODE_DIRECT_COUNT]
                })
        } else {
            //对于二级索引的情况，需要先查二级索引块找到挂在它下面的一级索引块，再通过一级索引块找到数据块
            let last = inner_id - INDIRECT1_BOUND;
            let indirect1 = get_block_cache(self.indirect2 as usize, Arc::clone(block_device))
                .lock()
                .read(0, |indirect2: &IndirectBlock| {
                    indirect2[last / INODE_INDIRECT1_COUNT]
                });
            get_block_cache(indirect1 as usize, Arc::clone(block_device))
                .lock()
                .read(0, |indirect1: &IndirectBlock| {
                    indirect1[last % INODE_INDIRECT1_COUNT]
                })
        }
    }
    pub fn increase_size(
        &mut self,
        //容量扩充之后的文件大小
        new_size: u32,
        //一个保存了本次容量扩充所需块编号的向量，这些块都是由上层的磁盘块管理器负责分配的
        new_blocks: Vec<u32>,
        block_device: &Arc<dyn BlockDevice>,
    ) {
        let mut current_blocks = self.data_blocks();
        self.size = new_size;
        let mut total_blocks = self.data_blocks();
        let mut new_blocks = new_blocks.into_iter();
        // fill direct
        while current_blocks < total_blocks.min(INODE_DIRECT_COUNT as u32) {
            self.direct[current_blocks as usize] = new_blocks.next().unwrap();
            current_blocks += 1;
        }
        // alloc indirect1
        if total_blocks > INODE_DIRECT_COUNT as u32 {
            if current_blocks == INODE_DIRECT_COUNT as u32 {
                self.indirect1 = new_blocks.next().unwrap();
            }
            current_blocks -= INODE_DIRECT_COUNT as u32;
            total_blocks -= INODE_DIRECT_COUNT as u32;
        } else {
            return;
        }
        // fill indirect1
        get_block_cache(self.indirect1 as usize, Arc::clone(block_device))
            .lock()
            .modify(0, |indirect1: &mut IndirectBlock| {
                while current_blocks < total_blocks.min(INODE_INDIRECT1_COUNT as u32) {
                    indirect1[current_blocks as usize] = new_blocks.next().unwrap();
                    current_blocks += 1;
                }
            });
        // alloc indirect2
        if total_blocks > INODE_INDIRECT1_COUNT as u32 {
            if current_blocks == INODE_INDIRECT1_COUNT as u32 {
                self.indirect2 = new_blocks.next().unwrap();
            }
            current_blocks -= INODE_INDIRECT1_COUNT as u32;
            total_blocks -= INODE_INDIRECT1_COUNT as u32;
        } else {
            return;
        }
        // fill indirect2 from (a0, b0) -> (a1, b1)
        let mut a0 = current_blocks as usize / INODE_INDIRECT1_COUNT;
        let mut b0 = current_blocks as usize % INODE_INDIRECT1_COUNT;
        let a1 = total_blocks as usize / INODE_INDIRECT1_COUNT;
        let b1 = total_blocks as usize % INODE_INDIRECT1_COUNT;
        // alloc low-level indirect1
        get_block_cache(self.indirect2 as usize, Arc::clone(block_device))
            .lock()
            .modify(0, |indirect2: &mut IndirectBlock| {
                while (a0 < a1) || (a0 == a1 && b0 < b1) {
                    if b0 == 0 {
                        indirect2[a0] = new_blocks.next().unwrap();
                    }
                    // fill current
                    get_block_cache(indirect2[a0] as usize, Arc::clone(block_device))
                        .lock()
                        .modify(0, |indirect1: &mut IndirectBlock| {
                            indirect1[b0] = new_blocks.next().unwrap();
                        });
                    // move to next
                    b0 += 1;
                    if b0 == INODE_INDIRECT1_COUNT {
                        b0 = 0;
                        a0 += 1;
                    }
                }
            });
    }

    /// Clear size to zero and return blocks that should be deallocated.
    ///
    /// We will clear the block contents to zero later.
    //清空文件的内容并回收所有数据和索引块, 将回收的所有块的编号保存在一个向量中返回给磁盘块管理器
    pub fn clear_size(&mut self, block_device: &Arc<dyn BlockDevice>) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut data_blocks = self.data_blocks() as usize;
        self.size = 0;
        let mut current_blocks = 0usize;
        // direct
        while current_blocks < data_blocks.min(INODE_DIRECT_COUNT) {
            v.push(self.direct[current_blocks]);
            self.direct[current_blocks] = 0;
            current_blocks += 1;
        }
        // indirect1 block
        if data_blocks > INODE_DIRECT_COUNT {
            v.push(self.indirect1);
            data_blocks -= INODE_DIRECT_COUNT;
            current_blocks = 0;
        } else {
            return v;
        }
        // indirect1
        get_block_cache(self.indirect1 as usize, Arc::clone(block_device))
            .lock()
            .modify(0, |indirect1: &mut IndirectBlock| {
                while current_blocks < data_blocks.min(INODE_INDIRECT1_COUNT) {
                    v.push(indirect1[current_blocks]);
                    //indirect1[current_blocks] = 0;
                    current_blocks += 1;
                }
            });
        self.indirect1 = 0;
        // indirect2 block
        if data_blocks > INODE_INDIRECT1_COUNT {
            v.push(self.indirect2);
            data_blocks -= INODE_INDIRECT1_COUNT;
        } else {
            return v;
        }
        // indirect2
        assert!(data_blocks <= INODE_INDIRECT2_COUNT);
        let a1 = data_blocks / INODE_INDIRECT1_COUNT;
        let b1 = data_blocks % INODE_INDIRECT1_COUNT;
        get_block_cache(self.indirect2 as usize, Arc::clone(block_device))
            .lock()
            .modify(0, |indirect2: &mut IndirectBlock| {
                // full indirect1 blocks
                for entry in indirect2.iter_mut().take(a1) {
                    v.push(*entry);
                    get_block_cache(*entry as usize, Arc::clone(block_device))
                        .lock()
                        .modify(0, |indirect1: &mut IndirectBlock| {
                            for entry in indirect1.iter() {
                                v.push(*entry);
                            }
                        });
                }
                // last indirect1 block
                if b1 > 0 {
                    v.push(indirect2[a1]);
                    get_block_cache(indirect2[a1] as usize, Arc::clone(block_device))
                        .lock()
                        .modify(0, |indirect1: &mut IndirectBlock| {
                            for entry in indirect1.iter().take(b1) {
                                v.push(*entry);
                            }
                        });
                    //indirect2[a1] = 0;
                }
            });
        self.indirect2 = 0;
        v
    }

    //将文件内容从 offset 字节开始的部分读到内存中的缓冲区 buf 中，并返回实际读到的字节数。
    //如果文件剩下的内容还足够多，那么缓冲区会被填满；否则文件剩下的全部内容都会被读到缓冲区中。
    pub fn read_at(
        &self,
        offset: usize,
        buf: &mut [u8],
        block_device: &Arc<dyn BlockDevice>,
    ) -> usize {
        let mut start = offset;
        let end = (offset + buf.len()).min(self.size as usize);
        //如果要读取的内容超出了文件的范围，那么直接返回 0 ，表示读取不到任何内容
        if start >= end {
            return 0;
        }
        let mut start_block = start / BLOCK_SZ;//目前是文件内部第多少个数据块
        let mut read_size = 0usize;
        //遍历位于字节区间 start,end 中间的那些块，将它们视为一个 DataBlock （也就是一个字节
        //数组），并将其中的部分内容复制到缓冲区 buf 中适当的区域
        loop {
            // calculate end of current block
            let mut end_current_block = (start / BLOCK_SZ + 1) * BLOCK_SZ;
            end_current_block = end_current_block.min(end);
            // read and update read size
            let block_read_size = end_current_block - start;
            let dst = &mut buf[read_size..read_size + block_read_size];
            //后传入 get_block_cache 中将正确的数据块缓存到内存中进行访问
            get_block_cache(
                // 先调用 get_block_id从索引中查到这个数据块在块设备中的块编号
                self.get_block_id(start_block as u32, block_device) as usize,
                Arc::clone(block_device),
            )
            .lock()
            .read(0, |data_block: &DataBlock| {
                let src = &data_block[start % BLOCK_SZ..start % BLOCK_SZ + block_read_size];
                dst.copy_from_slice(src);
            });
            read_size += block_read_size;
            // move to next block
            if end_current_block == end {
                break;
            }
            start_block += 1;
            start = end_current_block;
        }
        read_size
    }
    /// File size must be adjusted before.
    pub fn write_at(
        &mut self,
        offset: usize,
        buf: &[u8],
        block_device: &Arc<dyn BlockDevice>,
    ) -> usize {
        let mut start = offset;
        let end = (offset + buf.len()).min(self.size as usize);
        assert!(start <= end);
        let mut start_block = start / BLOCK_SZ;
        let mut write_size = 0usize;
        loop {
            // calculate end of current block
            let mut end_current_block = (start / BLOCK_SZ + 1) * BLOCK_SZ;
            end_current_block = end_current_block.min(end);
            // write and update write size
            let block_write_size = end_current_block - start;
            get_block_cache(
                self.get_block_id(start_block as u32, block_device) as usize,
                Arc::clone(block_device),
            )
            .lock()
            .modify(0, |data_block: &mut DataBlock| {
                let src = &buf[write_size..write_size + block_write_size];
                let dst = &mut data_block[start % BLOCK_SZ..start % BLOCK_SZ + block_write_size];
                dst.copy_from_slice(src);
            });
            write_size += block_write_size;
            // move to next block
            if end_current_block == end {
                break;
            }
            start_block += 1;
            start = end_current_block;
        }
        write_size
    }
}

//每个目录项都是一个二元组
#[repr(C)]
pub struct DirEntry {
    name: [u8; NAME_LENGTH_LIMIT + 1],//首个元素是目录下面的一个文件（或子目录）的文件名（或目录名）
    inode_number: u32,//文件（或子目录）所在的索引节点编号
}

pub const DIRENT_SZ: usize = 32;

impl DirEntry {
    pub fn empty() -> Self {
        Self {
            name: [0u8; NAME_LENGTH_LIMIT + 1],
            inode_number: 0,
        }
    }
    pub fn new(name: &str, inode_number: u32) -> Self {
        let mut bytes = [0u8; NAME_LENGTH_LIMIT + 1];
        bytes[..name.len()].copy_from_slice(name.as_bytes());
        Self {
            name: bytes,
            inode_number,
        }
    }
    pub fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self as *const _ as usize as *const u8, DIRENT_SZ) }
    }
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self as *mut _ as usize as *mut u8, DIRENT_SZ) }
    }
    pub fn name(&self) -> &str {
        let len = (0usize..).find(|i| self.name[*i] == 0).unwrap();
        core::str::from_utf8(&self.name[..len]).unwrap()
    }
    pub fn inode_number(&self) -> u32 {
        self.inode_number
    }
}
