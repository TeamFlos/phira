fn main() {
    // Deeplink support
    for arg in std::env::args().skip(1) {
        let arg = arg.trim().to_owned();
        if arg.contains("://") {
            phira::deeplink::set_deeplink(arg);
            break;
        }
    }
    phira::quad_main();
}
