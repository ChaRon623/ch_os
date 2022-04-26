use super::__switch;
use super::{fetch_task,TaskStatus};
use super::{TaskContext ,TaskControlBlock};
use crate::trap::TrapContext;
use alloc::sync::Arc;
use lazy_static::*;
use crate::sync::UPSafeCell;
use crate::println;

pub struct Processor {
    //在当前处理器上正在执行的任务
    current: Option<Arc<TaskControlBlock>>,
    //当前处理器上的 idle 控制流的任务上下文
    idle_task_cx: TaskContext,
}

//描述CPU 执行状态 的数据结构
impl Processor {
    pub fn new() -> Self {
        Self {
            current: None,
            idle_task_cx: TaskContext::zero_init(),
        }
    }
    fn get_idle_task_cx_ptr(&mut self) -> *mut TaskContext {
        &mut self.idle_task_cx as *mut _
    }
    //取出当前正在执行的任务
    pub fn take_current(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.current.take()
    }
    //返回当前执行的任务的一份拷贝
    pub fn current(&self) -> Option<Arc<TaskControlBlock>> {
        self.current.as_ref().map(Arc::clone)
    }
}

lazy_static! {
    pub static ref PROCESSOR: UPSafeCell<Processor> = unsafe {
        UPSafeCell::new(Processor::new())
    };
}

//运行在这个CPU核的启动栈上，功能是尝试从任务管理器中选出一个任务来在当前 CPU 核上执行。
//在内核初始化完毕之后，会通过调用run_tasks函数来进入idle控制流
pub fn run_tasks() {
    loop {
        let mut processor = PROCESSOR.exclusive_access();
        //循环调用fetch_task直到顺利从任务管理器中取出一个任务，随后便准备通过任务切换的方式来执行
        if let Some(task) = fetch_task() {
            let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
            //先获取从任务管理器中取出对应的任务控制块，并获取任务块内部的next_task_cx_ptr 
            //作为__switch 的第二个参数，然后修改任务的状态为 Running 。
            let mut task_inner = task.inner_exclusive_access();
            let next_task_cx_ptr = &task_inner.task_cx as *const TaskContext;
            task_inner.task_status = TaskStatus::Running;
            //手动回收对即将执行任务的任务控制块的借用标记，使得后续我们仍可以访问该任务控制块
            drop(task_inner);
            //修改当前 Processor 正在执行的任务为我们取出的任务
            processor.current = Some(task);
            drop(processor);
            unsafe {
                //调用 __switch 来从当前的 idle 控制流切换到接下来要执行的任务
                __switch(idle_task_cx_ptr, next_task_cx_ptr);
            }
        } else {
            println!("no tasks available in run_tasks");
        }
    }
}

pub fn take_current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().take_current()
}

pub fn current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().current()
}

pub fn current_user_token() -> usize {
    let task = current_task().unwrap();
    let token = task.inner_exclusive_access().get_user_token();
    token
}

pub fn current_trap_cx() -> &'static mut TrapContext {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .get_trap_cx()
}

//当一个应用用尽了内核本轮分配给它的时间片或者它主动调用yield系统调用
//交出 CPU 使用权之后，内核会调用 schedule 函数来切换到 idle控制流
//并开启新一轮的任务调度
pub fn schedule(switched_task_cx_ptr: *mut TaskContext) {
    let mut processor = PROCESSOR.exclusive_access();
    let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
    drop(processor);
    unsafe {
        __switch(switched_task_cx_ptr, idle_task_cx_ptr);
    }
}