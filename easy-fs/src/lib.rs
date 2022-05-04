#![no_std]

extern crate alloc;

mod bitmap;
mod block_cache;
mod block_dev;
mod efs;
mod layout;
mod vfs;

//512 字节的数组，表示位于内存中的缓冲区
pub const BLOCK_SZ: usize = 512;
use bitmap::Bitmap;
use block_cache::{block_cache_sync_all, get_block_cache};
pub use block_dev::BlockDevice;
pub use efs::EasyFileSystem;
use layout::*;
pub use vfs::Inode;
