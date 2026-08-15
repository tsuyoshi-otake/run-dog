#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(unused_must_use)]

//! RunDog's deterministic, platform-independent application core.
//!
//! The Win32 adapter is intentionally kept at the edge of the crate. Tests
//! exercise this module with fakes and never invoke the real registry, tray,
//! process launcher, clock, or CPU APIs.

pub mod application;
pub mod core;
pub mod update;

#[cfg(windows)]
pub mod windows;
