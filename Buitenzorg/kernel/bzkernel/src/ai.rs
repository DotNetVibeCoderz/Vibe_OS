//! AI subsystem (v0.12 "Nalar"): a uniform **AI System API** backed by tiny,
//! real, local inference that runs on the CPU via the compute layer. The
//! architecture mirrors requirements.md §6 — LLM, computer vision, and GenAI —
//! deliberately scaled down (a production stack loads GGUF/ONNX models on the
//! GPU/NPU; here the models are toy-sized so they run in-kernel and offline).

use alloc::string::String;
use alloc::vec::Vec;

// --- LLM: a character-level n-gram (bigram) language model -------------------

/// A small training corpus (Buitenzorg lore). The bigram model learns which
/// character tends to follow which — a genuine, if tiny, local language model.
const CORPUS: &str = "buitenzorg adalah sistem operasi ai-native. kernel ditulis \
dengan rust, aplikasi dan layanan ai dengan c sharp. tanpa kekhawatiran, zonder \
zorg. kebun raya bogor menumbuhkan benih menjadi akar, batang, daun, kanopi, \
kembang, buah. ai lokal berjalan offline. model dari hugging face. selamat datang.";

fn char_index(c: u8) -> usize {
    // Map a-z, space, and a few marks into a compact alphabet.
    match c {
        b'a'..=b'z' => (c - b'a') as usize,
        b' ' => 26,
        b'.' => 27,
        b',' => 28,
        _ => 26,
    }
}

const ALPHA: usize = 29;
const CHARS: [u8; ALPHA] = *b"abcdefghijklmnopqrstuvwxyz .,";

/// Build the bigram transition counts from the corpus (the "trained model").
fn build_bigram() -> Vec<[u32; ALPHA]> {
    let mut table = alloc::vec![[0u32; ALPHA]; ALPHA];
    let bytes = CORPUS.as_bytes();
    for w in bytes.windows(2) {
        let a = char_index(w[0].to_ascii_lowercase());
        let b = char_index(w[1].to_ascii_lowercase());
        table[a][b] += 1;
    }
    table
}

/// Generate `len` characters continuing from `prompt` using the bigram model.
/// Deterministic (weighted-argmax with a rotating tie-break) so it is testable.
pub fn llm_complete(prompt: &str, len: usize) -> String {
    let table = build_bigram();
    let mut out = String::from(prompt);
    let mut cur = prompt
        .bytes()
        .last()
        .map(|c| char_index(c.to_ascii_lowercase()))
        .unwrap_or(26);
    let mut seed = 0x1234u32;
    for _ in 0..len {
        let row = &table[cur];
        let total: u32 = row.iter().sum();
        let next = if total == 0 {
            26 // space
        } else {
            // Weighted pick using a simple LCG for reproducibility.
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let mut r = (seed >> 8) % total;
            let mut idx = 0;
            for (i, &c) in row.iter().enumerate() {
                if r < c {
                    idx = i;
                    break;
                }
                r -= c;
            }
            idx
        };
        out.push(CHARS[next] as char);
        cur = next;
    }
    out
}

// --- Computer vision: edge detection over a grayscale image -----------------

/// Sobel-style edge magnitude over a `w`×`h` grayscale buffer, returning the
/// count of edge pixels above `threshold` (a real CV kernel via the compute
/// layer's style of tight loops).
pub fn vision_edges(gray: &[u8], w: usize, h: usize, threshold: u16) -> usize {
    let at = |x: usize, y: usize| gray[y * w + x] as i32;
    let mut edges = 0;
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let gx = -at(x - 1, y - 1) - 2 * at(x - 1, y) - at(x - 1, y + 1)
                + at(x + 1, y - 1) + 2 * at(x + 1, y) + at(x + 1, y + 1);
            let gy = -at(x - 1, y - 1) - 2 * at(x, y - 1) - at(x + 1, y - 1)
                + at(x - 1, y + 1) + 2 * at(x, y + 1) + at(x + 1, y + 1);
            let mag = (gx.abs() + gy.abs()) as u16;
            if mag > threshold {
                edges += 1;
            }
        }
    }
    edges
}

// --- GenAI: procedural text-to-image ----------------------------------------

/// Text-to-image: deterministically synthesize a `w`×`h` RGB image from a
/// prompt (a toy diffusion-free generator — a hash of the prompt drives colors
/// and shapes). Returns the pixel buffer (0x00RRGGBB).
pub fn genai_image(prompt: &str, w: usize, h: usize) -> Vec<u32> {
    let mut seed: u64 = 0xcbf29ce484222325;
    for b in prompt.bytes() {
        seed = (seed ^ b as u64).wrapping_mul(0x100000001b3);
    }
    let hue = (seed & 0xFF) as u32;
    let mut img = alloc::vec![0u32; w * h];
    for y in 0..h {
        for x in 0..w {
            // Interference pattern seeded by the prompt.
            let a = ((x as u64 * (3 + (seed & 7))) ^ (y as u64 * (5 + ((seed >> 3) & 7)))) as u32;
            let r = ((a.wrapping_add(hue)) & 0xFF) as u8;
            let g = ((a >> 2).wrapping_add(0x40) & 0xFF) as u8;
            let b = ((a >> 4).wrapping_add(hue / 2) & 0xFF) as u8;
            img[y * w + x] = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
        }
    }
    img
}

/// Self-test / demo: exercise all three AI capabilities and return short
/// human-readable results.
pub fn selftest() -> (String, usize, u32) {
    let text = llm_complete("kernel ", 40);
    // A small synthetic grayscale image with a bright square (edges present).
    let (w, h) = (32usize, 32usize);
    let mut gray = alloc::vec![20u8; w * h];
    for y in 8..24 {
        for x in 8..24 {
            gray[y * w + x] = 220;
        }
    }
    let edges = vision_edges(&gray, w, h, 200);
    let img = genai_image("kebun raya bogor", 16, 16);
    let checksum: u32 = img.iter().map(|&p| p & 0xFF).sum();
    (text, edges, checksum)
}
