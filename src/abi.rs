extern crate alloc;

use alloc::{format, string::String, vec::Vec};

pub const KERNEL_ABI_VERSION: u64 = 3;
pub const KERNEL_ABI_NAME: &str = "coolOS-userspace-abi";

pub fn version() -> u64 {
    KERNEL_ABI_VERSION
}

pub fn lines() -> Vec<String> {
    alloc::vec![
        format!("{} version {}", KERNEL_ABI_NAME, KERNEL_ABI_VERSION),
        String::from("syscalls: exit/write/yield/getpid/mmap/open/read/close/exec"),
        String::from("syscalls: pipe/dup/shmem/waitpid/spawn/sleep_ms/abi/dns/http"),
    ]
}
