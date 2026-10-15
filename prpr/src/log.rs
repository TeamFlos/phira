//! Logging, crash capture, retention, and diagnostic export utilities.

use chrono::{Duration, Local, NaiveDate};
use colored::Colorize;
use miniquad::{debug, error, info, trace, warn};
use std::{
    fs::{self, OpenOptions},
    io::{self, Seek, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use tracing::{field::Visit, Level, Subscriber};
use tracing_subscriber::{prelude::*, EnvFilter, Layer};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

pub const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024;
const LOG_FILE_FORMAT: &str = "%Y-%m-%d.log";

#[derive(Clone, Debug)]
pub struct CrashRecord {
    pub summary: String,
    pub details: String,
}

pub struct LogManager {
    dir: PathBuf,
    write_lock: Mutex<()>,
}

impl LogManager {
    pub fn new(dir: impl AsRef<Path>) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            write_lock: Mutex::new(()),
        })
    }

    pub fn path_for_date(&self, date: NaiveDate) -> PathBuf {
        self.dir.join(date.format(LOG_FILE_FORMAT).to_string())
    }

    pub fn append_line(&self, line: &str) -> io::Result<()> {
        self.append_line_at(Local::now().date_naive(), line)
    }

    pub fn append_line_at(&self, date: NaiveDate, line: &str) -> io::Result<()> {
        let _guard = self.write_lock.lock().unwrap_or_else(|it| it.into_inner());
        let path = self.path_for_date(date);
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        file.write_all(line.as_bytes())?;
        if !line.ends_with('\n') {
            file.write_all(b"\n")?;
        }
        file.flush()?;
        drop(file);
        trim_file(&path)
    }

    pub fn cleanup(&self) -> io::Result<()> {
        self.cleanup_before(Local::now().date_naive())
    }

    pub fn cleanup_before(&self, today: NaiveDate) -> io::Result<()> {
        let _guard = self.write_lock.lock().unwrap_or_else(|it| it.into_inner());
        let first_kept = today - Duration::days(6);
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|it| it.to_str()) else {
                continue;
            };
            let Some(stem) = name.strip_suffix(".log") else {
                continue;
            };
            let Ok(date) = NaiveDate::parse_from_str(stem, "%Y-%m-%d") else {
                continue;
            };
            if date < first_kept {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    pub fn write_zip<W: Write + Seek>(&self, output: W, crash_details: &str) -> io::Result<()> {
        self.cleanup()?;
        let today = Local::now().date_naive();
        let first_kept = today - Duration::days(6);
        let mut zip = ZipWriter::new(output);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|it| it.to_str()) else {
                continue;
            };
            let Some(stem) = name.strip_suffix(".log") else {
                continue;
            };
            let Ok(date) = NaiveDate::parse_from_str(stem, "%Y-%m-%d") else {
                continue;
            };
            if date >= first_kept && date <= today {
                // Logging rewrites the daily file when trimming it. Snapshot it before compressing.
                let bytes = {
                    let _guard = self.write_lock.lock().unwrap_or_else(|it| it.into_inner());
                    fs::read(&path)?
                };
                zip.start_file(name, options)?;
                zip.write_all(&bytes)?;
            }
        }
        zip.start_file("crash.txt", options)?;
        zip.write_all(crash_details.as_bytes())?;
        zip.finish()?;
        Ok(())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

fn trim_file(path: &Path) -> io::Result<()> {
    let mut bytes = fs::read(path)?;
    while bytes.len() as u64 >= MAX_LOG_SIZE {
        let Some(end) = bytes.iter().rposition(|byte| *byte == b'\n') else {
            bytes.clear();
            break;
        };
        let start = bytes[..end].iter().rposition(|byte| *byte == b'\n').map_or(0, |pos| pos + 1);
        bytes.truncate(start);
    }
    fs::write(path, bytes)
}

static MANAGER: OnceLock<Arc<LogManager>> = OnceLock::new();
static CRASH_RECORD: OnceLock<Mutex<Option<CrashRecord>>> = OnceLock::new();
static PANIC_HOOK: OnceLock<()> = OnceLock::new();

fn crash_slot() -> &'static Mutex<Option<CrashRecord>> {
    CRASH_RECORD.get_or_init(|| Mutex::new(None))
}

pub fn manager() -> Option<Arc<LogManager>> {
    MANAGER.get().cloned()
}

pub fn crash_record() -> Option<CrashRecord> {
    crash_slot().lock().unwrap_or_else(|it| it.into_inner()).clone()
}

pub fn set_crash_record(record: CrashRecord) {
    let mut slot = crash_slot().lock().unwrap_or_else(|it| it.into_inner());
    if slot.is_none() {
        *slot = Some(record);
    }
}

pub fn record_error(error: &anyhow::Error) -> CrashRecord {
    let summary = format!("{error}");
    let details = format!("{error:?}");
    let record = CrashRecord { summary, details };
    set_crash_record(record.clone());
    if let Some(manager) = manager() {
        let _ = manager.append_line(&format!("ERROR: {}", record.details));
    }
    record
}

pub fn register_with_dir(dir: impl AsRef<Path>) -> io::Result<Arc<LogManager>> {
    let manager = Arc::new(LogManager::new(dir)?);
    manager.cleanup()?;
    let _ = MANAGER.set(manager.clone());
    install_panic_hook();

    let filter = if std::env::var("RUST_LOG").is_ok() {
        EnvFilter::from_default_env()
    } else {
        EnvFilter::try_new("hyper=info,rustls=info,debug").unwrap()
    };
    let subscriber = tracing_subscriber::registry()
        .with(CustomLayer)
        .with(FileLayer { manager: manager.clone() })
        .with(filter);
    let _ = subscriber.try_init();
    Ok(manager)
}

pub fn register() {
    let _ = register_with_dir(PathBuf::from("logs"));
}

pub fn install_panic_hook() {
    if PANIC_HOOK.set(()).is_err() {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let payload = info
                .payload()
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic");
            let location = info
                .location()
                .map(|it| format!("{}:{}:{}", it.file(), it.line(), it.column()))
                .unwrap_or_else(|| "unknown location".to_owned());
            let thread = std::thread::current();
            let thread_name = thread.name().unwrap_or("unnamed");
            let details =
                format!("thread={thread_name}\nlocation={location}\npayload={payload}\nbacktrace:\n{}", std::backtrace::Backtrace::force_capture());
            set_crash_record(CrashRecord {
                summary: payload.to_owned(),
                details: details.clone(),
            });
            if let Some(manager) = manager() {
                let _ = manager.append_line(&format!("PANIC: {details}"));
            }
        }));
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| previous(info)));
    }));
}

#[derive(Default)]
struct EventData {
    message: Option<String>,
    target: Option<String>,
    fields: Vec<(&'static str, String)>,
}

impl Visit for EventData {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_owned());
        } else if field.name() == "log.target" {
            self.target = Some(value.to_owned());
        } else if !field.name().starts_with("log.") {
            self.fields.push((field.name(), value.to_owned()));
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let value = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(value);
        } else if !field.name().starts_with("log.") {
            self.fields.push((field.name(), value));
        }
    }
}

fn event_data(event: &tracing::Event<'_>) -> (Level, String, EventData) {
    let mut data = EventData::default();
    event.record(&mut data);
    let meta = event.metadata();
    let target = data.target.clone().unwrap_or_else(|| meta.target().to_owned());
    (*meta.level(), target, data)
}

fn format_plain(event: &tracing::Event<'_>) -> Option<String> {
    let (level, target, data) = event_data(event);
    if target.starts_with("jni::") && level >= Level::INFO {
        return None;
    }
    let mut message = format!("{:.6?} {level:>5} {target}", Local::now());
    if !data.fields.is_empty() {
        message.push_str(" {");
        for (index, (name, value)) in data.fields.iter().enumerate() {
            if index > 0 {
                message.push(' ');
            }
            message.push_str(name);
            message.push('=');
            message.push_str(value);
        }
        message.push('}');
    }
    if let Some(content) = data.message {
        message.push_str(": ");
        message.push_str(&content);
    }
    Some(message)
}

struct FileLayer {
    manager: Arc<LogManager>,
}

impl<S> Layer<S> for FileLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        if let Some(message) = format_plain(event) {
            let _ = self.manager.append_line(&message);
        }
    }
}

struct CustomLayer;

impl<S> Layer<S> for CustomLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let (level, target, data) = event_data(event);
        if target.starts_with("jni::") && level >= Level::INFO {
            return;
        }
        #[cfg(not(target_os = "android"))]
        let mut msg = format!("{:.6?} ", Local::now()).bright_black().to_string()
            + &match level {
                Level::TRACE => "TRACE".bright_black(),
                Level::DEBUG => "DEBUG".magenta(),
                Level::INFO => " INFO".green(),
                Level::WARN => " WARN".yellow(),
                Level::ERROR => "ERROR".red(),
            }
            .to_string()
            + " ";
        #[cfg(target_os = "android")]
        let mut msg = String::new();
        msg += &target.bright_black().to_string();
        if !data.fields.is_empty() {
            use std::fmt::Write;
            msg += &"{".bold().to_string();
            for (name, value) in &data.fields {
                let _ = write!(msg, "{}={value} ", name.italic());
            }
            msg.pop();
            msg += &"}".bold().to_string();
        }
        if let Some(message) = data.message {
            msg += ": ";
            msg += &message;
        }
        match level {
            Level::TRACE => trace!("{}", msg),
            Level::DEBUG => debug!("{}", msg),
            Level::INFO => info!("{}", msg),
            Level::WARN => warn!("{}", msg),
            Level::ERROR => error!("{}", msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LogManager, MAX_LOG_SIZE};
    use chrono::{Duration, Local, NaiveDate};
    use std::io::Cursor;
    use tempfile::tempdir;
    use zip::ZipArchive;

    #[test]
    fn trims_the_last_lines_until_the_daily_file_is_under_the_limit() {
        let dir = tempdir().unwrap();
        let manager = LogManager::new(dir.path()).unwrap();
        let date = Local::now().date_naive();

        manager.append_line_at(date, &"a".repeat(MAX_LOG_SIZE as usize - 1024)).unwrap();
        manager.append_line_at(date, &"b".repeat(2048)).unwrap();

        let bytes = std::fs::read(manager.path_for_date(date)).unwrap();
        assert!(bytes.len() < MAX_LOG_SIZE as usize);
        assert!(bytes.iter().all(|byte| *byte == b'a' || *byte == b'\n'));
    }

    #[test]
    fn removes_only_date_named_logs_older_than_seven_days() {
        let dir = tempdir().unwrap();
        let manager = LogManager::new(dir.path()).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 10, 15).unwrap();
        let old = today - Duration::days(7);
        let kept = today - Duration::days(6);
        std::fs::write(manager.path_for_date(old), b"old").unwrap();
        std::fs::write(manager.path_for_date(kept), b"kept").unwrap();
        std::fs::write(dir.path().join("notes.log"), b"keep").unwrap();
        std::fs::write(dir.path().join("2026-99-99.log"), b"keep").unwrap();

        manager.cleanup_before(today).unwrap();

        assert!(!manager.path_for_date(old).exists());
        assert!(manager.path_for_date(kept).exists());
        assert!(dir.path().join("notes.log").exists());
        assert!(dir.path().join("2026-99-99.log").exists());
    }

    #[test]
    fn exports_recent_logs_and_crash_metadata_to_zip() {
        let dir = tempdir().unwrap();
        let manager = LogManager::new(dir.path()).unwrap();
        let date = Local::now().date_naive();
        manager.append_line_at(date, "hello").unwrap();
        let mut output = Cursor::new(Vec::new());

        manager.write_zip(&mut output, "panic details").unwrap();

        let mut archive = ZipArchive::new(Cursor::new(output.into_inner())).unwrap();
        let mut log = String::new();
        std::io::Read::read_to_string(&mut archive.by_name(&format!("{date}.log")).unwrap(), &mut log).unwrap();
        assert!(log.contains("hello"));
        let mut crash = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("crash.txt").unwrap(), &mut crash).unwrap();
        assert_eq!(crash, "panic details");
    }

    #[test]
    fn exports_a_complete_snapshot_when_the_log_changes_during_compression() {
        struct TruncateLogOnWrite {
            output: Cursor<Vec<u8>>,
            log_path: std::path::PathBuf,
            truncated: bool,
        }

        impl std::io::Write for TruncateLogOnWrite {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if !self.truncated && bytes.len() >= 1024 {
                    std::fs::write(&self.log_path, [])?;
                    self.truncated = true;
                }
                std::io::Write::write(&mut self.output, bytes)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        impl std::io::Seek for TruncateLogOnWrite {
            fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
                std::io::Seek::seek(&mut self.output, position)
            }
        }

        let dir = tempdir().unwrap();
        let manager = LogManager::new(dir.path()).unwrap();
        let date = Local::now().date_naive();
        let mut seed = 0x12345678_u32;
        let mut expected: String = (0..256 * 1024)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                char::from(b' ' + (seed % 95) as u8)
            })
            .collect();
        expected.push('\n');
        manager.append_line_at(date, &expected).unwrap();
        let mut output = TruncateLogOnWrite {
            output: Cursor::new(Vec::new()),
            log_path: manager.path_for_date(date),
            truncated: false,
        };

        manager.write_zip(&mut output, "manual export").unwrap();

        assert!(output.truncated);
        let mut archive = ZipArchive::new(Cursor::new(output.output.into_inner())).unwrap();
        let mut actual = String::new();
        std::io::Read::read_to_string(&mut archive.by_name(&format!("{date}.log")).unwrap(), &mut actual).unwrap();
        assert_eq!(actual.len(), expected.len());
        assert_eq!(actual, expected);
    }
}
