//! `CLRHOST.ELF` -- Buitenzorg's managed C# runtime: **RustCLR** (the runtime
//! of RustNetRuntime) as an ordinary ring-3 program. Arguments:
//! `<path to .dll> [program arguments...]`, e.g. `/disk/HELLOCLR.DLL`.
//!
//! It loads a Roslyn-compiled assembly (`dotnet build`, unmodified), lets the
//! RustCLR loader resolve it against the framework contract implemented by
//! RustBCL, and interprets its IL with a tracing garbage collector -- the
//! *managed* path for C#, next to the *native* bflat/NativeAOT path of the
//! other apps. The same source builds for x86-64, ARM64 and RISC-V: the
//! interpreter does not care which CPU runs it.
//!
//! The host supplies what RustCLR asks of an embedder (`Host`): console
//! output (DEBUG_WRITE), the clocks (TICKS, CLOCK_RTC), the arguments
//! (PROC_ARGS); plus the Rust heap (`heap.rs`, MMAP + MPROTECT), a big
//! stack (the interpreter recurses per managed call), and real threads
//! (`threads.rs`: RustCLR's `threads` feature over THREAD_CREATE, futexes
//! with timeouts and CLOCK_NANOS -- `Thread`, `Task.Run`, `Parallel`,
//! `Monitor.Wait`, a GC that stops every thread at safe points).

#![no_std]
#![no_main]

extern crate alloc;

mod bzapi;
mod heap;
mod sys;
mod threads;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::arch::global_asm;

use rustclr_core::host::Host;
use rustclr_core::{ClrExceptionKind, ClrObject, ExecResult, ExecutionError, Interpreter, Value};
use rustclr_metadata::{Image, TableId};

/// Stack for the interpreter (the loader's stack is 128 KiB).
const STACK: u64 = 16 << 20;

// Entry: the kernel's stack, argument in the first argument register.
#[cfg(target_arch = "x86_64")]
global_asm!(".section .text._start, \"ax\"", ".global _start", "_start:", "and rsp, -16", "call bz_clr_entry", "ud2");
#[cfg(target_arch = "aarch64")]
global_asm!(".section .text._start, \"ax\"", ".global _start", "_start:", "bl bz_clr_entry", "brk #0");
#[cfg(target_arch = "riscv64")]
global_asm!(".section .text._start, \"ax\"", ".global _start", "_start:", "call bz_clr_entry", "unimp");

#[no_mangle]
extern "C" fn bz_clr_entry() -> ! {
    if !heap::init() {
        sys::print("[clr] cannot reserve the heap\n");
        sys::exit(0xC1);
    }
    // A fresh stack, committed up front (the kernel does not grow stacks).
    let base = sys::syscall(sys::MMAP, STACK, sys::PROT_RW, 0);
    if sys::is_err(base) || base == 0 {
        sys::print("[clr] cannot map the interpreter stack\n");
        sys::exit(0xC2);
    }
    sys::run_on_stack((base + STACK) & !0xF, clr_main)
}

/// The runtime's view of Buitenzorg.
struct BzHost {
    args: Vec<String>,
    t0: u64,
}

impl Host for BzHost {
    fn write_out(&mut self, text: &str) {
        sys::print(text);
    }
    fn write_err(&mut self, text: &str) {
        sys::print(text);
    }
    fn read_line(&mut self) -> Option<String> {
        None
    }
    fn monotonic_millis(&mut self) -> u64 {
        sys::syscall(threads::CLOCK_NANOS, 0, 0, 0) / 1_000_000 - self.t0
    }
    fn monotonic_nanos(&mut self) -> u64 {
        sys::syscall(threads::CLOCK_NANOS, 0, 0, 0)
    }
    fn wall_clock_millis(&mut self) -> i64 {
        sys::unix_millis()
    }
    fn args(&self) -> &[String] {
        &self.args
    }
}

extern "C" fn clr_main() -> ! {
    let line = sys::args();
    let mut words = line.split_whitespace();
    let Some(path) = words.next().map(String::from) else {
        sys::print("usage: clrhost <assembly.dll> [args...]\n");
        sys::exit(2);
    };
    let prog_args: Vec<String> = words.map(String::from).collect();

    let Some(bytes) = sys::read_file(&path) else {
        sys::print(&format!("[clr] {}: file not found\n", path));
        sys::exit(3);
    };
    threads::install();
    let t0 = sys::syscall(threads::CLOCK_NANOS, 0, 0, 0) / 1_000_000;
    let image = match Image::from_bytes(bytes) {
        Ok(i) => i,
        Err(e) => {
            sys::print(&format!("[clr] {}: not a .NET assembly ({})\n", path, e));
            sys::exit(4);
        }
    };
    let name = image.assembly_name().to_string();

    let mut interp = Interpreter::with_host(Box::new(BzHost { args: prog_args, t0 }));
    // Threads this program starts get the same console.
    interp.host_factory = Some(alloc::sync::Arc::new(move || Box::new(BzHost { args: Vec::new(), t0 }) as Box<dyn Host>));
    rustclr_bcl::install(&mut interp);
    // RustBCL's `Assembly.Load` needs `std` (a filesystem to search); this
    // host has one -- the VFS -- so it supplies its own.
    for member in ["Load(string)", "Load/1", "LoadFrom(string)"] {
        interp.register_native(format!("System.Reflection.Assembly::{}", member), assembly_load);
    }
    // The OS API for managed apps (Buitenzorg.App's [DllImport("buitenzorg")]).
    bzapi::install(&mut interp);
    // Libraries the program references, from /disk, before the program: a
    // TypeRef binds only to a type the loader already has.
    let dir = path.rsplit_once('/').map(|(d, _)| String::from(d)).unwrap_or_else(|| String::from("/disk"));
    if let Err(e) = load_references(&mut interp, &image, &dir, 0) {
        sys::print(&format!("[clr] {}: {}
", name, e));
        sys::exit(6);
    }
    let id = match interp.loader.load_image(image) {
        Ok(id) => id,
        Err(e) => {
            sys::print(&format!("[clr] {}: load failed: {}\n", name, e));
            sys::exit(5);
        }
    };
    sys::print(&format!(
        "[clr] RustCLR managed runtime: {} loaded ({} types, {} methods, {} native bindings)\n",
        name,
        interp.loader.registry.type_count(),
        interp.loader.registry.method_count(),
        interp.native_count()
    ));

    let outcome = interp.run_entry_point(id);
    let code = match &outcome {
        Ok(c) => *c as u64 as u32 as u64,
        Err(e) => {
            sys::print(&format!("[clr] {}: unhandled: {}\n", name, e));
            0xE0
        }
    };
    let gc = interp.heap.stats();
    let (committed, used) = heap::HEAP.stats();
    sys::print(&format!(
        "[clr] {} exited with {}: {} IL instructions, {} calls, {} managed allocations, {} collections ({} objects freed), peak {} KiB live; Rust heap {} KiB in use of {} MiB committed; {} ms\n",
        name,
        code,
        interp.stats.instructions_executed,
        interp.stats.calls,
        gc.total_allocations,
        gc.collections,
        gc.total_objects_freed,
        gc.peak_live_bytes / 1024,
        used / 1024,
        committed >> 20,
        sys::syscall(threads::CLOCK_NANOS, 0, 0, 0) / 1_000_000 - t0
    ));
    sys::exit(code)
}

/// The 8.3 file a referenced assembly lives in on /disk.
fn library_file(dir: &str, name: &str) -> String {
    match name {
        "Buitenzorg.App" => format!("{}/BZAPP.DLL", dir),
        _ => format!("{}/{}.DLL", dir, name.chars().take(8).collect::<String>().to_uppercase()),
    }
}

/// Load every assembly `image` references that is not part of the framework
/// (System.*, Microsoft.*, netstandard, mscorlib -- RustBCL implements those)
/// and is not loaded yet, depth-first, so a library's own references come
/// before it.
fn load_references(interp: &mut Interpreter, image: &Image, dir: &str, depth: u32) -> Result<(), String> {
    if depth > 8 {
        return Err(String::from("assembly references nest too deep"));
    }
    let md = image.metadata();
    let mut names = Vec::new();
    for row in 1..=md.row_count(TableId::AssemblyRef) {
        if let Ok(r) = md.assembly_ref(row) {
            names.push(String::from(r.name));
        }
    }
    for name in names {
        let framework = name.starts_with("System") || name.starts_with("Microsoft") || name == "netstandard" || name == "mscorlib";
        if framework || interp.loader.find_assembly(&name).is_some() {
            continue;
        }
        let file = library_file(dir, &name);
        let Some(bytes) = sys::read_file(&file) else {
            return Err(format!("references '{}', not found as {}", name, file));
        };
        let lib = Image::from_bytes(bytes).map_err(|e| format!("{}: not a .NET assembly ({})", file, e))?;
        load_references(interp, &lib, dir, depth + 1)?;
        interp.loader.load_image(lib).map_err(|e| format!("{}: load failed: {}", file, e))?;
    }
    Ok(())
}

/// `Assembly.Load(name)` / `Assembly.LoadFrom(path)`: an assembly already
/// loaded is returned as is; otherwise `/disk/<NAME>.DLL` (8.3: the first eight
/// characters of the simple name) or the given path is read and loaded.
fn assembly_load(i: &mut Interpreter, a: &[Value]) -> ExecResult<Option<Value>> {
    let arg = match a.first() {
        Some(Value::Obj(h)) => i.string_value(*h).unwrap_or_default(),
        _ => String::new(),
    };
    // "Foo, Version=..., Culture=..." or ".../Foo.dll" -> "Foo".
    let simple = arg.split(',').next().unwrap_or("").trim();
    let simple = simple.rsplit(['/', '\\']).next().unwrap_or(simple);
    let simple = simple.strip_suffix(".dll").or_else(|| simple.strip_suffix(".DLL")).unwrap_or(simple);
    let id = match i.loader.find_assembly(simple) {
        Some(id) => id,
        None => {
            let short: String = simple.chars().take(8).collect::<String>().to_uppercase();
            let candidates = [arg.clone(), format!("/disk/{}.DLL", short)];
            let bytes = candidates.iter().filter(|p| p.starts_with('/')).find_map(|p| sys::read_file(p));
            let Some(bytes) = bytes else {
                return Err(ExecutionError::exception(
                    ClrExceptionKind::InvalidOperation,
                    format!("Could not load file or assembly '{}': not on /disk", arg),
                ));
            };
            let image = Image::from_bytes(bytes).map_err(|e| {
                ExecutionError::exception(ClrExceptionKind::InvalidOperation, format!("'{}' is not a .NET assembly: {}", arg, e))
            })?;
            i.loader.load_image(image)?
        }
    };
    let Some(ty) = i.loader.registry.find_type_by_name("System.Reflection.Assembly") else {
        return Ok(Some(Value::Null));
    };
    let h = i.alloc_object(ty);
    i.heap.with_mut::<ClrObject, _>(h, |o| {
        if let Some(f) = o.fields.get_mut(0) {
            *f = Value::I32(id.index() as i32);
        }
    });
    Ok(Some(Value::Obj(h)))
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    sys::print(&format!("[clr] PANIC: {}\n", info));
    sys::exit(0xBAD)
}
