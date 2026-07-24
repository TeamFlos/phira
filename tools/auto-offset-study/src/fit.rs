use anyhow::{bail, Context, Result};

use crate::model::{FittedPlane, StudyRow};

pub fn fit_log_peak_plane(rows: impl AsRef<[StudyRow]>) -> Result<FittedPlane> {
    let rows = rows.as_ref();
    let rows: Vec<&StudyRow> = rows
        .iter()
        .filter(|row| row.raw_peak > 0.0 && row.note_energy > 0.0 && row.audio_energy > 0.0)
        .collect();
    if rows.len() < 3 {
        bail!("need at least 3 valid rows to fit log peak plane")
    }

    let mut xtx = [[0.0; 3]; 3];
    let mut xtz = [0.0; 3];
    let mut zs = Vec::with_capacity(rows.len());
    for row in &rows {
        let x = row.note_energy.log10();
        let y = row.audio_energy.log10();
        let z = row.raw_peak.log10();
        let v = [1.0, x, y];
        for i in 0..3 {
            xtz[i] += v[i] * z;
            for j in 0..3 {
                xtx[i][j] += v[i] * v[j];
            }
        }
        zs.push(z);
    }

    let coef = solve_3x3(xtx, xtz).context("failed to fit log peak plane")?;
    let plane = FittedPlane {
        intercept: coef[0],
        note_coef: coef[1],
        audio_coef: coef[2],
        r2: 0.0,
        rmse: 0.0,
    };

    let mean_z = zs.iter().sum::<f64>() / zs.len() as f64;
    let mut ss_res = 0.0;
    let mut ss_tot = 0.0;
    for (row, z) in rows.iter().zip(zs) {
        let residual = z - plane.predict_log_raw(row.note_energy, row.audio_energy);
        ss_res += residual * residual;
        ss_tot += (z - mean_z).powi(2);
    }

    Ok(FittedPlane {
        r2: if ss_tot > 0.0 { 1.0 - ss_res / ss_tot } else { 1.0 },
        rmse: (ss_res / rows.len() as f64).sqrt(),
        ..plane
    })
}

fn solve_3x3(mut a: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<[f64; 3]> {
    for col in 0..3 {
        let mut pivot = col;
        for row in col + 1..3 {
            if a[row][col].abs() > a[pivot][col].abs() {
                pivot = row;
            }
        }
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);

        let denom = a[col][col];
        for value in a[col].iter_mut().skip(col) {
            *value /= denom;
        }
        b[col] /= denom;

        for row in 0..3 {
            if row == col {
                continue;
            }
            let factor = a[row][col];
            let pivot_row = a[col];
            for (value, &pivot_value) in a[row].iter_mut().zip(pivot_row.iter()).skip(col) {
                *value -= factor * pivot_value;
            }
            b[row] -= factor * b[col];
        }
    }
    Some(b)
}
