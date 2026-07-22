//! Model Manager (v0.12 "Nalar"): a Hugging Face-style model gallery. Browse a
//! catalog of models with metadata (size, license, VRAM, task), "pull" to mark
//! them available offline, and query. The actual weights are not fetched here
//! (multi-GB downloads are out of scope on bare metal); this is the manager +
//! registry that a real download pipeline plugs into (requirements.md §6.2).

use alloc::{string::String, vec::Vec};
use spin::Mutex;

pub struct Model {
    pub id: &'static str,     // Hugging Face-style repo id
    pub task: &'static str,   // text-generation | vision | text-to-image | speech
    pub size_mb: u32,
    pub vram_mb: u32,
    pub license: &'static str,
    pub format: &'static str, // gguf | onnx | safetensors
}

/// The gallery catalog (subset of popular models).
pub const GALLERY: &[Model] = &[
    Model { id: "TinyLlama/TinyLlama-1.1B", task: "text-generation", size_mb: 640, vram_mb: 1200, license: "Apache-2.0", format: "gguf" },
    Model { id: "microsoft/phi-2", task: "text-generation", size_mb: 1600, vram_mb: 3000, license: "MIT", format: "gguf" },
    Model { id: "openai/whisper-base", task: "speech", size_mb: 140, vram_mb: 500, license: "MIT", format: "onnx" },
    Model { id: "microsoft/trocr-base", task: "vision", size_mb: 330, vram_mb: 900, license: "MIT", format: "onnx" },
    Model { id: "stabilityai/sd-turbo", task: "text-to-image", size_mb: 2400, vram_mb: 4000, license: "SAI-NC", format: "safetensors" },
    // The built-in tiny model that actually runs locally (ai.rs).
    Model { id: "buitenzorg/nalar-bigram", task: "text-generation", size_mb: 1, vram_mb: 0, license: "MIT", format: "builtin" },
];

/// Models pulled (available offline). The built-in one is always present.
static PULLED: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn seed() {
    let mut p = PULLED.lock();
    if p.is_empty() {
        p.push(String::from("buitenzorg/nalar-bigram"));
    }
}

pub fn find(id: &str) -> Option<&'static Model> {
    GALLERY.iter().find(|m| m.id == id || short(m.id) == id)
}

/// Short name = the part after '/'.
pub fn short(id: &str) -> &str {
    id.rsplit('/').next().unwrap_or(id)
}

pub fn is_pulled(id: &str) -> bool {
    let full = find(id).map(|m| m.id).unwrap_or(id);
    PULLED.lock().iter().any(|x| x == full)
}

/// "Download" a model from the gallery (registers it as available offline).
pub fn pull(id: &str) -> Result<&'static Model, &'static str> {
    let m = find(id).ok_or("model not in gallery")?;
    if is_pulled(m.id) {
        return Err("already available");
    }
    PULLED.lock().push(String::from(m.id));
    Ok(m)
}
