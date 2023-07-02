fn main() {
    println!("cargo:rustc-link-search={}/static-lib/{}", std::env::var("CARGO_MANIFEST_DIR").unwrap(), std::env::var("CARGO_CFG_TARGET_OS").unwrap());
    println!("cargo:rustc-link-lib=z");
}
