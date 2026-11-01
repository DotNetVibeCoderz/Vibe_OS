//! The first RISC-V ELF program: greet, check the ABI version, exit 0.
#![no_std]
#![no_main]
mod rt;

fn main(_arg: u64) -> u64 {
    rt::print("Hello from Rust on RISC-V (U-mode ELF)!\n");
    let abi = rt::syscall(rt::ABI_VERSION, 0, 0, 0);
    rt::print("hello: ABI version ");
    rt::print_u64(abi);
    rt::print("\n");
    if abi == 0 || abi > 0xFFFF { 1 } else { 0 }
}
