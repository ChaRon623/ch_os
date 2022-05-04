use super::{BlockDevice, BLOCK_SZ};
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;
use spin::Mutex;

pub struct BlockCache {
    cache: [u8; BLOCK_SZ],
    block_id: usize,//这个块缓存来自于磁盘中的块的编号
    block_device: Arc<dyn BlockDevice>,//底层块设备的引用，可通过它进行块读写
    modified: bool,//记录这个块从磁盘载入内存缓存之后，它有没有被修改过
}

impl BlockCache {
    /// Load a new BlockCache from disk.
    pub fn new(block_id: usize, block_device: Arc<dyn BlockDevice>) -> Self {
        let mut cache = [0u8; BLOCK_SZ];
        block_device.read_block(block_id, &mut cache);
        Self {
            cache,
            block_id,
            block_device,
            modified: false,
        }
    }

    //得到一个 BlockCache 内部的缓冲区中指定偏移量 offset 的字节地址
    fn addr_of_offset(&self, offset: usize) -> usize {
        &self.cache[offset] as *const _ as usize
    }

    //获取缓冲区中的位于偏移量 offset 的一个类型为 T 的磁盘上数据结构的不可变引用
    pub fn get_ref<T>(&self, offset: usize) -> &T
    where
        T: Sized,
    {
        let type_size = core::mem::size_of::<T>();
        assert!(offset + type_size <= BLOCK_SZ);
        let addr = self.addr_of_offset(offset);
        unsafe { &*(addr as *const T) }
    }

    //获取磁盘上数据结构的可变引用，由此可以对数据结构进行修改
    pub fn get_mut<T>(&mut self, offset: usize) -> &mut T
    where
        T: Sized,
    {
        let type_size = core::mem::size_of::<T>();
        assert!(offset + type_size <= BLOCK_SZ);
        self.modified = true;
        let addr = self.addr_of_offset(offset);
        unsafe { &mut *(addr as *mut T) }
    }

    //在 BlockCache 缓冲区偏移量为 offset 的位置获取一个类型为 T 的磁盘上数据结构的
    //不可变/可变引用（分别对应 read/modify ），并让它执行传入的闭包 f 中所定义的操作。
    pub fn read<T, V>(&self, offset: usize, f: impl FnOnce(&T) -> V) -> V {
        f(self.get_ref(offset))
    }

    pub fn modify<T, V>(&mut self, offset: usize, f: impl FnOnce(&mut T) -> V) -> V {
        f(self.get_mut(offset))
    }

    //如果自身确实被修改过的话才会将缓冲区的内容写回磁盘
    pub fn sync(&mut self) {
        if self.modified {
            self.modified = false;
            self.block_device.write_block(self.block_id, &self.cache);
        }
    }
}

impl Drop for BlockCache {
    fn drop(&mut self) {
        self.sync()
    }
}

//内存中同时只能驻留有限个磁盘块的缓冲区
const BLOCK_CACHE_SIZE: usize = 16;

pub struct BlockCacheManager {
    queue: VecDeque<(usize, Arc<Mutex<BlockCache>>)>,
}

//块缓存全局管理器的功能是：当我们要对一个磁盘块进行读写时，首先看它是否已经被载入到内存缓存中
//了，如果已经被载入的话则直接返回，否则需要先读取磁盘块的数据到内存缓存中。此时，如果内存中
//驻留的磁盘块缓冲区的数量已满，则需要遵循某种缓存替换算法将某个块的缓存从内存中移除，再将刚
//刚读到的块数据加入到内存缓存中。我们这里使用一种类 FIFO 的简单缓存替换算法
impl BlockCacheManager {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    //尝试从块缓存管理器中获取一个编号为 block_id 的块的块缓存，
    //如果找不到，会从磁盘读取到内存中，还有可能会发生缓存替换
    pub fn get_block_cache(
        &mut self,
        block_id: usize,
        block_device: Arc<dyn BlockDevice>,
    ) -> Arc<Mutex<BlockCache>> {
        //遍历整个队列试图找到一个编号相同的块缓存，
        //如果找到了，会将块缓存管理器中保存的块缓存的引用复制一份并返回
        if let Some(pair) = self.queue.iter().find(|pair| pair.0 == block_id) {
            Arc::clone(&pair.1)
        } else {
            //将块从磁盘读入内存中的缓冲区
            //判断管理器保存的块缓存数量是否已经达到了上限,若是则执行缓存替换算法，丢掉某个块缓存并空出一个空位
            if self.queue.len() == BLOCK_CACHE_SIZE {
                //这里使用一种类 FIFO 算法：每加入一个块缓存时要从队尾加入；要替换时则从队头弹出。
                //但此时队头对应的块缓存可能仍在使用：判断的标志是其强引用计数 ，即除了块缓存管理器
                //保留的一份副本之外，在外面还有若干份副本正在使用。因此，我们的做法是从队头遍历到队尾
                //找到第一个强引用计数恰好为 1 的块缓存并将其替换出去。
                if let Some((idx, _)) = self
                    .queue
                    .iter()
                    .enumerate()
                    .find(|(_, pair)| Arc::strong_count(&pair.1) == 1)
                {
                    self.queue.drain(idx..=idx);
                } else {
                    panic!("Run out of BlockCache!");
                }
            }
            // 创建一个新的块缓存（会触发 read_block 进行块读取）并加入到队尾，最后返回给请求者
            let block_cache = Arc::new(Mutex::new(BlockCache::new(
                block_id,
                Arc::clone(&block_device),
            )));
            self.queue.push_back((block_id, Arc::clone(&block_cache)));
            block_cache
        }
    }
}

lazy_static! {
    pub static ref BLOCK_CACHE_MANAGER: Mutex<BlockCacheManager> =
        Mutex::new(BlockCacheManager::new());
}

//对于其他模块而言，就可以直接通过 get_block_cache 方法来请求块缓存
pub fn get_block_cache(
    block_id: usize,
    block_device: Arc<dyn BlockDevice>,
) -> Arc<Mutex<BlockCache>> {
    BLOCK_CACHE_MANAGER
        .lock()
        .get_block_cache(block_id, block_device)
}

pub fn block_cache_sync_all() {
    let manager = BLOCK_CACHE_MANAGER.lock();
    for (_, cache) in manager.queue.iter() {
        cache.lock().sync();
    }
}
