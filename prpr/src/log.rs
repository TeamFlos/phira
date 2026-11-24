//! Logging, crash capture, retention, and diagnostic export utilities.

use chrono::{Duration, Local, NaiveDate};
use colored::Colorize;
use miniquad::{debug, error, info, trace, warn};
use std::{
    fs,
    io::{self, Seek, Write},
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex, OnceLock},
    thread,
};
use tracing::{field::Visit, Level, Subscriber};
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::{prelude::*, EnvFilter, Layer};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

const LOG_FILE_PREFIX: &str = "phira";
const LOG_FILE_FORMAT: &str = "%Y-%m-%d";
const LOG_KEEP_DAYS: usize = 7;
/// Size of the in-memory tail kept for crash exports. It survives even when
/// the writer thread cannot drain its channel (wedged disk, hard crash).
const TAIL_BUFFER_SIZE: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct CrashRecord {
    pub summary: String,
    pub details: String,
}

enum WriterMessage {
    Line(String),
    /// Acknowledged by the writer thread once everything before it hit the OS.
    Flush(mpsc::SyncSender<()>),
}

pub struct LogManager {
    dir: PathBuf,
    tx: mpsc::Sender<WriterMessage>,
    tail: Mutex<Vec<u8>>,
}

impl LogManager {
    pub fn new(dir: impl AsRef<Path>) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        // Daily rotation and retention are delegated to tracing-appender; the
        // file name carries the UTC date (rotation uses `now_utc`).
        let appender = Builder::new()
            .rotation(Rotation::DAILY)
            .filename_prefix(LOG_FILE_PREFIX)
            .filename_suffix("log")
            .max_log_files(LOG_KEEP_DAYS)
            .build(&dir)
            .map_err(|it| io::Error::other(it.to_string()))?;
        let (tx, rx) = mpsc::channel();
        thread::Builder::new().name("log-writer".to_owned()).spawn(move || {
            let mut appender = appender;
            while let Ok(message) = rx.recv() {
                match message {
                    WriterMessage::Line(line) => {
                        if let Err(error) = appender.write_all(line.as_bytes()) {
                            eprintln!("failed to write log line: {error}");
                        }
                    }
                    WriterMessage::Flush(ack) => {
                        let _ = appender.flush();
                        let _ = ack.send(());
                    }
                }
            }
        })?;
        Ok(Self {
            dir,
            tx,
            tail: Mutex::new(Vec::new()),
        })
    }

    pub fn path_for_date(&self, date: NaiveDate) -> PathBuf {
        self.dir.join(format!("{LOG_FILE_PREFIX}.{}.log", date.format(LOG_FILE_FORMAT)))
    }

    pub fn append_line(&self, line: &str) -> io::Result<()> {
        {
            let mut tail = self.tail.lock().unwrap_or_else(|it| it.into_inner());
            push_tail_line(&mut tail, line);
        }
        let mut line = line.to_owned();
        if !line.ends_with('\n') {
            line.push('\n');
        }
        self.tx
            .send(WriterMessage::Line(line))
            .map_err(|_| io::Error::other("log writer thread has stopped"))
    }

    /// Blocks until every line appended so far has been handed to the OS.
    /// Best-effort: if the writer thread is gone, returns immediately.
    pub fn flush(&self) {
        let (tx, rx) = mpsc::sync_channel(1);
        if self.tx.send(WriterMessage::Flush(tx)).is_ok() {
            let _ = rx.recv();
        }
    }

    pub fn write_zip<W: Write + Seek>(&self, output: W, crash_details: &str) -> io::Result<()> {
        self.flush();
        let today = Local::now().date_naive();
        // Rotation runs on UTC, so accept one day of slack around the local
        // date range. Retention (max_log_files) already bounds the file count.
        let first_kept = today - Duration::days(LOG_KEEP_DAYS as i64 - 1);
        let last_kept = today + Duration::days(1);
        let mut zip = ZipWriter::new(output);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|it| it.to_str()) else {
                continue;
            };
            let Some(stem) = name.strip_prefix(&format!("{LOG_FILE_PREFIX}.")).and_then(|it| it.strip_suffix(".log")) else {
                continue;
            };
            let Ok(date) = NaiveDate::parse_from_str(stem, LOG_FILE_FORMAT) else {
                continue;
            };
            if date < first_kept || date > last_kept {
                continue;
            }
            let bytes = fs::read(&path)?;
            zip.start_file(name, options)?;
            zip.write_all(&bytes)?;
        }
        let tail = self.tail.lock().unwrap_or_else(|it| it.into_inner());
        zip.start_file("log-tail.txt", options)?;
        zip.write_all(&tail)?;
        drop(tail);
        zip.start_file("crash.txt", options)?;
        zip.write_all(crash_details.as_bytes())?;
        zip.finish()?;
        Ok(())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

fn push_tail_line(tail: &mut Vec<u8>, line: &str) {
    tail.extend_from_slice(line.as_bytes());
    if !line.ends_with('\n') {
        tail.push(b'\n');
    }
    if tail.len() > TAIL_BUFFER_SIZE {
        // Drop whole lines from the front to get back under the cap, so the
        // buffer always starts at a line boundary.
        let excess = tail.len() - TAIL_BUFFER_SIZE;
        let cut = tail[excess..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(tail.len(), |pos| excess + pos + 1);
        tail.drain(..cut);
    }
}

static MANAGER: OnceLock<Arc<LogManager>> = OnceLock::new();
static CRASH_RECORD: OnceLock<Mutex<Option<CrashRecord>>> = OnceLock::new();
static PANIC_HOOK: OnceLock<()> = OnceLock::new();
/// The thread that installed the panic hook, i.e. the frame-loop thread.
/// Panics on other threads are logged but left to their local handlers.
static MAIN_THREAD: OnceLock<thread::ThreadId> = OnceLock::new();

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

pub fn register_with_dir(dir: impl AsRef<Path>) {
    // Failing to set up file logging must never keep the game from starting;
    // console logging through CustomLayer still works without a manager.
    let manager = LogManager::new(dir).ok().map(Arc::new);
    if let Some(it) = &manager {
        let _ = MANAGER.set(it.clone());
    }
    install_panic_hook();

    let filter = if std::env::var("RUST_LOG").is_ok() {
        EnvFilter::from_default_env()
    } else {
        EnvFilter::try_new("hyper=info,rustls=info,debug").unwrap()
    };
    let subscriber = tracing_subscriber::registry().with(CustomLayer).with(FileLayer { manager }).with(filter);
    let _ = subscriber.try_init();
}

pub fn install_panic_hook() {
    if PANIC_HOOK.set(()).is_err() {
        return;
    }
    let _ = MAIN_THREAD.set(thread::current().id());
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
            // Only a panic on the frame-loop thread tears the game down into
            // the crash scene; worker threads may have their own recovery
            // paths (e.g. the library export thread's error dialog).
            if MAIN_THREAD.get().is_some_and(|it| *it == thread.id()) {
                set_crash_record(CrashRecord {
                    summary: payload.to_owned(),
                    details: details.clone(),
                });
            }
            if let Some(manager) = manager() {
                let _ = manager.append_line(&format!("PANIC: {details}"));
                manager.flush();
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
    manager: Option<Arc<LogManager>>,
}

impl<S> Layer<S> for FileLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        if let (Some(manager), Some(message)) = (&self.manager, format_plain(event)) {
            let _ = manager.append_line(&message);
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
    use super::{push_tail_line, LogManager, TAIL_BUFFER_SIZE};
    use chrono::{Duration, Local};
    use std::io::{Cursor, Read};
    use tempfile::tempdir;
    use zip::ZipArchive;

    #[test]
    fn appends_lines_to_the_daily_file() {
        let dir = tempdir().unwrap();
        let manager = LogManager::new(dir.path()).unwrap();

        manager.append_line("hello").unwrap();
        manager.flush();

        // The file is named after the UTC date, so find it instead of guessing.
        let log_files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|it| it == "log"))
            .collect();
        assert_eq!(log_files.len(), 1);
        assert_eq!(std::fs::read(&log_files[0]).unwrap(), b"hello\n");
    }

    #[test]
    fn exports_recent_logs_and_crash_metadata_to_zip() {
        let dir = tempdir().unwrap();
        let manager = LogManager::new(dir.path()).unwrap();
        let today = Local::now().date_naive();
        std::fs::write(manager.path_for_date(today - Duration::days(6)), "earlier\n").unwrap();
        std::fs::write(manager.path_for_date(today - Duration::days(7)), "expired\n").unwrap();

        manager.append_line("latest").unwrap();
        let mut cursor = Cursor::new(Vec::new());
        manager.write_zip(&mut cursor, "crash details").unwrap();

        let mut archive = ZipArchive::new(cursor).unwrap();
        assert!(archive
            .by_name(&format!("{prefix}.{}.log", today - Duration::days(6), prefix = super::LOG_FILE_PREFIX))
            .is_ok());
        assert!(archive
            .by_name(&format!("{prefix}.{}.log", today - Duration::days(7), prefix = super::LOG_FILE_PREFIX))
            .is_err());
        // The active file is named after the UTC date; it must contain the
        // appended line and be the only remaining log entry.
        let names: Vec<String> = archive.file_names().map(str::to_owned).collect();
        let log_entries = names.iter().filter(|name| name.ends_with(".log")).count();
        assert_eq!(log_entries, 2);
        for name in &names {
            if name.ends_with(".log") && !name.contains(&format!("{}", today - Duration::days(6))) {
                let mut contents = String::new();
                archive.by_name(name).unwrap().read_to_string(&mut contents).unwrap();
                assert_eq!(contents, "latest\n");
            }
        }
        let mut tail = String::new();
        archive.by_name("log-tail.txt").unwrap().read_to_string(&mut tail).unwrap();
        assert!(tail.contains("latest\n"));
        let mut crash = String::new();
        archive.by_name("crash.txt").unwrap().read_to_string(&mut crash).unwrap();
        assert_eq!(crash, "crash details");
    }

    #[test]
    fn tail_buffer_keeps_only_whole_recent_lines() {
        let mut tail = Vec::new();
        for index in 0..2000 {
            push_tail_line(&mut tail, &format!("line-{index:04}: {}", "x".repeat(90)));
        }

        assert!(tail.len() <= TAIL_BUFFER_SIZE);
        assert!(tail.starts_with(b"line-"));
        assert!(tail.ends_with(b"\n"));
        let text = String::from_utf8(tail).unwrap();
        assert!(!text.contains("line-0000:"));
        assert!(text.contains("\nline-1999:"));
    }
}
