use std::{fs, path::Path};

use anyhow::Result;

use crate::study::{Histogram, StudyDataset};

const PLOTLY_VERSION: &str = "2.35.2";

struct PlotSection {
    id: &'static str,
    controls_html: Option<String>,
    script: String,
}

pub fn write_report_html(path: &Path, dataset: &StudyDataset) -> Result<()> {
    let sections = build_sections(dataset)?;
    let html = render_page(dataset, &sections)?;
    fs::write(path, html)?;
    Ok(())
}

fn build_sections(dataset: &StudyDataset) -> Result<Vec<PlotSection>> {
    let score = dataset.score_distribution(0.025);
    let lag = dataset.lag_distribution(5.0, 300.0);
    let energy = dataset.energy_space()?;

    Ok(vec![
        PlotSection {
            id: "score-distribution",
            controls_html: Some(histogram_controls("score-distribution", &score)),
            script: histogram_script(
                HistogramPlot {
                    id: "score-distribution",
                    title: "Preprocessed match score distribution by listing status",
                    x_title: "match score",
                    value_name: "score bin",
                    bar_width: 0.023,
                    x_range: None,
                    color_mode: HistogramColorMode::MeanAbsLag,
                },
                &score,
            )?,
        },
        PlotSection {
            id: "lag-distribution",
            controls_html: Some(histogram_controls("lag-distribution", &lag)),
            script: histogram_script(
                HistogramPlot {
                    id: "lag-distribution",
                    title: "Recommended lag change distribution by listing status",
                    x_title: "suggested offset - search center (ms)",
                    value_name: "lag bin ms",
                    bar_width: 4.6,
                    x_range: Some("[-300, 300]"),
                    color_mode: HistogramColorMode::MeanScore,
                },
                &lag,
            )?,
        },
        PlotSection {
            id: "energy-space",
            controls_html: None,
            script: energy_space_script(&energy)?,
        },
    ])
}

fn render_page(dataset: &StudyDataset, sections: &[PlotSection]) -> Result<String> {
    let plot_blocks = sections
        .iter()
        .map(|section| {
            let mut block = String::new();
            if let Some(controls_html) = &section.controls_html {
                block.push_str(controls_html);
                block.push('\n');
            }
            block.push_str(&format!(r#"    <div id="{}" class="plot"></div>"#, section.id));
            block
        })
        .collect::<Vec<_>>()
        .join("\n    <div class=\"spacer\"></div>\n");
    let scripts = sections.iter().map(|section| section.script.as_str()).collect::<Vec<_>>().join("\n\n");

    Ok(format!(
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>Auto-offset study report</title>
  <script src="https://cdn.plot.ly/plotly-{plotly_version}.min.js"></script>
  <style>
    html, body {{ margin: 0; background: #f6f5f1; color: #242424; font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
    header {{ padding: 20px 28px 12px; }}
    h1 {{ margin: 0 0 6px; font-size: 22px; font-weight: 650; }}
    .summary {{ margin: 0; color: #555; font-size: 14px; }}
    section {{ padding: 0 20px 28px; }}
    .plot-controls {{ display: flex; gap: 6px; align-items: center; padding: 0 0 8px; }}
    .plot-tab {{ appearance: none; border: 1px solid #bfc4ca; background: #fff; color: #242424; padding: 5px 11px; border-radius: 4px; font: inherit; font-size: 13px; cursor: pointer; }}
    .plot-tab:hover {{ background: #f0f4f8; }}
    .plot-tab.is-active {{ background: #26384d; border-color: #26384d; color: #fff; }}
    .plot {{ width: 100%; height: 72vh; min-height: 520px; background: #fff; border: 1px solid #d9d7cf; box-sizing: border-box; }}
    .spacer {{ height: 20px; }}
  </style>
</head>
<body>
  <header>
    <h1>Auto-offset study report</h1>
    <p class="summary">{summary}</p>
  </header>
  <section>
{plot_blocks}
  </section>
  <script>
    function setHistogramTab(plotId, index, count) {{
      const visible = Array.from({{ length: count }}, (_, traceIndex) => traceIndex === index);
      Plotly.restyle(plotId, {{ visible: visible }});
      document.querySelectorAll('.plot-tab[data-plot="' + plotId + '"]').forEach((button) => {{
        button.classList.toggle('is-active', Number(button.dataset.trace) === index);
      }});
    }}

{scripts}
  </script>
</body>
</html>
"#,
        plotly_version = PLOTLY_VERSION,
        summary = html_escape(&dataset.summary_line()),
        plot_blocks = plot_blocks,
        scripts = scripts,
    ))
}

fn histogram_controls(id: &str, hist: &Histogram) -> String {
    let count = hist.series.len();
    let buttons = hist
        .series
        .iter()
        .enumerate()
        .map(|(index, series)| {
            format!(
                r#"<button class="plot-tab{}" type="button" data-plot="{}" data-trace="{}" onclick="setHistogramTab('{}', {}, {})">{}</button>"#,
                if index == 0 { " is-active" } else { "" },
                html_escape(id),
                index,
                js_escape(id),
                index,
                count,
                html_escape(series.label),
            )
        })
        .collect::<Vec<_>>()
        .join("\n      ");
    format!(
        r#"    <div class="plot-controls">
      {buttons}
    </div>"#,
        buttons = buttons,
    )
}

struct HistogramPlot<'a> {
    id: &'a str,
    title: &'a str,
    x_title: &'a str,
    value_name: &'a str,
    bar_width: f64,
    x_range: Option<&'a str>,
    color_mode: HistogramColorMode,
}

#[derive(Debug, Clone, Copy)]
enum HistogramColorMode {
    MeanScore,
    MeanAbsLag,
}

fn histogram_script(plot: HistogramPlot<'_>, hist: &Histogram) -> Result<String> {
    let traces = hist
        .series
        .iter()
        .map(|series| histogram_trace(&plot, hist, series))
        .collect::<Result<Vec<_>>>()?
        .join(",\n");
    let xaxis_range = plot.x_range.map_or(String::new(), |range| format!(", range: {range}"));

    Ok(format!(
        r#"    Plotly.newPlot('{id}', [
{traces}
    ], {{
      title: '{title}',
      xaxis: {{ title: '{x_title}'{xaxis_range} }},
      yaxis: {{ title: 'chart count', rangemode: 'tozero' }},
      bargap: 0.03,
      showlegend: false,
      margin: {{ l: 58, r: 24, b: 54, t: 54 }}
    }}, {{ responsive: true }});"#,
        id = plot.id,
        traces = traces,
        title = js_escape(plot.title),
        x_title = js_escape(plot.x_title),
        xaxis_range = xaxis_range,
    ))
}

fn histogram_trace(plot: &HistogramPlot<'_>, hist: &Histogram, series: &crate::study::HistogramSeries) -> Result<String> {
    let marker = match plot.color_mode {
        HistogramColorMode::MeanScore => format!(
            "{{ color: {}, colorscale: {}, cmin: 0.5, cmax: 1.5, colorbar: {{ title: 'avg score' }}, line: {{ color: '#4f4f4f', width: 0.35 }} }}",
            serde_json::to_string(&series.mean_score)?,
            mean_score_colorscale()
        ),
        HistogramColorMode::MeanAbsLag => format!(
            "{{ color: {}, colorscale: {}, cmin: 0, cmax: 50, colorbar: {{ title: 'avg |lag| ms' }}, line: {{ color: '#4f4f4f', width: 0.35 }} }}",
            serde_json::to_string(&series.mean_abs_lag_ms)?,
            abs_lag_colorscale()
        ),
    };
    let hovertemplate = match plot.color_mode {
        HistogramColorMode::MeanScore => format!(
            "{}<br>{}: %{{x:.3f}}<br>charts: %{{y}}<br>avg score: %{{marker.color:.4f}}<extra></extra>",
            js_escape(series.label),
            js_escape(plot.value_name)
        ),
        HistogramColorMode::MeanAbsLag => format!(
            "{}<br>{}: %{{x:.3f}}<br>charts: %{{y}}<br>avg |lag|: %{{marker.color:.1f}}ms<extra></extra>",
            js_escape(series.label),
            js_escape(plot.value_name)
        ),
    };

    Ok(format!(
        "      {{ type: 'bar', name: '{}', x: {}, y: {}, width: {}, marker: {}, visible: {}, hovertemplate: '{}' }}",
        js_escape(series.label),
        serde_json::to_string(&hist.centers)?,
        serde_json::to_string(&series.counts)?,
        plot.bar_width,
        marker,
        series.key == "all",
        hovertemplate,
    ))
}

fn mean_score_colorscale() -> &'static str {
    "[[0.00, '#313695'], [0.20, '#4575b4'], [0.35, '#74add1'], [0.50, '#ffffbf'], [0.65, '#fdae61'], [0.80, '#f46d43'], [1.00, '#a50026']]"
}

fn abs_lag_colorscale() -> &'static str {
    "[[0.00, '#f7fbff'], [0.18, '#deebf7'], [0.36, '#9ecae1'], [0.55, '#4292c6'], [0.74, '#08519c'], [1.00, '#08306b']]"
}

fn energy_space_script(energy: &crate::study::EnergySpace) -> Result<String> {
    Ok(format!(
        r#"    const scatter = {{
      type: 'scatter3d', mode: 'markers', name: 'charts', x: {x}, y: {y}, z: {z}, text: {text},
      marker: {{
        size: 3.5, color: {residual}, colorscale: 'RdBu', reversescale: true,
        cmin: -{color_abs}, cmax: {color_abs}, cmid: 0,
        colorbar: {{ title: 'log(raw / fitted)' }}, opacity: 0.78
      }},
      hovertemplate: '%{{text}}<br>log note: %{{x:.3f}}<br>log audio: %{{y:.3f}}<br>log raw: %{{z:.3f}}<br>residual: %{{marker.color:.4f}}<extra></extra>'
    }};
    const fittedPlane = {{
      type: 'surface', name: 'fitted plane', x: {plane_x}, y: {plane_y}, z: {plane_z},
      showscale: false, opacity: 0.34,
      colorscale: [[0, 'rgba(236, 183, 42, 0.34)'], [1, 'rgba(236, 183, 42, 0.34)']],
      hovertemplate: 'fitted plane<br>log note=%{{x:.3f}}<br>log audio=%{{y:.3f}}<br>log raw=%{{z:.3f}}<extra></extra>'
    }};
    Plotly.newPlot('energy-space', [scatter, fittedPlane], {{
      title: 'Corrected raw peak vs note/audio energy',
      scene: {{
        xaxis: {{ title: 'log10(note energy)' }},
        yaxis: {{ title: 'log10(audio energy)' }},
        zaxis: {{ title: 'log10(raw peak)' }}
      }},
      margin: {{ l: 0, r: 0, b: 0, t: 54 }}
    }}, {{ responsive: true }});"#,
        x = serde_json::to_string(&energy.x)?,
        y = serde_json::to_string(&energy.y)?,
        z = serde_json::to_string(&energy.z)?,
        text = serde_json::to_string(&energy.text)?,
        residual = serde_json::to_string(&energy.residual)?,
        color_abs = energy.color_abs,
        plane_x = serde_json::to_string(&energy.plane_x)?,
        plane_y = serde_json::to_string(&energy.plane_y)?,
        plane_z = serde_json::to_string(&energy.plane_z)?,
    ))
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn js_escape(value: &str) -> String {
    value.replace('\\', "").replace('\'', "\\'")
}
