//! LocalChart 本地谱面分享的网络传输层。
//!
//! - [`ChartServer`]：房主启动的轻量 HTTP 文件服务器，将 `download/{chart_id}`
//!   目录打包为 zip 提供下载。优先监听 IPv6，失败回退 IPv4（V6 -> V4 顺序）。
//! - [`ChartSyncing`]：玩家从房主下载谱面时的共享状态（用于渲染"正在同步谱面"转圈）。

use anyhow::{Context, Result};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use crate::dir;

/// 把本地谱面 `local_path`（相对 `dir::charts()` 的子路径，如 `download/123` 或自定义路径）
/// 对应的目录复制到 `download/{uuid}`，供后续 serve / download 使用同一 UUID 目录。
pub fn stage_local_chart(local_path: &str, uuid: &str) -> Result<()> {
    let src = format!("{}/{}", dir::charts()?, local_path);
    let src_path = std::path::Path::new(&src);
    if !src_path.is_dir() {
        anyhow::bail!("local chart directory not found: {}", src_path.display());
    }
    let dst = format!("{}/download/{uuid}", dir::charts()?);
    let dst_path = std::path::Path::new(&dst);
    if dst_path.exists() {
        if dst_path.is_file() {
            std::fs::remove_file(dst_path)?;
        } else {
            std::fs::remove_dir_all(dst_path)?;
        }
    }
    copy_dir(src_path, dst_path)?;
    Ok(())
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src).with_context(|| format!("read dir {}", src.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// 把 `charts` 目录下的一个谱面包（`download/{chart_id}`，chart_id 为 UUID）打包成 zip（内存中）。
/// 返回 zip 的字节内容。若目录不存在则报错。
pub fn pack_chart_dir(chart_id: &str) -> Result<Vec<u8>> {
    let root = format!("{}/download/{chart_id}", dir::charts()?);
    let root = std::path::Path::new(&root);
    if !root.is_dir() {
        anyhow::bail!("local chart directory not found: {}", root.display());
    }

    let mut out = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        let options =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        fn visit(
            zip: &mut zip::ZipWriter<std::io::Cursor<&mut Vec<u8>>>,
            options: zip::write::SimpleFileOptions,
            base: &std::path::Path,
            path: &std::path::Path,
        ) -> Result<()> {
            for entry in std::fs::read_dir(path).with_context(|| format!("read dir {}", path.display()))? {
                let entry = entry?;
                let p = entry.path();
                let rel = p.strip_prefix(base)?;
                if p.is_dir() {
                    let name = format!("{}/", rel.to_string_lossy());
                    zip.add_directory(name, options)?;
                    visit(zip, options, base, &p)?;
                } else {
                    zip.start_file(rel.to_string_lossy().to_string(), options)?;
                    let mut f = std::fs::File::open(&p)?;
                    std::io::copy(&mut f, zip)?;
                }
            }
            Ok(())
        }

        visit(&mut zip, options, root, root)?;
        zip.finish()?;
    }
    Ok(out)
}

/// 房主本地谱面 HTTP 下载服务器。
pub struct ChartServer {
    addr: String,
    port: u16,
    config: Arc<ChartServerConfig>,
    shutdown: Arc<AtomicBool>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
}

struct ChartServerConfig {
    chart_id: String,
    /// 最新打包好的谱面 zip 内容（惰性生成并缓存，保存失败前的版本）
    zip_cache: RwLock<Option<Vec<u8>>>,
    ready: AtomicBool,
    error: Mutex<Option<String>>,
}

impl ChartServer {
    /// 启动服务器。`chart_id` 是要分享的本地谱面 UUID。
    /// 会先尝试监听 IPv6 `[::]:0`，失败则回退监听 IPv4 `0.0.0.0:0`（V6 -> V4 顺序）。
    /// 地址由服务端协调（服务端负责打洞/下发可达地址），此处只报告监听端口。
    pub fn start(chart_id: String) -> Result<Arc<Self>> {
        let config = Arc::new(ChartServerConfig {
            chart_id,
            zip_cache: RwLock::new(None),
            ready: AtomicBool::new(false),
            error: Mutex::new(None),
        });

        let listener = listen_v6_then_v4().context("failed to bind chart download server")?;
        let addr = listener.local_addr()?;
        let shutdown = Arc::new(AtomicBool::new(false));

        let handle = {
            let config = Arc::clone(&config);
            let shutdown = Arc::clone(&shutdown);
            thread::spawn(move || serve_loop(listener, config, shutdown))
        };

        Ok(Arc::new(Self {
            addr: addr.ip().to_string(),
            port: addr.port(),
            config,
            shutdown,
            handle: Mutex::new(Some(handle)),
        }))
    }

    /// 当前可连接的地址（V4 形式，若是 :: 则返回 0.0.0.0）
    #[allow(dead_code)]
    pub fn addr(&self) -> &str {
        &self.addr
    }

    #[allow(dead_code)]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// 服务器是否已经准备好（已监听）
    pub fn ready(&self) -> bool {
        self.config.ready.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // 主动连接一次以唤醒 accept 循环
        let _ = std::net::TcpStream::connect((self.addr.as_str(), self.port));
        if let Some(h) = self.handle.lock().unwrap().take() {
            let _ = h.join();
        }
    }
}

fn listen_v6_then_v4() -> Result<TcpListener> {
    for host in ["[::]:0", "0.0.0.0:0"] {
        if let Ok(l) = TcpListener::bind(host) {
            return Ok(l);
        }
    }
    anyhow::bail!("no usable address to bind")
}

fn serve_loop(
    listener: TcpListener,
    config: Arc<ChartServerConfig>,
    shutdown: Arc<AtomicBool>,
) {
    config.ready.store(true, Ordering::SeqCst);
    listener
        .set_nonblocking(true)
        .ok();

    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let config = Arc::clone(&config);
                thread::spawn(move || handle_conn(stream, config));
            }
            _ => {
                thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

fn handle_conn(mut stream: TcpStream, config: Arc<ChartServerConfig>) {
    use std::io::{BufRead, BufReader};
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    // 丢弃请求头
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line == "\r\n" || line == "\n" {
            break;
        }
    }

    let mut path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .trim_start_matches('/')
        .to_string();
    if let Some(q) = path.find('?') {
        path.truncate(q);
    }

    let body = if path == format!("download/{}/chart.zip", config.chart_id) {
        // 生成并缓存 zip
        if config.zip_cache.read().unwrap().is_none() {
            match pack_chart_dir(&config.chart_id) {
                Ok(bytes) => {
                    *config.zip_cache.write().unwrap() = Some(bytes);
                }
                Err(e) => {
                    *config.error.lock().unwrap() = Some(e.to_string());
                    respond(&mut stream, "404 Not Found", b"chart not found");
                    return;
                }
            }
        }
        config.zip_cache.read().unwrap().clone().unwrap_or_default()
    } else {
        respond(&mut stream, "404 Not Found", b"not found");
        return;
    };

    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

fn respond(stream: &mut TcpStream, status: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

impl Drop for ChartServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 玩家从房主下载谱面时的共享状态（用于渲染"正在同步谱面"转圈）。
pub struct ChartSyncing {
    pub done: AtomicBool,
    pub error: Mutex<Option<String>>,
    pub started: AtomicBool,
}

impl ChartSyncing {
    pub fn new() -> Self {
        Self {
            done: AtomicBool::new(false),
            error: Mutex::new(None),
            started: AtomicBool::new(false),
        }
    }

    pub fn mark_started(&self) {
        self.started.store(true, Ordering::SeqCst);
    }

    pub fn mark_done(&self) {
        self.done.store(true, Ordering::SeqCst);
    }

    pub fn set_error(&self, e: impl Into<String>) {
        *self.error.lock().unwrap() = Some(e.into());
        self.done.store(true, Ordering::SeqCst);
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }
}

/// 房主把本地谱面包（`download/{chart_id}`）打包成 zip，经 game 连接上传到服务端中转。
/// 玩家将经服务端（同一 game 连接）下载，兼容内网穿透（无需额外 web 端口映射）。
pub async fn upload_chart(client: &phira_mp_client::Client, chart_id: &str) -> Result<()> {
    let zip = pack_chart_dir(chart_id)?;
    client.upload_chart(chart_id.to_string(), zip).await?;
    Ok(())
}

/// 玩家经 game 连接从服务端获取谱面包（`download/{chart_id}`），
/// 解压到本地 `download/{chart_id}` 目录。
pub async fn download_chart(
    client: &phira_mp_client::Client,
    chart_id: &str,
    syncing: Arc<ChartSyncing>,
) -> Result<()> {
    syncing.mark_started();
    let bytes = client.download_chart(chart_id.to_string()).await?;

    // 解压到临时目录
    let tmp = format!("{}/download/sync_{chart_id}", dir::charts()?);
    let tmp_path = std::path::Path::new(&tmp);
    if tmp_path.exists() {
        if tmp_path.is_file() {
            std::fs::remove_file(tmp_path)?;
        } else {
            std::fs::remove_dir_all(tmp_path)?;
        }
    }
    std::fs::create_dir_all(tmp_path)?;
    {
        let chart_dir = prpr::dir::Dir::new(tmp_path)?;
        prpr::ext::unzip_into(std::io::Cursor::new(bytes), &chart_dir, false)?;
    }

    // 移动到 download/{chart_id}
    let to = format!("{}/download/{chart_id}", dir::charts()?);
    let to_path = std::path::Path::new(&to);
    if to_path.exists() {
        if to_path.is_file() {
            std::fs::remove_file(to_path)?;
        } else {
            std::fs::remove_dir_all(to_path)?;
        }
    }
    if let Some(parent) = to_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(tmp_path, to_path)?;

    syncing.mark_done();
    Ok(())
}
