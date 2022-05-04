use crate::fs::{open_file, OpenFlags};
use crate::memory::{translated_refmut, translated_str};
use crate::task::{
    add_task, current_task, current_user_token, exit_current_and_run_next,
    suspend_current_and_run_next,
};
use crate::timer::get_time_ms;
use alloc::sync::Arc;

//调用 sys_yield 主动交出使用权
pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

//主动退出或者出错由内核终止
pub fn sys_exit(exit_code: i32) -> ! {
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

pub fn sys_get_time() -> isize {
    get_time_ms() as isize
}

pub fn sys_getpid() -> isize {
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    //将子进程的 Trap 上下文中用来存放系统调用返回值的 a0 寄存器修改为 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        let task = current_task().unwrap();
        task.exec(all_data.as_slice());
        0
    } else {
        -1
    }
}

//如果当前的进程不存在一个进程 ID 为 pid（pid==-1 或 pid > 0）的子进程，则返回 -1；如果存在一个进程 ID 为
// pid 的僵尸子进程，则正常回收并返回子进程的 pid，并更新系统调用的退出码参数为 exit_code 。这里还有一个 -2 的
//返回值，它的含义是子进程还没退出，通知用户库 user_lib （是实际发出系统调用的地方），这样用户库看到是 -2 后，
//就进一步调用 sys_yield 系统调用，让当前父进程进入等待状态。
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    let task = current_task().unwrap();
    // find a child process

    let mut inner = task.inner_exclusive_access();
    //判断 sys_waitpid 是否会返回 -1 ，这取决于当前进程是否有一个符合要求的子进程。当传入的 pid 为 -1 的时候，
    //任何一个子进程都算是符合要求；但 pid 不为 -1 的时候，则只有 PID 恰好与 pid 相同的子进程才算符合条件
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    //判断符合要求的子进程中是否有僵尸进程，如果有的话还需要同时找出它在当前进程控制块子进程向量中的下标。如果找不到的话直接返回 -2
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        //将子进程从向量中移除并置于当前上下文中
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        //行确认这是对于该子进程控制块的唯一一次强引用，即它不会出现在某个进程的子进程向量中，更不会出现在处理器
        //监控器或者任务管理器中。当它所在的代码块结束，这次引用变量的生命周期结束，将导致该子进程进程控制块的引用
        //计数变为 0 ，彻底回收掉它占用的所有资源，包括：内核栈和它的 PID 还有它的应用地址空间存放页表的那些物理页帧等等。
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        //写入到当前进程的应用地址空间中。由于应用传递给内核的仅仅是一个指向应用地址空间中保存子进程返回值的内存区域的指针，
        //我们还需要在 translated_refmut 中手动查页表找到应该写入到物理内存中的哪个位置，这样才能把子进程的退出码 exit_code 返回给父进程。
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}