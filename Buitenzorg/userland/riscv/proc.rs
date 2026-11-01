//! Processes and files through the shared syscalls: write a file on /disk
//! (FS_WRITE) and read it back (FS_READ), then start `/disk/HELLO.ELF` as a
//! child process with arguments (PROC_SPAWN) and wait for its exit code
//! (PROC_WAIT).
#![no_std]
#![no_main]
mod rt;
use rt::*;

const FS_READ: u64 = 30;
const FS_WRITE: u64 = 32;
const PROC_SPAWN: u64 = 40;
const PROC_WAIT: u64 = 41;

fn main(_arg: u64) -> u64 {
    let path = b"/disk/RVPROC.TXT\0";
    let data = b"ditulis oleh proses RISC-V lewat FS_WRITE\n";
    let wrote = syscall(FS_WRITE, path.as_ptr() as u64, data.as_ptr() as u64, data.len() as u64);
    let mut back = [0u8; 64];
    let read = syscall(FS_READ, path.as_ptr() as u64, back.as_mut_ptr() as u64, back.len() as u64);
    let file_ok = wrote == data.len() as u64 && read == data.len() as u64 && &back[..data.len()] == data;
    print(if file_ok { "proc: /disk/RVPROC.TXT written + read back\n" } else { "proc: file round trip FAILED\n" });

    let child = b"/disk/HELLO.ELF\0";
    let args = b"dari-proc\0";
    let pid = syscall(PROC_SPAWN, child.as_ptr() as u64, args.as_ptr() as u64, 0);
    if pid == 0 || pid > 1 << 32 {
        print("proc: PROC_SPAWN failed\n");
        return 1;
    }
    let code = syscall(PROC_WAIT, pid, 0, 0);
    print("proc: child pid ");
    print_u64(pid);
    print(" exited with ");
    print_u64(code);
    print("\n");
    if file_ok && code == 0 { 0 } else { 1 }
}
