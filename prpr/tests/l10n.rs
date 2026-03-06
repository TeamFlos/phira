use prpr_l10n::tools::check_langfile;

#[test]
fn check_all() {
    match check_langfile(concat!(env!("CARGO_MANIFEST_DIR"), "/locales/")) {
        Ok(_) => {}
        Err(e) => panic!("Error: {}", e),
    }
}
