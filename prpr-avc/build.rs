use std::{path::Path, time};

const EXPECTED_LIB_VERSION: &str = "20260730_v0";

fn main() {
    let libs_dir = std::env::var("PRPR_AVC_LIBS").unwrap_or_else(|_| format!("{}/static-lib", std::env::var("CARGO_MANIFEST_DIR").unwrap()));
    let libs_dir = Path::new(&libs_dir);
    let target_dir = libs_dir.join(std::env::var("TARGET").unwrap());
    let libs_path_str = target_dir.display();
    println!("cargo:rustc-link-search={libs_path_str}");
    println!("cargo:rustc-link-lib=z");
    println!("cargo:rerun-if-changed={libs_path_str}");
    check_static_lib_version(libs_dir);
    check_upstream_version();
}

fn check_static_lib_version(libs_dir: &Path) {
    let version_file = libs_dir.join(".version");
    println!("cargo:rerun-if-changed={}", version_file.display());

    if !version_file.exists() {
        panic!(
            "\n\n\
             [prpr-avc] static-lib/.version does not exist.\n\
             Run: ./pull-static-lib.sh\n\
             Expected Version: {EXPECTED_LIB_VERSION}\n"
        );
    }

    let actual_version = std::fs::read_to_string(&version_file).unwrap().trim().to_string();

    if actual_version != EXPECTED_LIB_VERSION {
        panic!(
            "\n\n\
             [prpr-avc] Static lib version mismatch!\n\
             Expected: {EXPECTED_LIB_VERSION}\n\
             Current: {actual_version}\n\
             Please run: ./pull-static-lib.sh\n"
        );
    }
}

const GITHUB_API: &str = "https://api.github.com/repos/TeamFlos/prpr-avc-ffmpeg/releases/latest";
const CHECK_TIMEOUT: time::Duration = time::Duration::from_secs(5);

fn check_upstream_version() {
    let agent = ureq::AgentBuilder::new().timeout(CHECK_TIMEOUT).build();
    match agent.get(GITHUB_API).set("User-Agent", "prpr-avc-build-script").call() {
        Ok(response) => match response.into_json::<serde_json::Value>() {
            Ok(json) => {
                if let Some(latest) = json["tag_name"].as_str() {
                    if latest != EXPECTED_LIB_VERSION {
                        println!(
                            "cargo:warning=[prpr-avc] Newer static lib version available: {} \
                             (currently pinned: {}). Consider updating build.rs",
                            latest, EXPECTED_LIB_VERSION
                        );
                    }
                }
            }
            Err(e) => {
                println!("cargo:warning=[prpr-avc] Failed to parse GitHub API response: {e}");
            }
        },
        Err(ureq::Error::Transport(e)) => {
            println!("cargo:warning=[prpr-avc] Unable to check upstream version (offline or timeout): {e}");
        }
        Err(e) => {
            println!("cargo:warning=[prpr-avc] Failed to check upstream version: {e}");
        }
    }
}
