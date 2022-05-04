use super::{get_block_cache, BlockDevice, BLOCK_SZ};
use alloc::sync::Arc;

//将位图区域中的一个磁盘块解释为长度为64的一个u64 数组， 每个 u64 打包了一组 64 bits，
//于是整个数组包含 64 x 64 = 4096 bits，且可以以组为单位进行操作。
type BitmapBlock = [u64; 64];

const BLOCK_BITS: usize = BLOCK_SZ * 8;

pub struct Bitmap {
    //所在区域的起始块编号以及区域的长度为多少个块
    start_block_id: usize,
    blocks: usize,
}

/// Return (block_pos, bits64_pos, inner_pos)
fn decomposition(mut bit: usize) -> (usize, usize, usize) {
    let block_pos = bit / BLOCK_BITS;
    bit %= BLOCK_BITS;
    (block_pos, bit / 64, bit % 64)
}

impl Bitmap {
    pub fn new(start_block_id: usize, blocks: usize) -> Self {
        Self {
            start_block_id,
            blocks,
        }
    }
    //遍历区域中的每个块，再在每个块中以bit组（每组 64 bits）为单位进行遍历，找到一个尚未
    //被全部分配出去的组，最后在里面分配一个bit。它将会返回分配的bit所在的位置，等同于索引
    //节点/数据块的编号
    pub fn alloc(&self, block_device: &Arc<dyn BlockDevice>) -> Option<usize> {
        //枚举区域中的每个块（编号为 block_id ），在循环内部我们需要读写这个块，
        //在块内尝试找到一个空闲的bit并置 1 
        for block_id in 0..self.blocks {
            //获取块缓存
            let pos = get_block_cache(
                block_id + self.start_block_id as usize,
                Arc::clone(block_device),
            )
            .lock()//获取块缓存的互斥锁从而可以对块缓存进行访问
            //传入的偏移量offset为0，这是因为整个块上只有一个BitmapBlock，它的大小恰好为512字节。
            //因此我们需要从块的开头开始才能访问到完整的 BitmapBlock
            //modify指从缓冲区偏移量为 0 的位置开始将一段连续的数据（数据的长度随具体类型而定）
            //解析为一个 BitmapBlock 并要对该数据结构进行修改
            .modify(0, |bitmap_block: &mut BitmapBlock| {
                //遍历每64bits构成的组（一个u64 ），如果它并没有达到u64::MAX （即 2的64次方-1），则通过
                //u64::trailing_ones 找到最低的一个 0 并置为 1 。如果能够找到的话，
                //bit组的编号将保存在变量 bits64_pos 中，而分配的bit在组内的位置将保存在变量 inner_pos 中
                if let Some((bits64_pos, inner_pos)) = bitmap_block
                    .iter()
                    .enumerate()
                    .find(|(_, bits64)| **bits64 != u64::MAX)
                    .map(|(bits64_pos, bits64)| (bits64_pos, bits64.trailing_ones() as usize))
                {
                    // modify cache
                    bitmap_block[bits64_pos] |= 1u64 << inner_pos;
                    Some(block_id * BLOCK_BITS + bits64_pos * 64 + inner_pos as usize)
                } else {
                    None
                }
            });
            //一旦在某个块中找到一个空闲的bit并成功分配，就不再考虑后续的块
            if pos.is_some() {
                return pos;
            }
        }
        None
    }
    //调用 decomposition 函数将bit编号 bit 分解为区域中的块编号 block_pos 、块内的
    //组编号 bits64_pos 以及组内编号 inner_pos 的三元组
    pub fn dealloc(&self, block_device: &Arc<dyn BlockDevice>, bit: usize) {
        let (block_pos, bits64_pos, inner_pos) = decomposition(bit);
        get_block_cache(block_pos + self.start_block_id, Arc::clone(block_device))
            .lock()
            .modify(0, |bitmap_block: &mut BitmapBlock| {
                assert!(bitmap_block[bits64_pos] & (1u64 << inner_pos) > 0);
                bitmap_block[bits64_pos] -= 1u64 << inner_pos;
            });
    }

    pub fn maximum(&self) -> usize {
        self.blocks * BLOCK_BITS
    }
}
