use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use prpr::{
    bin::{BinaryReader, BinaryWriter},
    core::ChartExtra,
    fs::FileSystem,
    info::ChartFormat,
    parse::{parse_pec, parse_phigros, parse_rpe},
};
use std::{
    any::Any,
    fs::File,
    io::{BufWriter, Cursor},
};

const HELP: &'static str = "
Usage: prpr-pbc [options] input output

Options:
    -h, --help  Display this message
";

struct DummyFileSystem;
#[async_trait]
impl FileSystem for DummyFileSystem {
    async fn load_file(&mut self, _path: &str) -> Result<Vec<u8>> {
        bail!("Not implemented");
    }
    async fn exists(&mut self, _path: &str) -> Result<bool> {
        Ok(false)
    }
    fn list_root(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }
    fn clone_box(&self) -> Box<dyn FileSystem> {
        Box::new(DummyFileSystem)
    }
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

fn main() -> Result<()> {
    let mut iter = std::env::args().skip(1);
    let mut input = None;
    let mut output = None;
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{}", HELP.trim());
                return Ok(());
            }
            _ => {
                if input.is_none() {
                    input = Some(arg);
                } else if output.is_none() {
                    output = Some(arg);
                } else {
                    bail!("Too many arguments");
                }
            }
        }
    }

    let input = input.ok_or_else(|| anyhow!("Missing input"))?;
    let output = output.ok_or_else(|| anyhow!("Missing output"))?;

    let bytes = std::fs::read(input).context("Failed to read chart")?;
    let format = if let Ok(text) = String::from_utf8(bytes.clone()) {
        if text.starts_with('{') {
            if text.contains("\"META\"") {
                ChartFormat::Rpe
            } else {
                ChartFormat::Pgr
            }
        } else {
            ChartFormat::Pec
        }
    } else {
        ChartFormat::Pbc
    };

    let mut fs = Box::new(DummyFileSystem);
    let extra = ChartExtra::default();
    let mut chart = match format {
        ChartFormat::Rpe => pollster::block_on(parse_rpe(&String::from_utf8_lossy(&bytes), fs.as_mut(), extra)),
        ChartFormat::Pgr => parse_phigros(&String::from_utf8_lossy(&bytes), extra),
        ChartFormat::Pec => parse_pec(&String::from_utf8_lossy(&bytes), extra),
        ChartFormat::Pbc => {
            let mut r = BinaryReader::new(Cursor::new(&bytes));
            r.read()
        }
    }?;

    let output = BufWriter::new(File::create(output)?);
    let mut w = BinaryWriter::new(output);
    w.write(&mut chart)?;

    Ok(())
}
