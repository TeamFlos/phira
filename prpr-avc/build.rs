fn main() {
    let lib_path = format!("{}/static-lib/{}", std::env::var("CARGO_MANIFEST_DIR").unwrap(), std::env::var("TARGET").unwrap());
    println!("cargo:rustc-link-search={lib_path}");
    println!("cargo:rustc-link-lib=z");
    println!("cargo:rerun-if-changed={lib_path}");
}
