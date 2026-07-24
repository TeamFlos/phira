use anyhow::{Context, Result};

use crate::model::{FittedPlane, StudyRow};

#[derive(Debug, Clone)]
pub struct StudyDataset {
    rows: Vec<StudyRow>,
    plane: Option<FittedPlane>,
}

#[derive(Debug, Clone)]
pub struct Histogram {
    pub centers: Vec<f64>,
    pub series: Vec<HistogramSeries>,
}

#[derive(Debug, Clone)]
pub struct HistogramSeries {
    pub key: &'static str,
    pub label: &'static str,
    pub counts: Vec<usize>,
    pub mean_score: Vec<Option<f64>>,
    pub mean_abs_lag_ms: Vec<Option<f64>>,
}

#[derive(Debug, Clone)]
pub struct EnergySpace {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    pub residual: Vec<f64>,
    pub text: Vec<String>,
    pub plane_x: Vec<Vec<f64>>,
    pub plane_y: Vec<Vec<f64>>,
    pub plane_z: Vec<Vec<f64>>,
    pub color_abs: f64,
}

impl StudyDataset {
    pub fn new(rows: Vec<StudyRow>) -> Self {
        Self { rows, plane: None }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn rows(&self) -> &[StudyRow] {
        &self.rows
    }

    pub fn rows_mut(&mut self) -> &mut [StudyRow] {
        &mut self.rows
    }

    pub fn set_plane(&mut self, plane: FittedPlane) {
        self.plane = Some(plane);
    }

    pub fn plane(&self) -> Option<FittedPlane> {
        self.plane
    }

    pub fn summary_line(&self) -> String {
        match self.plane {
            Some(plane) => format!(
                "rows: {}; fit log_raw = {:.3} + {:.3} log_note + {:.3} log_audio, R2={:.3}, RMSE={:.3}",
                self.rows.len(),
                plane.intercept,
                plane.note_coef,
                plane.audio_coef,
                plane.r2,
                plane.rmse
            ),
            None => format!("rows: {}", self.rows.len()),
        }
    }

    pub fn score_distribution(&self, bin_size: f64) -> Histogram {
        let bins = (1.0 / bin_size).ceil() as usize;
        self.grouped_histogram(bins, 0.0, bin_size, |row| {
            let value = row.normalized_peak;
            if !value.is_finite() {
                return None;
            }
            Some(((value.clamp(0.0, 1.0) / bin_size).floor() as usize).min(bins - 1))
        })
    }

    pub fn lag_distribution(&self, bin_size_ms: f64, range_ms: f64) -> Histogram {
        let bins = ((range_ms * 2.0) / bin_size_ms).ceil() as usize;
        self.grouped_histogram(bins, -range_ms, bin_size_ms, move |row| {
            let value = row.lag_sec * 1000.0;
            if !value.is_finite() || value < -range_ms || value > range_ms {
                return None;
            }
            Some((((value + range_ms) / bin_size_ms).floor() as usize).min(bins - 1))
        })
    }

    pub fn energy_space(&self) -> Result<EnergySpace> {
        let plane = self.plane.context("fitted plane is missing from study dataset")?;
        let energy_points: Vec<&StudyRow> = self
            .rows
            .iter()
            .filter(|row| row.raw_peak > 0.0 && row.note_energy > 0.0 && row.audio_energy > 0.0)
            .collect();
        let x: Vec<f64> = energy_points.iter().map(|row| row.note_energy.log10()).collect();
        let y: Vec<f64> = energy_points.iter().map(|row| row.audio_energy.log10()).collect();
        let z: Vec<f64> = energy_points.iter().map(|row| row.raw_peak.log10()).collect();
        let residual: Vec<f64> = energy_points
            .iter()
            .map(|row| row.raw_peak.log10() - plane.predict_log_raw(row.note_energy, row.audio_energy))
            .collect();
        let text: Vec<String> = energy_points
            .iter()
            .map(|row| {
                format!(
                    "#{} {}<br>notes: {}<br>listing: {}<br>lag: {:+.0}ms<br>score: {:.4}<br>drag ratio: {:.3}<br>raw: {:.3}",
                    row.chart_id,
                    row.chart_name,
                    row.notes,
                    row.listing_label(),
                    row.lag_sec * 1000.0,
                    row.normalized_peak,
                    row.drag_ratio,
                    row.raw_peak
                )
            })
            .collect();
        let (x_min, x_max) = min_max(&x);
        let (y_min, y_max) = min_max(&y);
        let plane_x = vec![vec![x_min, x_max], vec![x_min, x_max]];
        let plane_y = vec![vec![y_min, y_min], vec![y_max, y_max]];
        let plane_z: Vec<Vec<f64>> = plane_y
            .iter()
            .enumerate()
            .map(|(row_index, row)| {
                row.iter()
                    .enumerate()
                    .map(|(col_index, &audio)| plane.intercept + plane.note_coef * plane_x[row_index][col_index] + plane.audio_coef * audio)
                    .collect()
            })
            .collect();
        let color_abs = percentile_abs(&residual, 0.95).max(0.05);

        Ok(EnergySpace {
            x,
            y,
            z,
            residual,
            text,
            plane_x,
            plane_y,
            plane_z,
            color_abs,
        })
    }

    fn grouped_histogram<F>(&self, bins: usize, start: f64, bin_size: f64, mut indexer: F) -> Histogram
    where
        F: FnMut(&StudyRow) -> Option<usize>,
    {
        let mut series = HistogramBucket::all()
            .iter()
            .copied()
            .map(|bucket| HistogramAccumulator::new(bucket, bins))
            .collect::<Vec<_>>();

        for row in &self.rows {
            let Some(index) = indexer(row) else {
                continue;
            };
            for item in &mut series {
                if item.bucket.includes(row.chart_listed) {
                    item.add(index, row.normalized_peak, row.lag_sec);
                }
            }
        }

        let centers = (0..bins).map(|index| start + (index as f64 + 0.5) * bin_size).collect();
        Histogram {
            centers,
            series: series.into_iter().map(HistogramAccumulator::finish).collect(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum HistogramBucket {
    All,
    Listed,
    Unlisted,
    Unknown,
}

impl HistogramBucket {
    fn all() -> &'static [Self] {
        &[Self::All, Self::Listed, Self::Unlisted, Self::Unknown]
    }

    fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Listed => "listed",
            Self::Unlisted => "unlisted",
            Self::Unknown => "unknown",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Listed => "listed",
            Self::Unlisted => "unlisted",
            Self::Unknown => "unknown",
        }
    }

    fn includes(self, chart_listed: Option<bool>) -> bool {
        match self {
            Self::All => true,
            Self::Listed => chart_listed == Some(true),
            Self::Unlisted => chart_listed == Some(false),
            Self::Unknown => chart_listed.is_none(),
        }
    }
}

struct HistogramAccumulator {
    bucket: HistogramBucket,
    counts: Vec<usize>,
    score_sums: Vec<f64>,
    score_counts: Vec<usize>,
    abs_lag_sums_ms: Vec<f64>,
    abs_lag_counts: Vec<usize>,
}

impl HistogramAccumulator {
    fn new(bucket: HistogramBucket, bins: usize) -> Self {
        Self {
            bucket,
            counts: vec![0; bins],
            score_sums: vec![0.0; bins],
            score_counts: vec![0; bins],
            abs_lag_sums_ms: vec![0.0; bins],
            abs_lag_counts: vec![0; bins],
        }
    }

    fn add(&mut self, index: usize, normalized_score: f64, lag_sec: f64) {
        self.counts[index] += 1;
        if normalized_score.is_finite() {
            self.score_sums[index] += normalized_score;
            self.score_counts[index] += 1;
        }
        if lag_sec.is_finite() {
            self.abs_lag_sums_ms[index] += lag_sec.abs() * 1000.0;
            self.abs_lag_counts[index] += 1;
        }
    }

    fn finish(self) -> HistogramSeries {
        let mean_score = self
            .score_sums
            .into_iter()
            .zip(self.score_counts)
            .map(|(sum, count)| (count > 0).then_some(sum / count as f64))
            .collect();
        let mean_abs_lag_ms = self
            .abs_lag_sums_ms
            .into_iter()
            .zip(self.abs_lag_counts)
            .map(|(sum, count)| (count > 0).then_some(sum / count as f64))
            .collect();
        HistogramSeries {
            key: self.bucket.key(),
            label: self.bucket.label(),
            counts: self.counts,
            mean_score,
            mean_abs_lag_ms,
        }
    }
}

impl AsRef<[StudyRow]> for StudyDataset {
    fn as_ref(&self) -> &[StudyRow] {
        &self.rows
    }
}

impl AsMut<[StudyRow]> for StudyDataset {
    fn as_mut(&mut self) -> &mut [StudyRow] {
        &mut self.rows
    }
}

fn min_max(values: &[f64]) -> (f64, f64) {
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !min.is_finite() || !max.is_finite() {
        (0.0, 1.0)
    } else if (max - min).abs() < 1e-9 {
        (min, min + 1.0)
    } else {
        (min, max)
    }
}

fn percentile_abs(values: &[f64], percentile: f64) -> f64 {
    let mut values: Vec<f64> = values.iter().map(|value| value.abs()).filter(|value| value.is_finite()).collect();
    if values.is_empty() {
        return 0.3;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let index = ((values.len() - 1) as f64 * percentile.clamp(0.0, 1.0)).round() as usize;
    values[index]
}
