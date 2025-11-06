use std::path::Path;

fn main() {
    for lib in ["libavformat", "libavcodec", "libavutil", "libswscale", "libswresample"] {
        pkg_config::Config::new().statik(false).probe(lib);
    }
}
