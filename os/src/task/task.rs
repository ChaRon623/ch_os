use super::TaskContext;
use super::{pid_alloc, KernelStack, PidHandle};
use crate::config::TRAP_CONTEXT;
use crate::memory::{MemorySet, PhysPageNum, VirtAddr, KERNEL_SPACE};
use crate::sync::UPSafeCell;
use crate::trap::{trap_handler, TrapContext};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::cell::RefMut;

#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    Ready,
    Running,
    Zombie,
}

pub struct TaskControlBlock {
	// immutable
    //初始化之后就不再变化的元数据：直接放在任务控制块中
	pub pid: PidHandle,
	pub kernel_stack: KernelStack,
	// mutable
	inner: UPSafeCell<TaskControlBlockInner>,
}

//在运行过程中可能发生变化的元数据
pub struct TaskControlBlockInner {
    //应用地址空间中的Trap上下文被放在的物理页帧的物理页号
    pub trap_cx_ppn: PhysPageNum,
    //应用数据仅有可能出现在应用地址空间低于base_size字节的区域中。
    //借助它我们可以清楚的知道应用有多少数据驻留在内存中
    pub base_size: usize,
    pub task_cx: TaskContext,//将暂停的任务的任务上下文保存在任务控制块中
    pub task_status: TaskStatus,//当前进程的执行状态
    pub memory_set: MemorySet,//应用地址空间
    pub parent: Option<Weak<TaskControlBlock>>,//指向当前进程的父进程
    //将当前进程的所有子进程的任务控制块以Arc智能指针的形式保存在一个向量中
    pub children: Vec<Arc<TaskControlBlock>>,
    //当进程调用 exit 系统调用主动退出或者执行出错由内核终止的时候，它的退出码exit_code
    //会被内核保存在它的任务控制块中，并等待它的父进程通过 waitpid 回收它的资源的同时也收集
    //它的 PID 以及退出码
    pub exit_code: i32,
}

impl TaskControlBlockInner {
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    fn get_status(&self) -> TaskStatus {
        self.task_status
    }
    pub fn is_zombie(&self) -> bool {
        self.get_status() == TaskStatus::Zombie
    }
}

//在内核中手动生成的进程只有初始进程initproc，余下所有的进程都是它直接或间接fork出来的。
//当一个子进程被fork出来之后，它可以调用exec系统调用来加载并执行另一个可执行文件。
impl TaskControlBlock {
    pub fn inner_exclusive_access(&self) -> RefMut<'_, TaskControlBlockInner> {
        self.inner.exclusive_access()
    }
    //创建一个新的进程，目前仅用于内核中手动创建唯一一个初始进程 initproc
    pub fn new(elf_data: &[u8]) -> Self {
		//解析应用的 ELF 执行文件得到应用地址空间 memory_set ，用户栈在应用地址空间中的位置 user_sp 以及应用的入口点 entry_point
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
		//查页表找到位于应用地址空间中新创建的Trap 上下文被实际放在哪个物理页帧上，用来做后续的初始化
        let trap_cx_ppn = memory_set
			.translate(VirtAddr::from(TRAP_CONTEXT).into())
			.unwrap()
			.ppn();
		//为该进程分配PID以及内核栈，并记录下内核栈在内核地址空间的位置kernel_stack_top
        let pid_handle = pid_alloc();
	    let kernel_stack = KernelStack::new(&pid_handle);
        let kernel_stack_top = kernel_stack.get_top();
		//在该进程的内核栈上压入初始化的任务上下文，使得第一次任务切换到它的时候可以跳转到trap_return并进入用户态开始执行
        //let task_cx_ptr = kernel_stack.push_on_top(TaskContext::goto_trap_return());
		//整合之前的部分信息创建进程控制块
        let task_control_block = Self {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: user_sp,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                })
            },
        };
            
		//task_control_block.acquire_inner_lock().init_rlimits();
		let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
		//初始化位于该进程应用地址空间中的 Trap 上下文，使得第一次进入用户态的时候
        //能正确跳转到应用入口点并设置好用户栈，同时也保证在 Trap 的时候用户态能正确进入内核态。
        *trap_cx = TrapContext::app_init_context(
			entry_point,
			user_sp,
			KERNEL_SPACE.exclusive_access().token(),
			kernel_stack_top,
			trap_handler as usize,
		);
		task_control_block
	}

    //当前进程 fork 出来一个与之几乎相同的子进程
    pub fn fork(self: &Arc<TaskControlBlock>) -> Arc<TaskControlBlock> {
		// ---- access parent PCB exclusively
        let mut parent_inner = self.inner_exclusive_access();
        // copy user space(include trap context)
        let memory_set = MemorySet::from_existed_user(&parent_inner.memory_set);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = KernelStack::new(&pid_handle);
        let kernel_stack_top = kernel_stack.get_top();
        let task_control_block = Arc::new(TaskControlBlock {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: parent_inner.base_size,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                })
            },
        });
        // add child
        parent_inner.children.push(task_control_block.clone());
        // modify kernel_sp in trap_cx
        // **** access children PCB exclusively
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        trap_cx.kernel_sp = kernel_stack_top;
        // return
        task_control_block
        // ---- release parent PCB automatically
        // **** release children PCB automatically
	}

    pub fn exec(&self, elf_data: &[u8]) {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();

        // **** access inner exclusively
        let mut inner = self.inner_exclusive_access();
        // substitute memory_set
        inner.memory_set = memory_set;
        // update trap_cx ppn
        inner.trap_cx_ppn = trap_cx_ppn;
        // initialize trap_cx
        let trap_cx = inner.get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            self.kernel_stack.get_top(),
            trap_handler as usize,
        );
        // **** release inner automatically
    }

    //以usize的形式返回当前进程的进程标识符
    pub fn getpid(&self) -> usize {
        self.pid.0
    }
}
