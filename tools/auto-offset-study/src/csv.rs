use std::{
    fs::{self, File},
    io::Write,
    path::Path,
};

use anyhow::{bail, Result};

use crate::model::StudyRow;

pub fn read_existing_csv(path: &Path) -> Result<Vec<StudyRow>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path)?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let Some(header_line) = lines.next() else {
        return Ok(Vec::new());
    };
    let headers = split_csv_line(header_line);
    let mut rows = Vec::new();
    for line in lines {
        let cols = split_csv_line(line);
        if let Some(row) = parse_existing_row(&headers, &cols)? {
            rows.push(row);
        }
    }
    Ok(rows)
}

pub fn write_csv(path: &Path, rows: impl AsRef<[StudyRow]>, allow_shrink: bool) -> Result<()> {
    let rows = rows.as_ref();
    if path.exists() && !allow_shrink {
        let existing_rows = count_csv_data_rows(path)?;
        if rows.len() < existing_rows {
            bail!(
                "refusing to overwrite {} rows in {} with {} rows; rerun with --allow-shrink if this is intentional",
                existing_rows,
                path.display(),
                rows.len()
            );
        }
    }
    let mut file = File::create(path)?;
    writeln!(file, "{}", StudyRow::header())?;
    for row in rows {
        writeln!(file, "{}", row.to_csv())?;
    }
    Ok(())
}

fn parse_existing_row(headers: &[String], cols: &[String]) -> Result<Option<StudyRow>> {
    let Some(chart_id) = get_col(headers, cols, "chart_id").and_then(|value| value.parse().ok()) else {
        return Ok(None);
    };
    let Some(note_energy) = parse_col_f64(headers, cols, "note_energy")? else {
        return Ok(None);
    };
    let Some(audio_energy) = parse_col_f64(headers, cols, "audio_energy")? else {
        return Ok(None);
    };

    let normalized_peak = first_f64(headers, cols, &["preprocessed_normalized_peak", "normalized_peak"])?.unwrap_or(0.0);
    let row = StudyRow {
        chart_id,
        chart_name: get_col(headers, cols, "chart_name").unwrap_or_default().to_owned(),
        notes: get_col(headers, cols, "notes").and_then(|value| value.parse().ok()).unwrap_or(0),
        duration_sec: parse_col_f64(headers, cols, "duration_sec")?.unwrap_or(0.0),
        search_center_sec: parse_col_f64(headers, cols, "search_center_sec")?.unwrap_or(0.0),
        suggested_offset_sec: first_f64(headers, cols, &["preprocessed_suggested_offset_sec", "suggested_offset_sec"])?.unwrap_or(0.0),
        lag_sec: first_f64(headers, cols, &["preprocessed_lag_sec", "lag_sec"])?.unwrap_or(0.0),
        raw_peak: first_f64(headers, cols, &["preprocessed_raw_peak", "raw_peak"])?.unwrap_or(0.0),
        note_energy,
        audio_energy,
        normalized_peak,
        reliable: get_col(headers, cols, "reliable")
            .and_then(|value| value.parse().ok())
            .unwrap_or(normalized_peak > 0.2),
        drag_ratio: parse_col_f64(headers, cols, "drag_ratio")?.unwrap_or(0.0),
        chart_listed: get_col(headers, cols, "chart_listed").and_then(parse_optional_bool),
    };
    Ok(Some(row))
}

fn first_f64(headers: &[String], cols: &[String], names: &[&str]) -> Result<Option<f64>> {
    for name in names {
        if let Some(value) = parse_col_f64(headers, cols, name)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn parse_col_f64(headers: &[String], cols: &[String], name: &str) -> Result<Option<f64>> {
    get_col(headers, cols, name).map_or(Ok(None), |value| if value.is_empty() { Ok(None) } else { Ok(Some(value.parse()?)) })
}

fn get_col<'a>(headers: &[String], cols: &'a [String], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .position(|header| header == name)
        .and_then(|index| cols.get(index))
        .map(String::as_str)
}

fn split_csv_line(line: &str) -> Vec<String> {
    let mut cols = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => cols.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    cols.push(cur);
    cols
}

fn parse_optional_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn count_csv_data_rows(path: &Path) -> Result<usize> {
    Ok(fs::read_to_string(path)?.lines().skip(1).filter(|line| !line.trim().is_empty()).count())
}
