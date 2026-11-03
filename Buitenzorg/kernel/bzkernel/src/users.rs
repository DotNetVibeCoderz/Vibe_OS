//! User accounts: names, salted password hashes, an administrator flag and a
//! profile picture. The foundation for logging in -- the SSH server and a
//! login screen check passwords here -- and for the "Pengguna" page of the
//! Settings app (USER_OP syscall).
//!
//! Passwords are never stored: each account keeps a random 16-byte salt and
//! PBKDF2-HMAC-SHA256 of the password (`ITERATIONS` rounds), compared in
//! constant time. Accounts persist in `/disk/USERS.CFG`, one per line:
//! `name:admin:salt-hex:hash-hex:avatar-path`. Without the file the system
//! starts with the default account **administrator** / **ganteng@ku**
//! (change it in Pengaturan > Pengguna).
//!
//! Changing accounts needs an administrator's password (or, to change your
//! own password, the current one); at least one administrator always remains.

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

pub const DEFAULT_USER: &str = "administrator";
pub const DEFAULT_PASSWORD: &str = "ganteng@ku";
pub const DEFAULT_AVATAR: &str = "/disk/AVATAR1.BMP";
const FILE: &str = "/disk/USERS.CFG";
/// PBKDF2 rounds: ~1 ms natively, still well under a second under emulation.
const ITERATIONS: u32 = 2_000;
const MAX_USERS: usize = 16;

#[derive(Clone)]
pub struct User {
    pub name: String,
    pub admin: bool,
    pub avatar: String,
    salt: [u8; 16],
    hash: [u8; 32],
}

static USERS: Mutex<Vec<User>> = Mutex::new(Vec::new());
static SALT_COUNTER: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

fn derive(password: &str, salt: &[u8; 16]) -> [u8; 32] {
    let mut out = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), salt, ITERATIONS, &mut out);
    out
}

/// A fresh salt from the kernel's random generator (`rng.rs`).
fn new_salt() -> [u8; 16] {
    SALT_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    crate::rng::bytes::<16>()
}

fn eq_ct(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        s.push(char::from_digit((b & 15) as u32, 16).unwrap());
    }
    s
}

fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// A user name: 1-16 of a-z, 0-9, '_', '-', '.', starting with a letter.
pub fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 16
        && n.as_bytes()[0].is_ascii_lowercase()
        && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-' || b == b'.')
}

/// A password: 4-64 printable characters, no newline.
pub fn valid_password(p: &str) -> bool {
    (4..=64).contains(&p.chars().count()) && !p.chars().any(|c| c.is_control())
}

fn make(name: &str, password: &str, admin: bool, avatar: &str) -> User {
    let salt = new_salt();
    User { name: String::from(name), admin, avatar: String::from(avatar), salt, hash: derive(password, &salt) }
}

/// Load `/disk/USERS.CFG`; without it (or with no valid account) create the
/// default administrator. Call once `/disk` is mounted.
pub fn load() {
    let mut list = Vec::new();
    if let Ok(bytes) = crate::vfs::read(FILE) {
        if let Ok(text) = core::str::from_utf8(&bytes) {
            for line in text.lines() {
                let f: Vec<&str> = line.trim().splitn(5, ':').collect();
                if f.len() < 4 || !valid_name(f[0]) {
                    continue;
                }
                let (Some(salt), Some(hash)) = (unhex::<16>(f[2]), unhex::<32>(f[3])) else { continue };
                list.push(User {
                    name: String::from(f[0]),
                    admin: f[1] == "1",
                    avatar: String::from(f.get(4).copied().unwrap_or(DEFAULT_AVATAR)),
                    salt,
                    hash,
                });
            }
        }
    }
    let created = !list.iter().any(|u| u.admin);
    if created {
        list.retain(|u| u.name != DEFAULT_USER);
        list.insert(0, make(DEFAULT_USER, DEFAULT_PASSWORD, true, DEFAULT_AVATAR));
    }
    let n = list.len();
    *USERS.lock() = list;
    if created {
        save();
    }
    crate::println!(
        "[users] {} account(s){}",
        n,
        if created { "; created the default 'administrator' (change its password in Pengaturan > Pengguna)" } else { "" }
    );
}

fn save() {
    let mut s = String::new();
    for u in USERS.lock().iter() {
        s.push_str(&alloc::format!("{}:{}:{}:{}:{}\r\n", u.name, u.admin as u8, hex(&u.salt), hex(&u.hash), u.avatar));
    }
    let _ = crate::vfs::write(FILE, s.as_bytes());
}

/// Does `password` belong to `name`?
pub fn verify(name: &str, password: &str) -> bool {
    let Some(u) = USERS.lock().iter().find(|u| u.name == name).cloned() else {
        // spend the same time on unknown names
        let _ = derive(password, &[0; 16]);
        return false;
    };
    eq_ct(&derive(password, &u.salt), &u.hash)
}

fn is_admin(name: &str, password: &str) -> bool {
    USERS.lock().iter().any(|u| u.name == name && u.admin) && verify(name, password)
}

/// (name, administrator?, avatar path) of every account.
pub fn list() -> Vec<(String, bool, String)> {
    USERS.lock().iter().map(|u| (u.name.clone(), u.admin, u.avatar.clone())).collect()
}

pub fn add(admin: &str, admin_password: &str, name: &str, password: &str, as_admin: bool) -> Result<(), &'static str> {
    if !is_admin(admin, admin_password) {
        return Err("an administrator's password is needed");
    }
    if !valid_name(name) {
        return Err("invalid user name");
    }
    if !valid_password(password) {
        return Err("the password needs 4-64 characters");
    }
    {
        let mut users = USERS.lock();
        if users.iter().any(|u| u.name == name) {
            return Err("that user exists");
        }
        if users.len() >= MAX_USERS {
            return Err("too many users");
        }
        let avatar = alloc::format!("/disk/AVATAR{}.BMP", users.len() % 6 + 1);
        users.push(make(name, password, as_admin, &avatar));
    }
    save();
    Ok(())
}

pub fn remove(admin: &str, admin_password: &str, name: &str) -> Result<(), &'static str> {
    if !is_admin(admin, admin_password) {
        return Err("an administrator's password is needed");
    }
    {
        let mut users = USERS.lock();
        let Some(i) = users.iter().position(|u| u.name == name) else { return Err("no such user") };
        if users[i].admin && users.iter().filter(|u| u.admin).count() == 1 {
            return Err("the last administrator cannot be deleted");
        }
        users.remove(i);
    }
    save();
    Ok(())
}

/// Change `name`'s password, proving it with the current one.
pub fn change_password(name: &str, old: &str, new: &str) -> Result<(), &'static str> {
    if !verify(name, old) {
        return Err("the current password is wrong");
    }
    set_password(name, new)
}

/// An administrator sets someone's password (a forgotten one).
pub fn reset_password(admin: &str, admin_password: &str, name: &str, new: &str) -> Result<(), &'static str> {
    if !is_admin(admin, admin_password) {
        return Err("an administrator's password is needed");
    }
    set_password(name, new)
}

fn set_password(name: &str, new: &str) -> Result<(), &'static str> {
    if !valid_password(new) {
        return Err("the password needs 4-64 characters");
    }
    {
        let mut users = USERS.lock();
        let u = users.iter_mut().find(|u| u.name == name).ok_or("no such user")?;
        u.salt = new_salt();
        u.hash = derive(new, &u.salt);
    }
    save();
    Ok(())
}

/// Set the profile picture (a BMP path on a mount), with that user's
/// password or an administrator's.
pub fn set_avatar(who: &str, password: &str, name: &str, path: &str) -> Result<(), &'static str> {
    let allowed = (who == name && verify(who, password)) || is_admin(who, password);
    if !allowed {
        return Err("password needed");
    }
    if !path.starts_with('/') || path.len() > 64 || path.contains(':') {
        return Err("invalid picture path");
    }
    {
        let mut users = USERS.lock();
        let u = users.iter_mut().find(|u| u.name == name).ok_or("no such user")?;
        u.avatar = String::from(path);
    }
    save();
    Ok(())
}

/// Boot check (`USERS OK`): the default account verifies, a wrong password
/// does not, an account can be added / its password changed / removed.
pub fn self_test() -> Result<(), &'static str> {
    let admin_pw_ok = verify(DEFAULT_USER, DEFAULT_PASSWORD);
    if verify(DEFAULT_USER, "salah") || verify("tidakada", DEFAULT_PASSWORD) {
        return Err("a wrong password or unknown user verified");
    }
    // the rest needs the default password (a changed one is fine: skip)
    if !admin_pw_ok {
        return Ok(());
    }
    let _ = remove(DEFAULT_USER, DEFAULT_PASSWORD, "bztest");
    add(DEFAULT_USER, DEFAULT_PASSWORD, "bztest", "rahasia1", false)?;
    if !verify("bztest", "rahasia1") || add("bztest", "rahasia1", "bztest2", "xxxx", false).is_ok() {
        return Err("add / non-admin rights wrong");
    }
    change_password("bztest", "rahasia1", "rahasia2")?;
    if verify("bztest", "rahasia1") || !verify("bztest", "rahasia2") {
        return Err("password change did not take");
    }
    remove(DEFAULT_USER, DEFAULT_PASSWORD, "bztest")?;
    if remove(DEFAULT_USER, DEFAULT_PASSWORD, DEFAULT_USER).is_ok() {
        return Err("the last administrator was deleted");
    }
    Ok(())
}
