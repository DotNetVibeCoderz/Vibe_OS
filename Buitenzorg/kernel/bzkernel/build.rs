// The ARM64 and RISC-V kernels are linked in the higher half with their own linker scripts
// (Limine loads executables at or above 0xffffffff80000000). The x86-64
// kernel keeps the target's default (PIE) layout for the bootloader crate.
fn main() {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if arch == "aarch64" || arch == "riscv64" {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let script = std::path::Path::new(&dir).join("src").join("arch").join(&arch).join("link.ld");
        println!("cargo:rustc-link-arg=-T{}", script.display());
        println!("cargo:rustc-link-arg=-zmax-page-size=4096");
        println!("cargo:rerun-if-changed={}", script.display());
    }
    println!("cargo:rerun-if-changed=build.rs");
}
