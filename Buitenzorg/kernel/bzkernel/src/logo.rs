//! Boot logo (requirements.md §16, v0.1 "Benih": ASCII art boot logo).

/// ASCII art shown on serial and framebuffer console at boot.
pub const BOOT_LOGO: &str = r#"
   ____        _ _                                       ____  _____
  |  _ \      (_) |                                     / __ \/ ____|
  | |_) |_   _ _| |_ ___ _ __  _______  _ __ __ _     | |  | | (___
  |  _ <| | | | | __/ _ \ '_ \|_  / _ \| '__/ _` |    | |  | |\___ \
  | |_) | |_| | | ||  __/ | | |/ / (_) | | | (_| |    | |__| |____) |
  |____/ \__,_|_|\__\___|_| |_/___\___/|_|  \__, |     \____/|_____/
                                             __/ |
              .   .                         |___/
               \_|_/
              -- @ --      v0.1 "Benih" - zonder zorg, tanpa kekhawatiran
               / | \
                 |         kernel: Rust (ring 0)  |  userland: C# (.NET)
             ~~~~~~~~~     Kebun Raya Bogor edition
              dibuat oleh Gravicode Studios -- dipimpin oleh Kang Fadhil
"#;
