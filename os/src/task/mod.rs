mod manager;
mod processor;
mod switch;
mod context;
mod pid;
#[allow(clippy::module_inception)]

mod task;

use crate::fs::{open_file, OpenFlags};
use alloc::sync::Arc;
use lazy_static::*;
pub use manager::{fetch_task,TaskManager};
use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;
pub use manager::add_task;
pub use pid::{pid_alloc, KernelStack, PidHandle,PidAllocator};
pub use processor::{
    current_task, current_trap_cx, current_user_token, run_tasks, schedule, take_current_task,Processor
};

//初始化初始进程的进程控制块 INITPROC
lazy_static! {
    pub static ref INITPROC: Arc<TaskControlBlock> = Arc::new({
        let inode = open_file("initproc", OpenFlags::RDONLY).unwrap();
        let v = inode.read_all();
        TaskControlBlock::new(v.as_slice())
    });
}

///Add init process to the manager
pub fn add_initproc() {
    add_task(INITPROC.clone());
}

pub fn suspend_current_and_run_next() {
    // 取出当前正在执行的任务
    let task = take_current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    let task_cx_ptr = &mut task_inner.task_cx as *mut TaskContext;
    // 修改其进程控制块内的状态
    task_inner.task_status = TaskStatus::Ready;
    drop(task_inner);

    // 将这个任务放入任务管理器的队尾
    add_task(task);
    // 触发调度并切换任务
    schedule(task_cx_ptr);
}

pub fn exit_current_and_run_next(exit_code: i32) {
    //将当前进程控制块从处理器监控 PROCESSOR 中取出而不是得到一份拷贝，这是为了正确维护进程控制块的引用计数
    let task = take_current_task().unwrap();
     // **** access current TCB exclusively
    let mut inner = task.inner_exclusive_access();
     // Change status to Zombie
    inner.task_status = TaskStatus::Zombie;

    // record exit code
    inner.exit_code = exit_code;
    // do not move to its parent but under initproc
  
    // ++++++ access initproc TCB exclusively
    {
        let mut initproc_inner = INITPROC.inner_exclusive_access();
        for child in inner.children.iter() {
            child.inner_exclusive_access().parent = Some(Arc::downgrade(&INITPROC));
            initproc_inner.children.push(child.clone());
        }
    }
    // ++++++ release parent PCB
    
    inner.children.clear();
    // deallocate user space
    inner.memory_set.recycle_data_pages();
    drop(inner);
    // **** release current PCB
    // drop task manually to maintain rc correctly
    drop(task);
    // we do not have to save task context
    
    let mut _unused = TaskContext::zero_init();
    schedule(&mut _unused as *mut _);
}
