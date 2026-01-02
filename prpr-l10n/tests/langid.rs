use prpr_l10n::Lazy;

#[test]
fn check_langid() {
    // Lang ID is illegal if panicked
    Lazy::force(&prpr_l10n::LANG_IDENTS);
}
