#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::{exec, fork, wait, yield_};

#[no_mangle]
fn main() -> i32 {
    //fork返回值为0的分支，表示子进程，此行直接通过exec执行shell程序user_shell，
    //注意我们需要在字符串末尾手动加入\0 ，因为Rust在将这些字符串连接到只读数据段的时候不会插入\0
    if fork() == 0 {
        exec("user_shell\0");
    } else {
        //返回值不为 0 的分支，表示调用 fork 的用户初始程序 initproc 自身。
        loop {
            let mut exit_code: i32 = 0;
            //不断循环调用 wait 来等待那些被移交到它下面的子进程并回收它们占据的资源。
            let pid = wait(&mut exit_code);
            if pid == -1 {
                //yield_ 交出 CPU 资源并在下次轮到它执行的时候再回收看看
                yield_();
                continue;
            }
            println!(
                "[initproc] Released a zombie process, pid={}, exit_code={}",
                pid, exit_code,
            );
        }
    }
    0
}
