use std::collections::BTreeSet;

use anyhow::Result;
use futures_util::{stream, StreamExt};

use crate::study::StudyDataset;
use crate::{chart::analyze_chart, config::Cli, csv::read_existing_csv, download::cached_chart_ids};

pub async fn analyze_samples(cli: &Cli) -> Result<StudyDataset> {
    let charts_dir = cli.root.join("charts");
    let existing_rows = if cli.recompute {
        Vec::new()
    } else {
        read_existing_csv(&cli.root.join("results.csv"))?
    };
    let done: BTreeSet<i32> = existing_rows.iter().map(|row| row.chart_id).collect();
    let mut rows = existing_rows;

    let target_rows = rows.len().max(cli.samples);
    let ids: Vec<i32> = cached_chart_ids(&charts_dir)?
        .into_iter()
        .filter(|id| !done.contains(id))
        .take(target_rows.saturating_sub(rows.len()))
        .collect();
    let jobs = cli.jobs.max(1);

    if !ids.is_empty() {
        println!("analyzing {} cached charts with {jobs} jobs", ids.len());
    }

    let mut analyzed = stream::iter(ids.into_iter().map(|id| {
        let dir = charts_dir.join(id.to_string());
        async move { (id, analyze_chart(id, &dir, cli).await) }
    }))
    .buffer_unordered(jobs);

    while let Some((id, result)) = analyzed.next().await {
        match result {
            Ok(row) => {
                println!("analyzed {id}: raw={:.3} score={:.4} lag={:+.0}ms", row.raw_peak, row.normalized_peak, row.lag_sec * 1000.0);
                rows.push(row);
            }
            Err(err) => eprintln!("skip analysis {id}: {err:#}"),
        }
    }

    rows.sort_by_key(|row| row.chart_id);
    Ok(StudyDataset::new(rows))
}
