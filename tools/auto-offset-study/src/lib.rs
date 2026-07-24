pub mod analysis;
pub mod api;
pub mod chart;
pub mod config;
pub mod csv;
pub mod download;
pub mod fit;
pub mod model;
pub mod report;
pub mod study;

use anyhow::Result;

use crate::{
    analysis::analyze_samples,
    api::enrich_listing_metadata,
    config::{Cli, REPORT_FILE},
    csv::write_csv,
    download::ensure_samples,
    fit::fit_log_peak_plane,
    report::write_report_html,
};

pub async fn run(cli: Cli) -> Result<()> {
    std::fs::create_dir_all(cli.root.join("charts"))?;

    ensure_samples(&cli).await?;
    let mut dataset = analyze_samples(&cli).await?;
    enrich_listing_metadata(&cli, dataset.rows_mut()).await;

    let plane = fit_log_peak_plane(&dataset)?;
    dataset.set_plane(plane);

    write_csv(&cli.root.join("results.csv"), &dataset, cli.allow_shrink)?;
    write_report_html(&cli.root.join(REPORT_FILE), &dataset)?;

    println!("rows: {}", dataset.len());
    if let Some(plane) = dataset.plane() {
        println!(
            "fit: log_raw = {:.6} + {:.6}*log_note + {:.6}*log_audio (r2={:.4}, rmse={:.4})",
            plane.intercept, plane.note_coef, plane.audio_coef, plane.r2, plane.rmse
        );
    }
    println!("csv: {}", cli.root.join("results.csv").display());
    println!("report: {}", cli.root.join(REPORT_FILE).display());
    Ok(())
}
