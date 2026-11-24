fn git_stdout(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = std::str::from_utf8(&output.stdout).ok()?.trim().to_string();
    if stdout.is_empty() {
        None
    } else {
        Some(stdout)
    }
}

fn version_string(v: &str, g: &str) -> String {
    let build_id = std::env::var("BUILD_ID").unwrap_or_default();
    let build_id = build_id.trim();
    if build_id.is_empty() {
        format!("{} ({})", v, g)
    } else {
        format!("{} ({}, {})", v, g, build_id)
    }
}

fn main() {
    dotenv_build::output(dotenv_build::Config::default()).unwrap();

    // Read at build-script runtime, so restored target caches still pick up the
    // current value instead of whatever was set when the script itself was built.
    println!("cargo:rerun-if-env-changed=BUILD_ID");

    let git_dir = git_stdout(&["rev-parse", "--git-dir"]).unwrap_or_else(|| ".git".to_string());
    println!("cargo:rerun-if-changed={}/HEAD", git_dir);
    println!("cargo:rerun-if-changed={}/packed-refs", git_dir);

    if let Some(ref_path) = git_stdout(&["symbolic-ref", "-q", "HEAD"]) {
        println!("cargo:rerun-if-changed={}/{}", git_dir, ref_path);
    }

    let git_hash = git_stdout(&["rev-parse", "--short=7", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=VERSION_STRING={}", version_string(env!("CARGO_PKG_VERSION"), &git_hash));
}
