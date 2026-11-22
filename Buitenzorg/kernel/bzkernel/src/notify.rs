//! Notifications (generic, every architecture): programs and the kernel post
//! a short message (app, title, body); the desktop shows the newest one as a
//! toast in the top-right corner for a few seconds and keeps the last
//! `MAX_NOTES` in the action center, opened from the bell in the tray.
//!
//! Posting never blocks and never allocates more than the strings: it can be
//! called from the NOTIFY syscall, a shell command or a kernel subsystem
//! (a USB disk plugged in, a screenshot saved, an app that failed to start).
//! It must not be called from an interrupt handler or from `_print`.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
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
    crate::println!("[notify] #{} {}: {} -- {}", id, note.app, note.title, note.body);
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
    let n = st.notes.last()?;
    (now.saturating_sub(n.ticks) < TOAST_TICKS && n.id != st.dismissed).then(|| n.clone())
}

/// Hide the current toast (a click on it).
pub fn dismiss_toast() {
    let mut st = STATE.lock();
    if let Some(n) = st.notes.last() {
        st.dismissed = n.id;
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
        st.notes.last().map(|n| (n.id as u64, n.ticks))
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
        Some(_) => {
            let (app, rest) = if args[0] == "-a" && args.len() >= 3 { (args[1], &args[2..]) } else { ("Shell", args) };
            let title = rest[0];
            let body = rest[1..].join(" ");
            let id = post(app, title, &body);
            alloc::vec![format!("notify: #{} dikirim", id)]
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
    let gen_ok = generation() > g0;
    let ok = toast_ok && unread_ok && read_ok && dismissed_ok && removed && listed && cmd_ok && cleared && gen_ok;
    if ok {
        crate::println!("[kernel] MILESTONE: NOTIFY OK (toast, unread count, action-center read/remove/clear, shell notify)");
    } else {
        crate::println!(
            "[notify] self-test: toast {} unread {} read {} dismiss {} remove {} list {} cmd {} clear {} gen {}",
            toast_ok, unread_ok, read_ok, dismissed_ok, removed, listed, cmd_ok, cleared, gen_ok
        );
    }
    ok
}
