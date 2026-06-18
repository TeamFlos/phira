use prpr_l10n::tools::check_langfile;

#[test]
fn check_all() {
    let root_path = concat!(env!("CARGO_MANIFEST_DIR"), "/locales/");
    let result = check_langfile(root_path);
    if let Err(e) = result {
        panic!("l10n check failed (Root: {root_path})\n{e}");
    }
}
