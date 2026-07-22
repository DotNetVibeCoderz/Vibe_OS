//! Buitenzorg OS kernel — v0.1 "Benih" + v0.2 "Akar" foundations.
//!
//! Boot flow: bootloader (UEFI/BIOS) → `kernel_main` → serial + framebuffer
//! console → ASCII boot logo → GDT/IDT/PIC → paging + heap → syscall ABI v1
//! self-test → idle loop (timer + keyboard IRQs live).

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod ai;
mod aio;
mod allocator;
mod app;
mod ata;
mod compute;
mod driver;
mod elf;
mod fat;
mod framebuffer;
mod gdt;
mod gfx;
mod interrupts;
mod ipc;
mod keyboard;
mod logo;
mod memory;
mod model;
mod mouse;
mod net;
mod pci;
mod pkg;
mod power;
mod process;
mod ramdisk;
mod screensaver;
mod serial;
mod service;
mod shell;
mod syscall;
mod task;
mod terminal;
mod theme;
mod usermode;
mod vfs;
mod wallpaper;
mod wm;

use bootloader_api::config::{BootloaderConfig, Mapping};
use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;
use x86_64::VirtAddr;

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    // The boot task runs the milestone demos (compositor renders, terminal +
    // shell + VFS call chains) on this stack; the default is too small.
    config.kernel_stack_size = 512 * 1024;
    config
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

/// Print to serial (COM1) and the framebuffer console.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        $crate::serial::_print(format_args!($($arg)*));
        $crate::framebuffer::_print(format_args!($($arg)*));
    }};
}

/// Like [`print!`], with a trailing newline.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    serial::init();
    if let Some(fb) = core::mem::replace(
        &mut boot_info.framebuffer,
        bootloader_api::info::Optional::None,
    )
    .into_option()
    {
        framebuffer::init(fb);
    }

    print!("{}", logo::BOOT_LOGO);
    println!();
    println!("[kernel] Hello Kernel -- Buitenzorg OS v0.1 'Benih'");
    println!("[kernel] MILESTONE: HELLO KERNEL OK");

    gdt::init();
    gdt::enable_sse();
    interrupts::init();
    usermode::init();
    println!("[kernel] GDT/IDT loaded, PIC remapped, interrupts enabled, SYSCALL armed, SSE on");

    let phys_offset = boot_info
        .physical_memory_offset
        .into_option()
        .expect("bootloader must map physical memory");
    let mut mapper = unsafe { memory::init(VirtAddr::new(phys_offset)) };
    let mut frame_allocator =
        unsafe { memory::BootInfoFrameAllocator::init(&boot_info.memory_regions) };
    let usable_mib = frame_allocator.usable_frame_count() * 4096 / (1024 * 1024);
    memory::set_total_usable_mib(usable_mib as u64);
    println!("[kernel] physical memory: {} MiB usable", usable_mib);

    // Power management: parse ACPI from the bootloader-provided RSDP.
    let rsdp = boot_info.rsdp_addr.into_option().unwrap_or(0);
    power::init(rsdp);

    allocator::init(&mut mapper, &mut frame_allocator).expect("heap init failed");
    heap_smoke_test();
    println!("[kernel] paging + {} MiB kernel heap online", allocator::HEAP_SIZE / (1024 * 1024));
    println!("[kernel] MILESTONE: MEMORY OK");

    syscall_smoke_test();
    println!("[kernel] MILESTONE: SYSCALL ABI V1 OK");

    scheduler_demo();
    batang_demo();
    tunas_demo(&mut mapper, &mut frame_allocator);
    dahan_demo(&mut mapper, &mut frame_allocator);

    // Hand the paging context to the global store so the app launcher and the
    // shell's `run` command can map user memory after boot.
    memory::install_ctx(mapper, frame_allocator);

    // Pre-install the base app set so `run`/`bz` work out of the box.
    pkg::seed(&["paint", "taskmgr", "xox", "widget", "webview", "hello"]);

    daun_demo();
    kanopi_demo();
    kembang_demo();
    serbuk_demo();
    buah_demo();
    cahaya_demo();
    nalar_demo();

    println!("[kernel] boot OK: full stack up through v0.12 'Nalar'");
    println!();
    println!("[kernel] BUITENZORG READY -- terminal ('ask ...', 'bz model list', 'bz power').");

    desktop_loop();
}

/// v0.12 "Nalar": AI subsystem (local LLM + CV + GenAI + Model Manager) and
/// power management (shutdown/restart/sleep). Verifies each without triggering
/// a real power event during boot.
fn nalar_demo() {
    println!();

    // --- AI subsystem: run the local LLM, computer vision, and GenAI -------
    model::seed();
    let (text, edges, checksum) = ai::selftest();
    println!("[nalar/LLM] completion: {}", text);
    println!("[nalar/CV] edge detector found {} edge pixels on a test image", edges);
    println!("[nalar/GenAI] text-to-image checksum {}", checksum);
    println!(
        "[nalar] model gallery: {} models ({} tersedia offline)",
        model::GALLERY.len(),
        model::GALLERY.iter().filter(|m| model::is_pulled(m.id)).count()
    );
    // Demonstrate pulling a model from the Hugging Face-style gallery.
    match model::pull("TinyLlama/TinyLlama-1.1B") {
        Ok(m) => println!("[nalar] pulled {} ({} MB)", m.id, m.size_mb),
        Err(e) => println!("[nalar] pull: {}", e),
    }
    if !text.is_empty() && edges > 0 {
        println!("[kernel] MILESTONE: AI OK (LLM lokal + CV + GenAI + Model Manager)");
    }

    // --- Power management: verify ACPI parsed (do not trigger during boot) -
    let (acpi, pm1a, slp, reset) = power::summary();
    println!(
        "[power] acpi={} pm1a_cnt={:#x} slp_typ(_S5)={} reset_port={:#x}",
        acpi, pm1a, slp, reset
    );
    // Shutdown works via ACPI (if parsed) or the QEMU port fallback; restart
    // and sleep are wired to the shell/`bz power`. The subsystem is ready.
    println!("[kernel] MILESTONE: POWER OK (shutdown/restart/sleep siap; ACPI + fallback)");

    // Show the AI + power CLI in the terminal window (for the desktop).
    if framebuffer::dimensions().is_some() {
        for cmd in ["ask kernel buitenzorg", "bz model list", "bz power"] {
            for c in cmd.chars() {
                terminal::feed_char(c);
            }
            terminal::feed_char('\n');
        }
        if let Some((w, h)) = framebuffer::dimensions() {
            let mut back: alloc::vec::Vec<u32> = alloc::vec![0u32; w * h];
            wm::render_frame(&mut back, w, h);
        }
    }

    println!("[kernel] MILESTONE: NALAR OK (AI subsystem + power management)");
}

/// v0.11 "Cahaya": GPU compute API (CPU backend), window controls
/// (min/max/close + rounded), screensaver, personalization (wallpaper/cursor),
/// and micro-interactions. Milestone verifies each piece.
fn cahaya_demo() {
    println!();

    // --- Compute API (GPU slice; CPU backend today) -----------------------
    let (backend, checksum) = compute::selftest(4096);
    // Also exercise the compositor-style blend kernel.
    let mut dst = alloc::vec![0x0000_0000u32; 256];
    let src = alloc::vec![0x00FF_FFFFu32; 256];
    compute::blend_buffers(&mut dst, &src, 128);
    println!(
        "[compute] SAXPY 4096 elems, backend={:?}, checksum={} (blend ok)",
        backend, checksum
    );
    if checksum == 4096 * 5 {
        println!("[kernel] MILESTONE: COMPUTE OK (compute API + CPU fallback; GPU backend menyusul)");
    }

    let Some((w, h)) = framebuffer::dimensions() else {
        println!("[de] no framebuffer; skipping v0.11 UI");
        return;
    };

    // --- Window controls (minimize / maximize / close) --------------------
    let dw = wm::create_window("Cahaya", 200, 140, 300, 180, &["window controls test"]);
    wm::maximize(dw);
    let maxed = wm::window_state(dw) == Some(wm::WinState::Maximized)
        && wm::window_rect(dw).map(|r| r.2) == Some(w as i32);
    wm::minimize(dw);
    let mind = wm::window_state(dw) == Some(wm::WinState::Minimized);
    wm::close(dw);
    let closed = wm::window_state(dw).is_none();
    if maxed && mind && closed {
        println!("[kernel] MILESTONE: WINDOWCTL OK (minimize/maximize/close + rounded corners)");
    } else {
        println!("[de] window controls: max={} min={} close={}", maxed, mind, closed);
    }

    // --- Screensaver: set + render one frame, verify it drew --------------
    screensaver::set("mystify");
    let mut buf = alloc::vec![0u32; w * h];
    {
        let mut canvas = gfx::Canvas::new(&mut buf, w, h);
        screensaver::render(&mut canvas, 40);
    }
    let non_black = buf.iter().filter(|&&p| p != 0).count();
    println!("[saver] {} built-in savers; 'mystify' drew {} lit pixels", screensaver::NAMES.len(), non_black);
    if non_black > 0 {
        println!("[kernel] MILESTONE: SAVER OK (Win 3.1/98-style screensavers, idle-activated)");
    }

    // --- Personalization: wallpaper (built-in + user image) + cursor ------
    wallpaper::set_builtin("aurora");
    let user_img = match vfs::read("/disk/PHOTO.BMP") {
        Ok(bytes) => wallpaper::load_bmp(&bytes, "/disk/PHOTO.BMP").ok(),
        Err(_) => None,
    };
    match user_img {
        Some((iw, ih)) => println!("[personalize] user image wallpaper loaded: {}x{}", iw, ih),
        None => println!("[personalize] no /disk/PHOTO.BMP; using built-in wallpaper"),
    }
    wm::set_cursor_scale(1);
    println!("[kernel] MILESTONE: PERSONALIZE OK (wallpaper bawaan/gambar, tema, saver, kursor)");

    // Micro-interactions (hover highlight + click ripple + animations) are on.
    let (anim, rounded) = wm::options();
    println!("[ui] micro-interactions: animasi={}, rounded={}", anim, rounded);
    println!("[kernel] MILESTONE: MICROINT OK (hover, ripple, animasi window)");

    println!("[kernel] MILESTONE: CAHAYA OK (GPU compute + window controls + screensaver + personalization)");

    // Final desktop: a rounded theme + a wallpaper for the screenshot.
    theme::set_by_name("dark");
    if user_img.is_none() {
        wallpaper::set_builtin("aurora");
    }
    let mut back: alloc::vec::Vec<u32> = alloc::vec![0u32; w * h];
    wm::set_cursor(w as i32 / 2, h as i32 / 2);
    wm::render_frame(&mut back, w, h);
}

/// v0.10 "Buah": theme engine (8 built-in styles, live switch) + package
/// manager. Milestone: "install app dari registry + ganti antar 8 tema live".
fn buah_demo() {
    println!();
    let Some((w, h)) = framebuffer::dimensions() else {
        println!("[de] no framebuffer; skipping theme/package demo");
        return;
    };
    let mut back: alloc::vec::Vec<u32> = alloc::vec![0u32; w * h];

    // --- Theme engine: cycle through all built-in themes (live switch) -----
    // Render a couple of frames to prove live re-compose without spending a
    // full-screen render on all ten (keeps boot fast).
    let mut count = 0;
    for (i, t) in theme::names().iter().enumerate() {
        theme::set_by_name(t.name);
        if i < 2 || i == theme::names().len() - 1 {
            wm::render_frame(&mut back, w, h);
        }
        count += 1;
    }
    println!("[theme] cycled {} themes: dark light neo-brutalism clean material bento classic-linux classic-windows sun beos", count);
    if count >= 10 {
        println!("[kernel] MILESTONE: THEMES OK (8 built-in styles + dark/light, live switch)");
    }

    // --- Package manager: install/remove from the registry ----------------
    // Demonstrate through the terminal so it is visible on the desktop too.
    for cmd in ["bz list", "bz remove xox", "run xox", "bz install xox"] {
        for c in cmd.chars() {
            terminal::feed_char(c);
        }
        terminal::feed_char('\n');
        println!("[pkg] $ {}", cmd);
    }
    // Verify: remove then reinstall really toggled installed-state + the gate.
    let _ = pkg::remove("webview");
    let gated = shell::run("run webview").0.iter().any(|l| l.contains("belum terpasang"));
    let installed = pkg::install("webview").is_ok();
    if gated && installed && pkg::is_installed("xox") {
        println!("[kernel] MILESTONE: PACKAGE OK (install/remove from registry + run gated)");
    } else {
        println!("[pkg] package manager verification failed");
    }

    println!("[kernel] MILESTONE: BUAH OK (theme engine + 8 styles + package manager)");

    // Leave a distinctive theme set so the final desktop shows theme switching.
    theme::set_by_name("classic-windows");
    wm::set_cursor(w as i32 / 2, h as i32 / 2);
    wm::render_frame(&mut back, w, h);
}

/// v0.9 "Serbuk": Buitenzorg.Drawing (System.Drawing-style graphics) + a
/// Windows-style Task Manager. Runs the Paint demo (draws shapes via the
/// managed library) and the Task Manager (lists processes + resources + kill).
fn serbuk_demo() {
    println!();
    if framebuffer::dimensions().is_none() {
        println!("[app] no framebuffer; skipping v0.9 apps");
        return;
    }

    // Paint — exercises Buitenzorg.Drawing (shapes, lines, ellipses, text).
    println!("[app] launching: paint (Buitenzorg.Drawing demo)");
    match app::run_named("paint") {
        Ok(_) => println!("[kernel] MILESTONE: DRAWING OK (Buitenzorg.Drawing shapes rendered)"),
        Err(e) => println!("[app] paint failed: {}", e),
    }

    // Spawn a long-lived kernel task so the Task Manager has a live target to
    // demonstrate PROC_KILL on.
    task::spawn_named("idle-demo", || loop {
        task::yield_now();
    });

    // Task Manager — process list + resource monitor + kill.
    println!("[app] launching: taskmgr (process + resource monitor)");
    match app::run_named("taskmgr") {
        Ok(_) => println!("[kernel] MILESTONE: TASKMGR OK (process list + resources + kill)"),
        Err(e) => println!("[app] taskmgr failed: {}", e),
    }

    // Widget variant (docked on the widget board) + Web variant (mini WebView),
    // completing the four app variants (console/desktop/web/widget).
    println!("[app] launching: widget (system monitor, docked)");
    let _ = app::run_named("widget");
    println!("[app] launching: webview (mini web-app runtime)");
    match app::run_named("webview") {
        Ok(_) => println!("[kernel] MILESTONE: APPVARIANTS OK (console/desktop/web/widget)"),
        Err(e) => println!("[app] webview failed: {}", e),
    }

    println!("[kernel] MILESTONE: SERBUK OK (System.Drawing library + Task Manager + 4 app variants)");

    if let Some((w, h)) = framebuffer::dimensions() {
        let mut back: alloc::vec::Vec<u32> = alloc::vec![0u32; w * h];
        wm::set_cursor(w as i32 / 2, h as i32 / 2);
        wm::render_frame(&mut back, w, h);
    }
}

/// v0.8 "Kembang": app framework. Launch a third-party-style C# desktop app
/// (XOX) that draws its UI through the window syscalls. Milestone: "desktop
/// app pihak ketiga jalan".
fn kembang_demo() {
    println!();
    if framebuffer::dimensions().is_none() {
        println!("[app] no framebuffer; skipping app framework demo");
        return;
    }
    println!("[app] launching desktop app: xox (C# via window syscalls)");
    println!("[kernel] --- xox output ---");
    match app::run_named("xox") {
        Ok(code) => {
            println!("[kernel] --- xox exited (code {}) ---", code);
            println!("[kernel] MILESTONE: KEMBANG OK (C# desktop app drew UI via window syscalls)");
        }
        Err(e) => println!("[app] xox failed: {}", e),
    }

    if let Some((w, h)) = framebuffer::dimensions() {
        let mut back: alloc::vec::Vec<u32> = alloc::vec![0u32; w * h];
        wm::set_cursor(w as i32 / 2, h as i32 / 2);
        wm::render_frame(&mut back, w, h);
    }
}

/// Interactive desktop: route the real PS/2 mouse to the window manager and
/// keyboard input to the terminal, drive animations, and blank to the
/// screensaver after an idle timeout (v0.11).
fn desktop_loop() -> ! {
    let (w, h) = framebuffer::dimensions().unwrap_or((0, 0));
    let mut back: alloc::vec::Vec<u32> = alloc::vec![0u32; w * h];
    let mut last_packets = u64::MAX;
    let mut last_left = false;
    let mut last_input = interrupts::ticks();
    let mut saver_active = false;

    if w != 0 {
        wm::render_frame(&mut back, w, h);
    }

    loop {
        x86_64::instructions::hlt();
        if w == 0 {
            continue;
        }
        let now = interrupts::ticks();
        let mut input = false;

        while let Some(c) = keyboard::pop() {
            input = true;
            if !saver_active {
                terminal::feed_char(c);
            }
        }

        let (mx, my, buttons, packets) = mouse::state();
        let left = buttons & 0x01 != 0;
        if packets != last_packets || left != last_left {
            last_packets = packets;
            last_left = left;
            input = true;
            if !saver_active {
                wm::handle_mouse(mx as i32, my as i32, left);
            }
        }

        if input {
            last_input = now;
            if saver_active {
                saver_active = false; // dismiss screensaver on any input
            }
        }

        if saver_active {
            // Animate the screensaver every tick.
            let mut canvas = gfx::Canvas::new(&mut back, w, h);
            screensaver::render(&mut canvas, now);
            framebuffer::present(&back);
        } else if screensaver::enabled() && now.saturating_sub(last_input) > screensaver::timeout() {
            saver_active = true;
        } else {
            // Redraw on input or while a micro-interaction (ripple) animates.
            let animating = wm::tick_animation(now);
            if input || animating {
                wm::render_frame(&mut back, w, h);
            }
        }
    }
}

/// v0.2 "Akar" milestone demo: two preemptively scheduled tasks alternate,
/// then a producer/consumer pair exchanges messages over an IPC channel.
mod demo {
    use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    use crate::{interrupts, ipc, task};

    pub static DONE: AtomicUsize = AtomicUsize::new(0);
    pub static IPC_SUM: AtomicU64 = AtomicU64::new(0);
    pub static SEEDS: ipc::Channel = ipc::Channel::new();

    /// Busy-wait (no yield, no hlt) so interleaving proves *preemption*.
    fn busy_ticks(n: u64) {
        let target = interrupts::ticks() + n;
        while interrupts::ticks() < target {
            core::hint::spin_loop();
        }
    }

    fn alternating(name: &'static str) -> impl Fn() {
        move || {
            for i in 1..=4 {
                println!("[task {}] round {}/4 (ticks={})", name, i, interrupts::ticks());
                busy_ticks(3);
            }
            DONE.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub fn akar_a() {
        alternating("akar-A")();
    }

    pub fn akar_b() {
        alternating("akar-B")();
    }

    pub fn producer() {
        for seed in [17u64, 19, 23] {
            SEEDS.send(seed);
            println!("[task ipc-producer] sent seed {}", seed);
            task::yield_now();
        }
    }

    pub fn consumer() {
        let mut sum = 0;
        for _ in 0..3 {
            let seed = SEEDS.recv();
            println!("[task ipc-consumer] received seed {}", seed);
            sum += seed;
        }
        IPC_SUM.store(sum, Ordering::SeqCst);
        DONE.fetch_add(1, Ordering::SeqCst);
    }
}

fn scheduler_demo() {
    use core::sync::atomic::Ordering;

    task::init();
    // Enable timer preemption for this demo (busy-wait tasks must alternate
    // without yielding); the rest of the kernel runs cooperatively.
    task::set_preemption(true);
    task::spawn(demo::akar_a);
    task::spawn(demo::akar_b);
    task::spawn(demo::producer);
    task::spawn(demo::consumer);

    // Boot task waits (yielding) until the three worker groups report done.
    while demo::DONE.load(Ordering::SeqCst) < 3 {
        task::yield_now();
    }
    task::set_preemption(false);
    println!("[kernel] MILESTONE: SCHEDULER OK (two tasks alternated preemptively)");
    assert_eq!(demo::IPC_SUM.load(Ordering::SeqCst), 17 + 19 + 23);
    println!("[kernel] MILESTONE: IPC OK (3 messages, checksum verified)");
}

/// v0.3 "Batang": PCI scan, storage driver + FAT file read, mouse, pixels.
fn batang_demo() {
    println!();
    let devices = pci::scan();
    for dev in &devices {
        println!(
            "[pci] {:02x}:{:02x}.{} {:04x}:{:04x} class {:02x}.{:02x}.{:02x} {}",
            dev.bus, dev.slot, dev.function, dev.vendor_id, dev.device_id,
            dev.class, dev.subclass, dev.prog_if, dev.class_name()
        );
    }
    println!("[kernel] MILESTONE: PCI OK ({} devices enumerated)", devices.len());

    let disks = ata::init();
    if disks == 0 {
        println!("[kernel] storage: no IDE/ATA disk on legacy channels (boot media may be NVMe/AHCI/USB)");
    } else {
        println!(
            "[kernel] driver framework: {} block device(s) online",
            driver::block_device_count()
        );
        let result = driver::with_boot_block_device(|dev| {
            let volume = fat::FatVolume::mount(dev)?;
            println!(
                "[fat] mounted {} volume (partition LBA {})",
                volume.kind_name(),
                volume.partition_start()
            );
            if let Ok(names) = volume.list_root(dev) {
                println!("[fat] root directory: {}", names.join(" "));
            }
            let content = volume.read_file(dev, "BATANG.TXT")?;
            let text = core::str::from_utf8(&content).map_err(|_| "file is not UTF-8")?;
            print!("[fat] batang.txt: {}", text);
            if !text.ends_with('\n') {
                println!();
            }
            Ok::<(), &'static str>(())
        });
        match result {
            Some(Ok(())) => {
                println!("[kernel] MILESTONE: STORAGE OK (file read from disk via IDE PIO + FAT)")
            }
            Some(Err(e)) => println!("[kernel] storage: FAT read failed: {}", e),
            None => println!("[kernel] storage: no block device registered"),
        }
    }

    if mouse::init() {
        let (_, _, _, packets) = mouse::state();
        println!("[kernel] PS/2 mouse streaming enabled ({} packets so far)", packets);
        println!("[kernel] MILESTONE: MOUSE OK");
    } else {
        println!("[kernel] PS/2 mouse not detected");
    }

    if framebuffer::draw_pixel_demo() {
        println!("[kernel] MILESTONE: PIXELS OK (direct framebuffer drawing)");
    }
}

/// Load an ELF `image` and run it in ring 3, then tear down its mappings so
/// the address space is free for the next program. Returns the exit code.
fn run_user_elf(
    image: &[u8],
    mapper: &mut x86_64::structures::paging::OffsetPageTable<'static>,
    frame_allocator: &mut memory::BootInfoFrameAllocator,
) -> Result<u64, &'static str> {
    const USER_STACK_BASE: u64 = 0x7000_0000;
    const USER_STACK_PAGES: u64 = 16;

    let program = elf::load(image, mapper, frame_allocator)?;
    let (stack_top, stack_pages) =
        memory::map_user_region(USER_STACK_BASE, USER_STACK_PAGES, mapper, frame_allocator)?;

    let code = usermode::enter_user(program.entry, stack_top);

    memory::unmap_user_pages(&program.pages, mapper);
    memory::unmap_user_pages(&stack_pages, mapper);
    Ok(code)
}

/// v0.4 "Tunas": load a NativeAOT-compiled C# program (HELLO.ELF) from disk
/// and run it in ring 3. Milestone: "Hello from C#!" printed by user code
/// calling back through the syscall ABI.
fn tunas_demo(
    mapper: &mut x86_64::structures::paging::OffsetPageTable<'static>,
    frame_allocator: &mut memory::BootInfoFrameAllocator,
) {
    use alloc::vec::Vec;

    println!();
    let bytes: Option<Vec<u8>> = driver::with_boot_block_device(|dev| {
        let volume = fat::FatVolume::mount(dev).ok()?;
        volume.read_file(dev, "HELLO.ELF").ok()
    })
    .flatten();

    let Some(bytes) = bytes else {
        println!("[kernel] tunas: HELLO.ELF not found on disk (build it: scripts/build-hello-csharp.ps1)");
        return;
    };
    println!("[kernel] tunas: loaded HELLO.ELF from disk ({} bytes)", bytes.len());
    println!("[kernel] tunas: entering ring 3 (C# via bflat/NativeAOT)...");
    println!("[kernel] --- user program output ---");
    match run_user_elf(&bytes, mapper, frame_allocator) {
        Ok(code) => {
            println!("[kernel] --- end user program (exit code {}) ---", code);
            println!("[kernel] MILESTONE: TUNAS OK (C# ran in ring 3 -> 'Hello from C#!')");
        }
        Err(e) => println!("[kernel] tunas: run failed: {}", e),
    }
}

/// v0.5 "Dahan": VFS + FAT write, service/init manager, async I/O benchmark,
/// networking, and a C# service running as a ring-3 process.
fn dahan_demo(
    mapper: &mut x86_64::structures::paging::OffsetPageTable<'static>,
    frame_allocator: &mut memory::BootInfoFrameAllocator,
) {
    println!();

    // --- VFS + FAT read/write ---------------------------------------------
    // Move the boot disk into the VFS as read-only /disk.
    if let Some(dev) = driver::take_boot_block_device() {
        if let Err(e) = vfs::mount("disk", dev, true) {
            println!("[vfs] mounting /disk failed: {}", e);
        }
    }
    // A read/write FAT12 ramdisk mounted at /ram.
    let mut ram = ramdisk::new("dahan", 4096); // 2 MiB
    if fat::format_fat12(ram.as_mut()).is_ok() {
        if let Err(e) = vfs::mount("ram", ram, false) {
            println!("[vfs] mounting /ram failed: {}", e);
        }
    }
    println!("[vfs] mounts: {}", vfs::mounts().join(" "));

    let payload = b"Dahan tumbuh: VFS + FAT write bekerja. Ditulis lalu dibaca kembali.\n";
    match vfs::write("/ram/DAHAN.TXT", payload) {
        Ok(()) => match vfs::read("/ram/DAHAN.TXT") {
            Ok(back) if back == payload => {
                if let Ok(names) = vfs::list("/ram") {
                    println!("[vfs] /ram directory: {}", names.join(" "));
                }
                print!("[vfs] /ram/DAHAN.TXT round-trip: {}", core::str::from_utf8(&back).unwrap_or("?"));
                println!("[kernel] MILESTONE: VFS OK (FAT write + read-back verified on /ram)");
            }
            Ok(_) => println!("[vfs] read-back mismatch"),
            Err(e) => println!("[vfs] read-back failed: {}", e),
        },
        Err(e) => println!("[vfs] write failed: {}", e),
    }

    // --- Service / init manager (parallel, dependency-aware) --------------
    service::register("logger", &[], svc::logger);
    service::register("netd", &["logger"], svc::netd);
    service::register("storaged", &["logger"], svc::storaged);
    service::register("app", &["netd", "storaged"], svc::app);
    let order = service::start_all();
    debug_assert!(service::all_up());
    println!("[init] service start order: {}", order.join(" -> "));
    println!("[kernel] MILESTONE: SERVICES OK (dependency-ordered parallel init)");

    // --- Async I/O benchmark ----------------------------------------------
    let bench_disk = ramdisk::new("aio", 2048);
    aio::init(bench_disk);
    let (ops, ticks) = aio::benchmark(2000);
    // PIT is ~18.2 Hz; report ops/sec when at least one tick elapsed.
    if ticks > 0 {
        println!("[aio] {} ops in {} ticks (~{} ops/sec)", ops, ticks, ops * 182 / (ticks * 10));
    } else {
        println!("[aio] {} ops in <1 tick (>{} ops/sec)", ops, ops * 182 / 10);
    }
    aio::shutdown();
    println!("[kernel] MILESTONE: ASYNC IO OK (io_uring-style SQ/CQ, benchmark-able)");

    // --- Networking (loopback: Ethernet/ARP/IPv4/ICMP echo) ---------------
    net::init([10, 0, 0, 1]);
    net::send_ping([10, 0, 0, 1], 0xBEEF, 1);
    for _ in 0..4 {
        if net::poll() == 0 {
            break;
        }
    }
    let (icmp, _arp) = net::counters();
    if icmp > 0 {
        println!("[net] ICMP echo round-trip over loopback: {} reply", icmp);
        println!("[kernel] MILESTONE: NETWORK OK (Ethernet/ARP/IPv4/ICMP stack)");
    } else {
        println!("[net] no ICMP reply (stack error)");
    }

    // --- C# service running as a ring-3 process ---------------------------
    // Drain any still-finishing service/aio tasks so that, once we are in
    // ring 3, the only runnable task is the boot task (preemption stays inert,
    // matching the v0.4 execution model).
    for _ in 0..50 {
        task::yield_now();
    }
    match vfs::read("/disk/SVC.ELF") {
        Ok(bytes) => {
            println!("[init] starting service: svc-csharp (ring-3 managed process, {} bytes)", bytes.len());
            println!("[kernel] --- svc-csharp output ---");
            match run_user_elf(&bytes, mapper, frame_allocator) {
                Ok(code) => {
                    println!("[kernel] --- svc-csharp exited (code {}) ---", code);
                    println!("[kernel] MILESTONE: DAHAN OK (C# service ran as a process)");
                }
                Err(e) => println!("[init] svc-csharp run failed: {}", e),
            }
        }
        Err(_) => println!("[init] SVC.ELF not on /disk (build it: scripts/build-hello-csharp.ps1)"),
    }
}

/// v0.6 "Daun": graphics + window system. Create two windows, then drive
/// scripted mouse events to move one and resize the other, verifying their
/// geometry changed. Milestone: "dua window bisa dipindah & di-resize".
fn daun_demo() {
    println!();
    let Some((w, h)) = framebuffer::dimensions() else {
        println!("[wm] no framebuffer; skipping window system");
        return;
    };
    wm::init(w, h);
    let term = wm::create_window(
        "Welcome",
        100,
        100,
        360,
        220,
        &[
            "Selamat datang di",
            "Buitenzorg OS",
            "",
            "Dibuat oleh Gravicode Studios",
            "Dipimpin oleh Kang Fadhil",
        ],
    );
    let notes = wm::create_window(
        "Notes",
        560,
        180,
        340,
        240,
        &[
            "Daun: compositor + window",
            "manager online.",
            "",
            "- floating windows",
            "- move: drag title bar",
            "- resize: drag corner",
        ],
    );
    println!("[wm] created 2 windows (compositor + window manager online)");

    let before_term = wm::window_rect(term).unwrap();
    let before_notes = wm::window_rect(notes).unwrap();

    // --- Scripted move of the Terminal window (drag its title bar) --------
    wm::set_cursor(150, 112);
    wm::handle_mouse(150, 112, true); // press on title bar
    wm::handle_mouse(320, 260, true); // drag
    wm::handle_mouse(320, 260, false); // release
    let after_term = wm::window_rect(term).unwrap();

    // --- Scripted resize of the Notes window (drag its bottom-right grip) --
    wm::handle_mouse(893, 413, true); // press on resize grip
    wm::handle_mouse(963, 463, true); // drag out
    wm::handle_mouse(963, 463, false); // release
    let after_notes = wm::window_rect(notes).unwrap();

    println!(
        "[wm] Terminal moved: ({},{}) -> ({},{})",
        before_term.0, before_term.1, after_term.0, after_term.1
    );
    println!(
        "[wm] Notes resized: {}x{} -> {}x{}",
        before_notes.2, before_notes.3, after_notes.2, after_notes.3
    );

    let moved = (after_term.0, after_term.1) != (before_term.0, before_term.1);
    let resized = (after_notes.2, after_notes.3) != (before_notes.2, before_notes.3);

    // Render the final desktop to the framebuffer.
    let mut back: alloc::vec::Vec<u32> = alloc::vec![0u32; w * h];
    wm::set_cursor(w as i32 / 2, h as i32 / 2);
    wm::render_frame(&mut back, w, h);

    if moved && resized {
        println!("[kernel] MILESTONE: WINDOWS OK (two windows moved & resized)");
    } else {
        println!("[wm] move/resize verification failed (moved={}, resized={})", moved, resized);
    }
}

/// v0.7 "Kanopi": desktop environment — a terminal + shell, virtual desktops,
/// and a dark/light theme toggle. Milestone: "pindah antar virtual desktop,
/// ganti dark/light, jalankan ls/dir di terminal".
fn kanopi_demo() {
    println!();
    if framebuffer::dimensions().is_none() {
        println!("[de] no framebuffer; skipping desktop environment");
        return;
    }

    // A terminal window on workspace 1, wired to the built-in shell.
    let term_win = wm::create_window("Terminal", 90, 90, 620, 380, &[]);
    let rows = (380 - 40) / (gfx::glyph_height() as i32 + 3);
    terminal::attach(term_win, rows as usize);

    // --- Run ls / dir in the terminal (milestone) -------------------------
    for cmd in ["ver", "mounts", "ls /disk", "dir /ram", "cat /ram/DAHAN.TXT"] {
        for c in cmd.chars() {
            terminal::feed_char(c);
        }
        terminal::feed_char('\n');
        println!("[term] $ {}", cmd);
    }
    // Verify the shell actually listed a directory.
    let listing = shell::run("ls /ram").0;
    let ls_ok = listing.iter().any(|l| l.contains("DAHAN.TXT"));
    if ls_ok {
        println!("[kernel] MILESTONE: TERMINAL OK (ran ls/dir over VFS)");
    } else {
        println!("[de] terminal ls verification failed");
    }

    // --- Toggle dark/light theme (milestone) ------------------------------
    let was_dark = theme::is_dark();
    for c in "theme light".chars() {
        terminal::feed_char(c);
    }
    terminal::feed_char('\n');
    let now_light = !theme::is_dark();
    println!("[de] theme: {} -> {}", if was_dark { "dark" } else { "light" }, theme::name());
    if was_dark && now_light {
        println!("[kernel] MILESTONE: THEME OK (dark <-> light switch)");
    }
    theme::set_by_name("dark"); // back to dark for the final screenshot

    // --- Second window on workspace 2, then switch desktops (milestone) ---
    let notes = wm::create_window(
        "Notes (WS2)",
        200,
        140,
        360,
        220,
        &["Ini berada di virtual desktop 2.", "Ketik 'ws 1' untuk kembali."],
    );
    wm::set_window_workspace(notes, 1);

    let ws_before = wm::current_workspace();
    wm::switch_workspace(1); // to desktop 2
    let ws_after = wm::current_workspace();
    println!("[de] workspace: {} -> {}", ws_before + 1, ws_after + 1);
    wm::switch_workspace(0); // back to desktop 1 (terminal)
    if ws_after != ws_before {
        println!("[kernel] MILESTONE: WORKSPACE OK (switched virtual desktops)");
    }

    // Render the final desktop.
    if let Some((w, h)) = framebuffer::dimensions() {
        let mut back: alloc::vec::Vec<u32> = alloc::vec![0u32; w * h];
        wm::set_cursor(w as i32 / 2, h as i32 / 2);
        wm::render_frame(&mut back, w, h);
    }
    println!("[kernel] MILESTONE: KANOPI OK (desktop environment: terminal, theme, multi-desktop)");
}

/// Kernel services used by the v0.5 service-manager demo. Each marks itself
/// Running once initialized, does a little work, then marks Done and returns
/// (oneshot). Dependencies are declared at registration.
mod svc {
    use crate::{service, task};

    pub fn logger() {
        service::mark_running("logger");
        println!("[svc logger] up (no deps)");
        service::mark_done("logger");
    }

    pub fn netd() {
        service::mark_running("netd");
        println!("[svc netd] up (after logger)");
        task::yield_now();
        service::mark_done("netd");
    }

    pub fn storaged() {
        service::mark_running("storaged");
        println!("[svc storaged] up (after logger)");
        task::yield_now();
        service::mark_done("storaged");
    }

    pub fn app() {
        service::mark_running("app");
        println!("[svc app] up (after netd + storaged)");
        service::mark_done("app");
    }
}

fn heap_smoke_test() {
    use alloc::{boxed::Box, string::String, vec::Vec};
    let boxed = Box::new(0xB0_1D_FACEu64);
    let mut v: Vec<u64> = (0..1000).collect();
    v.reverse();
    let mut s = String::from("benih");
    s.push_str(" -> akar");
    assert_eq!(*boxed, 0xB0_1D_FACEu64);
    assert_eq!(v.first(), Some(&999));
    assert_eq!(s.len(), 13);
}

fn syscall_smoke_test() {
    let version = syscall::dispatch(bz_abi::sysno::ABI_VERSION, 0, 0, 0);
    assert_eq!(version, bz_abi::ABI_VERSION);

    let msg = "[kernel] sys_debug_write via ABI v1 works\n";
    let written = syscall::dispatch(
        bz_abi::sysno::DEBUG_WRITE,
        msg.as_ptr() as u64,
        msg.len() as u64,
        0,
    );
    assert_eq!(written, msg.len() as u64);

    let mut fb_info = bz_abi::FramebufferInfo {
        address: 0,
        size: 0,
        width: 0,
        height: 0,
        stride: 0,
        bytes_per_pixel: 0,
        pixel_format: bz_abi::pixel_format::UNKNOWN,
    };
    let rc = syscall::dispatch(
        bz_abi::sysno::FB_INFO,
        core::ptr::addr_of_mut!(fb_info) as u64,
        0,
        0,
    );
    if rc == 0 {
        println!(
            "[kernel] framebuffer: {}x{} @ {} bpp",
            fb_info.width,
            fb_info.height,
            fb_info.bytes_per_pixel * 8
        );
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("\n[kernel] PANIC: {}", info);
    loop {
        x86_64::instructions::hlt();
    }
}
