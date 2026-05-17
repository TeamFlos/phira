use std::path::Path;

fn main() {
    let target = std::env::var("TARGET").unwrap();

    // The historical layout: prebuilt static libraries live under
    // `prpr-avc/static-lib/<TARGET>/`.
    let libs_dir = std::env::var("PRPR_AVC_LIBS")
        .unwrap_or_else(|_| format!("{}/static-lib", std::env::var("CARGO_MANIFEST_DIR").unwrap()));
    let libs_path = Path::new(&libs_dir).join(&target);
    println!("cargo:rustc-link-search={}", libs_path.display());
    println!("cargo:rerun-if-changed={}", libs_path.display());

    // zlib is shared by every prebuilt distribution we use.
    println!("cargo:rustc-link-lib=z");

    // Local dev shortcut for Intel Macs: prebuilt static libs aren't shipped
    // for x86_64-apple-darwin, so link against Homebrew's ffmpeg@4 dylibs
    // which still expose the pre-ffmpeg5 API (e.g. `swr_alloc_set_opts`).
    if target == "x86_64-apple-darwin" {
        let hb_prefix = std::env::var("HOMEBREW_PREFIX")
            .unwrap_or_else(|_| if Path::new("/opt/homebrew").exists() { "/opt/homebrew".into() } else { "/usr/local".into() });
        for candidate in ["ffmpeg@4", "ffmpeg"] {
            let p = format!("{}/opt/{}/lib", hb_prefix, candidate);
            if Path::new(&p).exists() {
                println!("cargo:rustc-link-search=native={}", p);
                println!("cargo:rerun-if-changed={}", p);
                break;
            }
        }
    }

    if let Ok(extra) = std::env::var("PRPR_AVC_EXTRA_LIBS") {
        for name in extra.split(',').filter(|s| !s.is_empty()) {
            println!("cargo:rustc-link-lib={}", name);
        }
    }
}
