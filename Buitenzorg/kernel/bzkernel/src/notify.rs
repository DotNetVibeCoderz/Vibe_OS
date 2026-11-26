//! Notifications (generic, every architecture): programs and the kernel post
//! a short message (app, title, body); the desktop shows the newest one as a
//! toast in the top-right corner for a few seconds and keeps the last
//! `MAX_NOTES` in the action center, opened from the bell in the tray.
//!
//! Posting never blocks and never allocates more than the strings: it can be
//! called from the NOTIFY syscall, a shell command or a kernel subsystem
//! (a USB disk plugged in, a screenshot saved, an app that failed to start).
//! It must not be called from an interrupt handler or from `_print`.
//!
//! Quiet mode: while a full-screen program has the screen (a game, a video)
//! or "do not disturb" is on, a normal notification only goes to the action
//! center; its toast is held and shown once the quiet time ends (the newest
//! one, with how many waited). An URGENT one (`bz_abi::notify_flags`) toasts
//! at once.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

/// Notifications kept for the action center.
pub const MAX_NOTES: usize = 50;
/// How long a toast stays (timer ticks, 18.2 per second): ~4.5 s.
pub const TOAST_TICKS: u64 = 82;

#[derive(Clone)]
pub struct Note {
    pub id: u32,
    pub app: String,
    pub title: String,
    pub body: String,
    /// Local time it arrived (hour, minute).
    pub time: (u8, u8),
    /// Tick it arrived (for the toast).
    pub ticks: u64,
    pub read: bool,
    /// Shown even in quiet mode.
    pub urgent: bool,
    /// Arrived in quiet mode: its toast waits until quiet mode ends.
    pub held: bool,
}

struct State {
    notes: Vec<Note>,
    next_id: u32,
    /// Toast hidden early (clicked away) for this id.
    dismissed: u32,
}

static STATE: Mutex<State> = Mutex::new(State { notes: Vec::new(), next_id: 1, dismissed: 0 });
/// Bumped by every change: the compositor's damage key.
static GEN: AtomicU64 = AtomicU64::new(0);
/// A full-screen program has the screen (set by the desktop loop).
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
/// "Do not disturb" (shell `notify dnd on|off`).
static DND: AtomicBool = AtomicBool::new(false);

/// Toasts are held now.
pub fn quiet() -> bool {
    FULLSCREEN.load(Ordering::Relaxed) || DND.load(Ordering::Relaxed)
}

fn quiet_changed(was: bool) {
    if was && !quiet() {
        release_held();
    }
    GEN.fetch_add(1, Ordering::Relaxed);
}

/// The desktop loop reports whether a full-screen window is showing.
pub fn set_fullscreen(on: bool) {
    let was = quiet();
    if FULLSCREEN.swap(on, Ordering::Relaxed) != on {
        quiet_changed(was);
    }
}

/// Turn "do not disturb" on or off.
pub fn set_dnd(on: bool) {
    let was = quiet();
    if DND.swap(on, Ordering::Relaxed) != on {
        quiet_changed(was);
    }
}

pub fn dnd() -> bool {
    DND.load(Ordering::Relaxed)
}

/// Quiet mode ended: the newest held notification toasts now, its title
/// saying how many waited; the others stay in the action center.
fn release_held() {
    let now = crate::interrupts::ticks();
    let mut st = STATE.lock();
    let n = st.notes.iter().filter(|n| n.held).count();
    let Some(last) = st.notes.iter().rposition(|n| n.held) else { return };
    for note in st.notes.iter_mut() {
        note.held = false;
    }
    // the newest held one is the toast now: move it to the end, past any
    // urgent note that toasted during the quiet time
    let mut note = st.notes.remove(last);
    note.ticks = now;
    if n > 1 {
        note.title = alloc::format!("{} (+{} lainnya)", note.title, n - 1);
    }
    st.notes.push(note);
}

fn clip(s: &str, max: usize) -> String {
    let mut out = String::new();
    for c in s.chars().filter(|c| !c.is_control()) {
        if out.len() + c.len_utf8() > max {
            break;
        }
        out.push(c);
    }
    out
}

/// Post a notification; returns its id (shown in the log too).
pub fn post(app: &str, title: &str, body: &str) -> u32 {
    post_with(app, title, body, false)
}

/// Post a notification, `urgent` = toast even in quiet mode.
pub fn post_with(app: &str, title: &str, body: &str, urgent: bool) -> u32 {
    let (h, m) = {
        let (_, _, _, h, m, _) = crate::clock::now();
        (h, m)
    };
    let note = Note {
        id: 0,
        app: clip(app, 32),
        title: clip(title, 80),
        body: clip(body, 240),
        time: (h, m),
        ticks: crate::interrupts::ticks(),
        read: false,
        urgent,
        held: !urgent && quiet(),
    };
    let id = {
        let mut st = STATE.lock();
        let id = st.next_id;
        st.next_id += 1;
        if st.notes.len() >= MAX_NOTES {
            st.notes.remove(0);
        }
        st.notes.push(Note { id, ..note.clone() });
        id
    };
    GEN.fetch_add(1, Ordering::Relaxed);
    crate::println!(
        "[notify] #{} {}: {} -- {}{}",
        id,
        note.app,
        note.title,
        note.body,
        if note.held { " (held: quiet mode)" } else if urgent { " (urgent)" } else { "" }
    );
    id
}

/// Every notification, oldest first.
pub fn list() -> Vec<Note> {
    STATE.lock().notes.clone()
}

/// Notifications not seen in the action center yet.
pub fn unread() -> usize {
    STATE.lock().notes.iter().filter(|n| !n.read).count()
}

/// The action center was opened: everything counts as seen.
pub fn mark_all_read() {
    let mut st = STATE.lock();
    if st.notes.iter().any(|n| !n.read) {
        for n in st.notes.iter_mut() {
            n.read = true;
        }
        GEN.fetch_add(1, Ordering::Relaxed);
    }
}

/// Remove one notification; true when it existed.
pub fn remove(id: u32) -> bool {
    let mut st = STATE.lock();
    let before = st.notes.len();
    st.notes.retain(|n| n.id != id);
    let gone = st.notes.len() != before;
    if gone {
        GEN.fetch_add(1, Ordering::Relaxed);
    }
    gone
}

/// Remove every notification; returns how many there were.
pub fn clear() -> usize {
    let mut st = STATE.lock();
    let n = st.notes.len();
    st.notes.clear();
    GEN.fetch_add(1, Ordering::Relaxed);
    n
}

/// The toast to show at `now`: the newest notification while it is young
/// and not dismissed.
pub fn toast(now: u64) -> Option<Note> {
    let st = STATE.lock();
    let n = current(&st)?;
    (now.saturating_sub(n.ticks) < TOAST_TICKS && n.id != st.dismissed).then(|| n.clone())
}

/// The notification a toast would show: the newest one not held.
fn current(st: &State) -> Option<&Note> {
    st.notes.iter().rev().find(|n| !n.held)
}

/// Hide the current toast (a click on it).
pub fn dismiss_toast() {
    let mut st = STATE.lock();
    if let Some(id) = current(&st).map(|n| n.id) {
        st.dismissed = id;
        GEN.fetch_add(1, Ordering::Relaxed);
    }
}

/// Changes so far (the desktop redraws when this moves).
pub fn generation() -> u64 {
    GEN.load(Ordering::Relaxed)
}

/// The toast expired at `now` (the desktop must redraw once): true once per
/// toast.
pub fn toast_expired(now: u64) -> bool {
    static SHOWN: AtomicU64 = AtomicU64::new(0);
    let cur = {
        let st = STATE.lock();
        current(&st).map(|n| (n.id as u64, n.ticks))
    };
    let Some((id, t)) = cur else { return false };
    if now.saturating_sub(t) < TOAST_TICKS {
        SHOWN.store(id, Ordering::Relaxed);
        return false;
    }
    if SHOWN.load(Ordering::Relaxed) == id {
        SHOWN.store(0, Ordering::Relaxed);
        GEN.fetch_add(1, Ordering::Relaxed);
        return true;
    }
    false
}

/// Shell `notify`: post, list, clear.
pub fn command(args: &[&str]) -> Vec<String> {
    use alloc::format;
    match args.first().copied() {
        None | Some("list") => {
            let notes = list();
            if notes.is_empty() {
                return alloc::vec![String::from("notify: tidak ada notifikasi")];
            }
            notes
                .iter()
                .rev()
                .map(|n| format!("#{} {:02}:{:02} {}{}: {} -- {}", n.id, n.time.0, n.time.1, if n.read { "" } else { "* " }, n.app, n.title, n.body))
                .collect()
        }
        Some("clear") => alloc::vec![format!("notify: {} notifikasi dihapus", clear())],
        Some("rm") | Some("remove") => {
            let id = args.get(1).and_then(|s| s.trim_start_matches('#').parse::<u32>().ok()).unwrap_or(0);
            alloc::vec![if remove(id) { format!("notify: #{} dihapus", id) } else { format!("notify: tidak ada #{}", id) }]
        }
        Some("read") => {
            mark_all_read();
            alloc::vec![String::from("notify: semua sudah dibaca")]
        }
        Some("dnd") => {
            match args.get(1).copied() {
                Some("on") => set_dnd(true),
                Some("off") => set_dnd(false),
                _ => {}
            }
            alloc::vec![format!(
                "notify: jangan ganggu {}{}",
                if dnd() { "aktif" } else { "mati" },
                if FULLSCREEN.load(Ordering::Relaxed) { "; program layar penuh sedang tampil (toast ditahan)" } else { "" }
            )]
        }
        Some(_) => {
            let mut rest = args;
            let mut app = "Shell";
            let mut urgent = false;
            loop {
                match rest.first().copied() {
                    Some("-a") if rest.len() >= 3 => {
                        app = rest[1];
                        rest = &rest[2..];
                    }
                    Some("-u") if rest.len() >= 2 => {
                        urgent = true;
                        rest = &rest[1..];
                    }
                    _ => break,
                }
            }
            let title = rest[0];
            let body = rest[1..].join(" ");
            let id = post_with(app, title, &body, urgent);
            alloc::vec![format!("notify: #{} dikirim{}", id, if !urgent && quiet() { " (ditahan: mode senyap)" } else { "" })]
        }
    }
}

/// Self-test boot: post, toast, unread count, mark read, remove, clear
/// (restores nothing the user had -- it runs before the session starts).
pub fn self_test() -> bool {
    let now = crate::interrupts::ticks();
    let g0 = generation();
    let a = post("Uji", "Notifikasi pertama", "isi satu");
    let b = post("Uji", "Notifikasi kedua", "isi dua");
    let toast_ok = toast(now).is_some_and(|n| n.id == b) && toast(now + TOAST_TICKS + 1).is_none();
    let unread_ok = unread() >= 2;
    mark_all_read();
    let read_ok = unread() == 0;
    dismiss_toast();
    let dismissed_ok = toast(now).is_none();
    let removed = remove(a) && !remove(a);
    let listed = list().iter().any(|n| n.id == b);
    let cmd_ok = command(&["-a", "Shell", "Dari", "perintah", "notify"]).first().is_some_and(|l| l.contains("dikirim"))
        && command(&["list"]).first().is_some_and(|l| l.contains("Dari"));
    let cleared = clear() >= 2 && list().is_empty();
    // quiet mode: a normal note is held (no toast), an urgent one shows, the
    // held one toasts when quiet mode ends -- with how many waited
    let was_dnd = dnd();
    set_dnd(true);
    let t = crate::interrupts::ticks();
    let h1 = post("Uji", "Ditahan satu", "");
    let h2 = post("Uji", "Ditahan dua", "");
    let held_ok = toast(t).is_none() && list().iter().filter(|n| n.held).count() == 2;
    let u = post_with("Uji", "Mendesak", "", true);
    let urgent_ok = toast(t).is_some_and(|n| n.id == u);
    set_dnd(false);
    let now2 = crate::interrupts::ticks();
    let released = toast(now2).is_some_and(|n| n.id == h2 && n.title.contains("+1")) && list().iter().all(|n| !n.held) && h1 != h2;
    set_dnd(was_dnd);
    clear();
    let quiet_ok = held_ok && urgent_ok && released;
    let gen_ok = generation() > g0;
    let ok = toast_ok && unread_ok && read_ok && dismissed_ok && removed && listed && cmd_ok && cleared && quiet_ok && gen_ok;
    if ok {
        crate::println!("[kernel] MILESTONE: NOTIFY OK (toast, unread count, action-center read/remove/clear, shell notify, quiet mode + urgent)");
    } else {
        crate::println!(
            "[notify] self-test: toast {} unread {} read {} dismiss {} remove {} list {} cmd {} clear {} quiet {}/{}/{} gen {}",
            toast_ok, unread_ok, read_ok, dismissed_ok, removed, listed, cmd_ok, cleared, held_ok, urgent_ok, released, gen_ok
        );
    }
    ok
}
