use prpr_l10n::tools::check_langfile;

#[test]
fn check_all() {
    check_langfile(concat!(env!("CARGO_MANIFEST_DIR"), "/locales/")).expect("l10n check failed");
}
