use std::{collections::HashMap, time::Duration};

use crate::{
    config::{Cli, API_URL},
    model::StudyRow,
};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteChart {
    pub id: i32,
    pub file: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicChartMetadata {
    id: i32,
    ranked: bool,
}

#[derive(Debug, Deserialize)]
pub struct PagedResult<T> {
    pub results: Vec<T>,
}

pub async fn send_with_retries(request: impl Fn() -> reqwest::RequestBuilder, retries: usize) -> Result<reqwest::Response, reqwest::Error> {
    let attempts = retries.max(1);
    let mut last_error = None;
    for attempt in 1..=attempts {
        match request().send().await {
            Ok(response) => {
                if !should_retry_status(response.status()) || attempt == attempts {
                    return Ok(response);
                }
                tokio::time::sleep(Duration::from_millis(150 * attempt as u64)).await;
            }
            Err(err) => {
                last_error = Some(err);
                if attempt < attempts {
                    tokio::time::sleep(Duration::from_millis(150 * attempt as u64)).await;
                }
            }
        }
    }
    Err(last_error.expect("at least one request attempt"))
}

pub async fn enrich_listing_metadata(cli: &Cli, rows: &mut [StudyRow]) {
    let ids: Vec<i32> = rows.iter().filter(|row| row.chart_listed.is_none()).map(|row| row.chart_id).collect();
    if ids.is_empty() {
        return;
    }

    let client = match reqwest::Client::builder().timeout(Duration::from_millis(cli.request_timeout_ms)).build() {
        Ok(client) => client,
        Err(err) => {
            eprintln!("skip listing metadata fetch: {err:#}");
            return;
        }
    };

    let mut metadata = HashMap::new();
    for chunk in ids.chunks(80) {
        let ids_str = chunk.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
        let request = || client.get(format!("{API_URL}/chart/multi-get")).query(&[("ids", ids_str.clone())]);
        match send_with_retries(request, cli.retries)
            .await
            .and_then(|response| response.error_for_status())
        {
            Ok(response) => match response.json::<Vec<PublicChartMetadata>>().await {
                Ok(items) => {
                    for item in items {
                        metadata.insert(item.id, item.ranked);
                    }
                }
                Err(err) => eprintln!("skip listing metadata chunk {ids_str}: {err:#}"),
            },
            Err(err) => eprintln!("skip listing metadata chunk {ids_str}: {err:#}"),
        }
    }

    let mut filled = 0;
    for row in rows {
        if let Some(listed) = metadata.get(&row.chart_id) {
            row.chart_listed = Some(*listed);
            filled += 1;
        }
    }
    if filled > 0 {
        println!("listing metadata: backfilled {filled} rows from Phira API");
    }
}

fn should_retry_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}
