use core::fmt::{self, Write};
use super::{read, write};
const STDIN: usize = 0;
const STDOUT: usize = 1;
struct Stdout;

impl Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        write(STDOUT, s.as_bytes());
        Ok(())
    }
}

pub fn print(args: fmt::Arguments) {
    Stdout.write_fmt(args).unwrap();
}

#[macro_export]
macro_rules! print {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!($fmt $(, $($arg)+)?));
    }
}

#[macro_export]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!(concat!($fmt, "\n") $(, $($arg)+)?));
    }
}

//在用户库中将read进一步封装成每次能够从标准输入中获取一个字符的getchar函数
pub fn getchar() -> u8 {
    let mut c = [0u8; 1];//每次临时声明一个长度为 1 的缓冲区
    read(STDIN, &mut c);
    c[0]
}