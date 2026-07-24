use clap::Parser;
use std::path::PathBuf;

pub const API_URL: &str = "https://phira.5wyxi.com";
pub const DEFAULT_ROOT: &str = "data/auto-offset-study";
pub const REPORT_FILE: &str = "study-report.html";

#[derive(Parser)]
#[command(name = "prpr-auto-offset-study")]
#[command(about = "Download chart samples and study preprocessed auto-offset scores")]
pub struct Cli {
    #[arg(long, default_value = DEFAULT_ROOT)]
    pub root: PathBuf,
    #[arg(short, long, default_value_t = 300)]
    pub samples: usize,
    #[arg(long)]
    pub download: bool,
    #[arg(long, default_value_t = 20)]
    pub pages: u64,
    #[arg(long, default_value_t = 30)]
    pub page_num: u64,
    #[arg(long, default_value = "-updated")]
    pub order: String,
    #[arg(long, default_value_t = 0.30)]
    pub range: f64,
    #[arg(long, default_value_t = 0.005)]
    pub interval: f64,
    #[arg(long, default_value_t = 0.02)]
    pub blur_sigma: f64,
    #[arg(long)]
    pub recompute: bool,
    #[arg(long, default_value_t = default_jobs())]
    pub jobs: usize,
    #[arg(long)]
    pub allow_shrink: bool,
    #[arg(long, default_value_t = 8000)]
    pub request_timeout_ms: u64,
    #[arg(long, default_value_t = 10)]
    pub retries: usize,
}

fn default_jobs() -> usize {
    std::thread::available_parallelism().map_or(4, usize::from).max(1)
}
