use std::fs;

const XCCONFIG_PATH: &str = "xcode/Shared.xcconfig";
const XCCONFIG_VERSION_KEY: &str = "MARKETING_VERSION";

fn main() {
    let cargo_version = std::env::var("CARGO_PKG_VERSION").unwrap();

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let xcconfig_path = std::path::Path::new(&manifest_dir).parent().unwrap().join(XCCONFIG_PATH);
    let content = fs::read_to_string(&xcconfig_path).unwrap_or_else(|_| panic!("`{XCCONFIG_PATH}` not found"));

    let xcode_version = content
        .lines()
        .find(|l| l.trim_start().starts_with(XCCONFIG_VERSION_KEY))
        .and_then(|l| l.split_once('=').map(|x| x.1))
        .map(|v| v.trim())
        .unwrap_or_else(|| panic!("{XCCONFIG_VERSION_KEY} not found in {}", xcconfig_path.display()));
    if cargo_version != xcode_version {
        panic!(
            "Inconsistent Version:\n\
             Cargo.toml={cargo_version}, {XCCONFIG_PATH}={xcode_version}\n"
        );
    }

    println!("cargo:rerun-if-changed={}", xcconfig_path.display());
    println!("cargo:rerun-if-changed=Cargo.toml");
}
