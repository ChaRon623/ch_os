//! Low level access to RISC-V processors
//!
//! # Minimum Supported Rust Version (MSRV)
//!
//! This crate is guaranteed to compile on stable Rust 1.42 and up. It *might*
//! compile with older versions but that may change in any new patch release.
//!
//! # Features
//!
//! This crate provides:
//!
//! - Access to core registers like `mstatus` or `mcause`.
//! - Interrupt manipulation mechanisms.
//! - Wrappers around assembly instructions like `WFI`.

#![no_std]
#![cfg_attr(feature = "inline-asm", feature(asm))]
#![cfg_attr(feature = "inline-asm", feature(asm_const))]

extern crate bare_metal;
extern crate bit_field;
extern crate embedded_hal;

pub mod asm;
pub mod delay;
pub mod interrupt;
pub mod register;

#[macro_use]
mod macros;
