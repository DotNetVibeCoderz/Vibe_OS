//! The interactive desktop session (shared by every architecture): route the
//! pointer to the window manager and keys to the terminal or the focused app,
//! launch Start-menu / icon requests as processes, drive animations, and blank
//! to the screensaver after an idle timeout. Input comes from PS/2 on PCs and
//! from virtio input (polled here) everywhere; frames go out through
//! `framebuffer::present` (which also feeds a virtio GPU).

#[allow(unused_imports)]
use crate::{
    app, framebuffer, gfx, interactive, interrupts, keyboard, mouse, power, println, process, screensaver, task,
    terminal, theme, virtio, wallpaper, wm,
};

/// Make sure the session's standing windows exist: the terminal (wired to the
/// shell), the welcome card and the notes that become "Tips". The PC boot
/// demos create them; a kernel without those demos (the ARM64 port) gets
/// them here. Windows that already exist are left alone.
pub fn ensure_session_windows() {
    if framebuffer::dimensions().is_none() {
        return;
    }
    if wm::window_id("Terminal").is_none() {
        let id = wm::create_window("Terminal", 90, 90, 620, 380, &[]);
        let rows = (380 - 40) / (gfx::glyph_height() as i32 + 3);
        terminal::attach(id, rows as usize);
    }
    if wm::window_id("Welcome").is_none() {
        wm::create_window(
            "Welcome",
            100,
            100,
            360,
            220,
            &["Selamat datang di", "Buitenzorg OS", "", "Dibuat oleh Gravicode Studios", "Dipimpin oleh Kang Fadhil"],
        );
    }
    if wm::window_id("Notes").is_none() && wm::window_id("Tips").is_none() {
        wm::create_window("Notes", 560, 180, 340, 240, &[]);
    }
}

/// Interactive desktop: route the real PS/2 mouse to the window manager and
/// keyboard input to the terminal, drive animations, and blank to the
/// screensaver after an idle timeout (v0.11).
pub fn run() -> ! {
    ensure_session_windows();
    // From here on a real user can type: shell-launched apps may block on input.
    interactive::set_active();
    // Start the session on a tidy desktop: drop the frozen windows the boot
    // demos left behind (their programs have exited).
    let tidied = wm::close_orphan_windows();
    println!("[desktop] tidied {} window(s) left by boot demos", tidied);
    let (w, h) = framebuffer::dimensions().unwrap_or((0, 0));
    // Lay the session out: the terminal on the left, the welcome card and the
    // notes stacked on the right, nothing overlapping; the terminal has focus.
    if w != 0 {
        let (wi, he) = (w as i32, h as i32);
        let right = wi - 24 - 380;
        wm::switch_workspace(0);
        let term_h = he - 34 - 80;
        wm::place_window("Terminal", 112, 40, (right - 136).min(700), term_h);
        terminal::start_session(((term_h - 24 - 14) / 19) as usize);
        wm::place_window("Welcome", right, 40, 380, 190);
        // The demo "Notes" window becomes a short how-to for the session.
        if let Some(id) = wm::window_id("Notes") {
            wm::set_title(id, "Tips");
            let tips = [
                "Mulai: semua app, per kategori",
                "",
                "help          daftar perintah",
                "run <app>     buka app",
                "bg list       pilih wallpaper",
                "theme cycle   ganti tema",
                "ps, kill <id> kelola proses",
                "ws 1..4       pindah desktop",
            ];
            wm::set_window_lines(id, tips.iter().map(|s| alloc::string::String::from(*s)).collect());
        }
        wm::place_window("Tips", right, 250, 380, 214);
        wm::focus_title("Terminal");
    }
    mouse::set_bounds(w, h);
    let mut back: alloc::vec::Vec<u32> = alloc::vec![0u32; w * h];
    let mut last_packets = u64::MAX;
    let mut last_left = false;
    let mut last_presses = mouse::presses();
    let mut last_input = interrupts::ticks();
    let mut last_keys = keyboard::pushed();
    let mut saver_active = false;

    if w != 0 {
        wm::render_frame(&mut back, w, h);
    }

    loop {
        // With user processes ready to run, give them the CPU (they are
        // preempted back to us on the next timer tick); otherwise sleep until
        // the next interrupt.
        if task::user_work_pending() {
            task::yield_now();
        } else {
            task::idle_halt(); // drops the Big Kernel Lock while asleep
        }
        if w == 0 {
            continue;
        }
        // virtio keyboards / mice / tablets (multi-arch M2) are polled here.
        virtio::input::poll();
        let now = interrupts::ticks();
        let mut input = false;

        // Typing is activity even when the keys are not ours: they stay
        // queued for the focused program's KEY_READ, and used to leave the
        // idle timer running (the screensaver came on while someone typed into
        // an app). A key that wakes the screensaver is swallowed, not passed on.
        // Keys typed on the serial console (a board with no keyboard driver yet).
        for _ in 0..64 {
            let Some(b) = crate::serial::read_byte() else { break };
            keyboard::feed_serial(b);
        }
        let keys = keyboard::pushed();
        if keys != last_keys {
            last_keys = keys;
            input = true;
            if saver_active {
                while keyboard::pop().is_some() {}
            }
        }

        // Keys go to the terminal unless the focused window belongs to a
        // running program — then they stay queued for its KEY_READ.
        if !process::is_alive(wm::focus_owner()) {
            while let Some(c) = keyboard::pop() {
                input = true;
                if !saver_active {
                    terminal::feed_char(c);
                }
            }
        }

        let (mx, my, buttons, packets) = mouse::state();
        let left = buttons & 0x01 != 0;
        let presses = mouse::presses();
        if presses != last_presses && !left && !last_left && !saver_active {
            // a whole click happened between two samples: replay it
            wm::handle_mouse(mx as i32, my as i32, true);
            wm::handle_mouse(mx as i32, my as i32, false);
            input = true;
        }
        last_presses = presses;
        if packets != last_packets || left != last_left {
            last_packets = packets;
            last_left = left;
            input = true;
            if !saver_active {
                wm::handle_mouse(mx as i32, my as i32, left);
            }
        }

        // Launch / power actions requested from the desktop shell (Start menu or
        // a desktop icon). The WM queues them; only the desktop loop can run an
        // app or trigger a power action.
        if !saver_active {
            if let Some(app_name) = wm::take_pending_launch() {
                if let Some(action) = app_name.strip_prefix('@') {
                    // Start-menu settings entries are system actions.
                    match action {
                        "wallpaper" => {
                            let names: alloc::vec::Vec<&str> = wallpaper::OFFICIAL.iter().map(|(n, _, _)| *n).collect();
                            let cur = wallpaper::label();
                            let next = names.iter().position(|n| *n == cur).map(|i| (i + 1) % names.len()).unwrap_or(0);
                            let _ = wallpaper::set_official(names[next]);
                        }
                        "theme" => {
                            theme::cycle();
                        }
                        _ => {}
                    }
                    input = true;
                } else {
                // Start it as its own process; the desktop keeps running.
                match app::spawn_named(&app_name) {
                    Ok(pid) => println!("[desktop] launched '{}' as pid {}", app_name, pid),
                    Err(e) => println!("[desktop] launch '{}' failed: {}", app_name, e),
                }
                input = true;
                }
            }
            app::reap_closed_windows();
            // An app that exited leaves no frozen window behind.
            if wm::close_orphan_windows() > 0 {
                input = true;
            }
            match wm::take_pending_power() {
                1 => power::shutdown(),
                2 => power::restart(),
                3 => power::sleep(),
                _ => {}
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
