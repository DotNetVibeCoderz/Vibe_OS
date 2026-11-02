fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg=-T{}/link.ld", dir);
    println!("cargo:rustc-link-arg=--static");
    println!("cargo:rerun-if-changed=link.ld");
}
