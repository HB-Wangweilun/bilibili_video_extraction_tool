//! 分片并发下载器：Range 请求 + 断点重试 + 备用 CDN 回退 + 实时进度事件。

use crate::bilibili::REFERER;
use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// 并行分片数
const MAX_PARALLEL: usize = 8;
/// 单个分片最小 2MiB，小于该值不切分
const MIN_CHUNK: u64 = 2 * 1024 * 1024;
/// 单分片最大重试次数
const MAX_RETRY: u32 = 5;

/// 推送给前端的进度负载。
#[derive(Serialize, Clone)]
pub struct DownloadProgress {
    /// preparing | downloading | merging | done | error | cancelled
    pub stage: String,
    /// video | audio | file | merge
    pub phase: String,
    pub downloaded: u64,
    pub total: u64,
    pub percent: f64,
    /// 字节 / 秒
    pub speed: f64,
    pub message: String,
}

/// 线程安全的进度状态：下载任务写，上报任务读。
pub struct ProgressState {
    downloaded: AtomicU64,
    total: AtomicU64,
    stage: Mutex<String>,
    phase: Mutex<String>,
    message: Mutex<String>,
}

impl Default for ProgressState {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressState {
    pub fn new() -> Self {
        Self {
            downloaded: AtomicU64::new(0),
            total: AtomicU64::new(0),
            stage: Mutex::new("preparing".into()),
            phase: Mutex::new(String::new()),
            message: Mutex::new(String::new()),
        }
    }

    pub fn set_stage(&self, stage: &str) {
        if let Ok(mut g) = self.stage.lock() {
            *g = stage.to_string();
        }
    }

    pub fn set_phase(&self, phase: &str) {
        if let Ok(mut g) = self.phase.lock() {
            *g = phase.to_string();
        }
    }

    pub fn set_message(&self, msg: impl AsRef<str>) {
        if let Ok(mut g) = self.message.lock() {
            *g = msg.as_ref().to_string();
        }
    }

    pub fn snapshot(&self) -> (String, String, u64, u64, String) {
        (
            self.stage.lock().map(|g| g.clone()).unwrap_or_default(),
            self.phase.lock().map(|g| g.clone()).unwrap_or_default(),
            self.downloaded.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
            self.message.lock().map(|g| g.clone()).unwrap_or_default(),
        )
    }

    fn add_downloaded(&self, n: u64) {
        self.downloaded.fetch_add(n, Ordering::Relaxed);
    }

    fn mark(&self) -> (u64, u64) {
        (
            self.downloaded.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        )
    }

    fn restore(&self, mark: (u64, u64)) {
        self.downloaded.store(mark.0, Ordering::Relaxed);
        self.total.store(mark.1, Ordering::Relaxed);
    }

    /// 开始一轮下载前重置计数。
    pub fn start_fresh(&self, total: u64) {
        self.downloaded.store(0, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
    }

    /// 用于重编码这类无法按字节统计的阶段：用 0-100 的百分比驱动进度条。
    pub fn set_percent_progress(&self, percent: f64) {
        self.total.store(1000, Ordering::Relaxed);
        self.downloaded
            .store((percent.clamp(0.0, 100.0) * 10.0) as u64, Ordering::Relaxed);
    }
}

/// 周期性把进度推给前端。
pub struct Reporter {
    stop: Arc<AtomicBool>,
}

impl Reporter {
    pub fn start(app: AppHandle, state: Arc<ProgressState>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        tauri::async_runtime::spawn(async move {
            let mut last_bytes = 0u64;
            let mut last_tick = Instant::now();
            let mut smooth_speed = 0f64;
            let mut interval = tokio::time::interval(Duration::from_millis(280));
            loop {
                interval.tick().await;
                if stop2.load(Ordering::Relaxed) {
                    break;
                }
                let (stage, phase, downloaded, total, message) = state.snapshot();
                let now = Instant::now();
                let dt = now.duration_since(last_tick).as_secs_f64().max(0.001);
                let delta = downloaded.saturating_sub(last_bytes);
                let inst = delta as f64 / dt;
                smooth_speed = if smooth_speed <= 0.0 {
                    inst
                } else {
                    smooth_speed * 0.55 + inst * 0.45
                };
                last_bytes = downloaded;
                last_tick = now;

                let percent = if total > 0 {
                    (downloaded as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
                } else if stage == "merging" || stage == "done" {
                    100.0
                } else {
                    0.0
                };

                let speed = if (total > 0 && downloaded >= total) || phase == "merge" {
                    0.0
                } else {
                    smooth_speed
                };

                let _ = app.emit(
                    "download-progress",
                    DownloadProgress {
                        stage,
                        phase,
                        downloaded,
                        total,
                        percent,
                        speed,
                        message,
                    },
                );
            }
        });
        Self { stop }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// 探测远程文件大小与是否支持 Range。
pub async fn probe_size(client: &reqwest::Client, url: &str) -> Result<(u64, bool)> {
    let resp = client
        .get(url)
        .header(reqwest::header::REFERER, REFERER)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await
        .context("无法连接视频服务器")?;

    let status = resp.status();
    if status.as_u16() == 206 {
        let total = resp
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.rsplit('/').next().map(|x| x.trim().to_string()))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        return Ok((total, true));
    }
    if status.is_success() {
        let len = resp
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        return Ok((len, false));
    }
    bail!("视频服务器返回 {status}，可能是地址已过期或需要登录 Cookie");
}

/// 平台相关的位置写入。
///
/// Windows 没有 `write_at`，只有 `seek_write`（会移动本句柄的文件指针），
/// 因此每个分片任务必须使用**独立的文件句柄**，否则并发写入会互相干扰。
#[cfg(windows)]
fn os_write_at(file: &std::fs::File, buf: &[u8], offset: u64) -> std::io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_write(buf, offset)
}

#[cfg(unix)]
fn os_write_at(file: &std::fs::File, buf: &[u8], offset: u64) -> std::io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.write_at(buf, offset)
}

/// 写入文件指定偏移（循环处理部分写入）。
fn write_at_all(file: &std::fs::File, mut buf: &[u8], mut offset: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        let n = os_write_at(file, buf, offset)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "写入磁盘失败",
            ));
        }
        offset += n as u64;
        buf = &buf[n..];
    }
    Ok(())
}

/// 下载单个分片，失败自动重试并从断点续传。
async fn download_chunk(
    client: reqwest::Client,
    url: String,
    start: u64,
    end: u64,
    dest: std::path::PathBuf,
    state: Arc<ProgressState>,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    // 独占句柄，避免并发写文件指针竞争
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&dest)
        .with_context(|| format!("无法打开临时文件 {}", dest.display()))?;

    let mut written: u64 = 0; // 本分片已成功落盘的字节（跨重试保留）
    let mut attempt: u32 = 0;

    loop {
        if cancel.load(Ordering::Relaxed) {
            bail!("已取消");
        }
        let from = start + written;
        if from > end {
            return Ok(());
        }

        let result: Result<()> = async {
            let resp = client
                .get(&url)
                .header(reqwest::header::REFERER, REFERER)
                .header(reqwest::header::RANGE, format!("bytes={from}-{end}"))
                .send()
                .await?;

            let status = resp.status();
            if !(status.is_success() || status.as_u16() == 206) {
                bail!("HTTP {status}");
            }

            let mut stream = resp.bytes_stream();
            while let Some(item) = stream.next().await {
                if cancel.load(Ordering::Relaxed) {
                    bail!("已取消");
                }
                let bytes = item?;
                write_at_all(&file, &bytes, from + written)?;
                written += bytes.len() as u64;
                state.add_downloaded(bytes.len() as u64);
                if from + written > end {
                    break;
                }
            }
            Ok(())
        }
        .await;

        match result {
            Ok(()) => return Ok(()),
            Err(e) => {
                if cancel.load(Ordering::Relaxed) {
                    bail!("已取消");
                }
                attempt += 1;
                if attempt >= MAX_RETRY {
                    return Err(anyhow!("分片下载失败（已重试 {attempt} 次）：{e}"));
                }
                tokio::time::sleep(Duration::from_millis(400 * attempt as u64)).await;
            }
        }
    }
}

/// 无 Range 支持时的顺序下载。
async fn download_sequential(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    state: &Arc<ProgressState>,
    cancel: &Arc<AtomicBool>,
) -> Result<u64> {
    let resp = client
        .get(url)
        .header(reqwest::header::REFERER, REFERER)
        .send()
        .await
        .context("连接视频服务器失败")?;
    if !resp.status().is_success() {
        bail!("视频服务器返回 {}，请重试", resp.status());
    }

    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("无法创建临时文件 {}", dest.display()))?;
    let mut stream = resp.bytes_stream();
    let mut total = 0u64;
    while let Some(item) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            bail!("已取消");
        }
        let bytes = item.context("下载数据中断")?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await?;
        total += bytes.len() as u64;
        state.add_downloaded(bytes.len() as u64);
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    Ok(total)
}

/// 下载一个资源到本地文件，依次尝试主地址与备用 CDN。
///
/// 注意：总大小由调用方在下载前通过 [`ProgressState::start_fresh`] 设定，
/// 本函数只负责累加已下载字节，换备用线路时会把计数还原到进入本函数时的基线。
pub async fn download_resource(
    client: &reqwest::Client,
    urls: &[String],
    dest: &Path,
    state: &Arc<ProgressState>,
    cancel: &Arc<AtomicBool>,
    phase: &str,
    label: &str,
) -> Result<u64> {
    if urls.is_empty() {
        bail!("没有可用的下载地址");
    }
    state.set_phase(phase);

    let mut last_err: Option<anyhow::Error> = None;
    let base_mark = state.mark();

    for (idx, url) in urls.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            bail!("已取消");
        }
        // 每条线路开始时回到基线，避免把失败线路的字节算进来
        state.restore(base_mark);

        state.set_message(format!(
            "{label}：正在连接{}…",
            if idx == 0 { "" } else { "备用线路" }
        ));

        let (total, supports_range) = match probe_size(client, url).await {
            Ok(v) => v,
            Err(e) => {
                last_err = Some(e);
                continue;
            }
        };
        if total == 0 || !supports_range || total <= MIN_CHUNK {
            state.set_message(format!("{label}：下载中…"));
            match download_sequential(client, url, dest, state, cancel).await {
                Ok(n) => return Ok(n),
                Err(e) => {
                    if e.to_string().contains("已取消") {
                        bail!("已取消");
                    }
                    let _ = std::fs::remove_file(dest);
                    last_err = Some(e);
                    continue;
                }
            }
        }

        // 分片并发下载
        let n_chunks = (total / MIN_CHUNK).clamp(1, MAX_PARALLEL as u64) as usize;
        let chunk_size = total.div_ceil(n_chunks as u64);

        // 预分配磁盘空间，随后交给各分片任务独立写入
        let file = std::fs::File::create(dest)
            .with_context(|| format!("无法创建临时文件 {}", dest.display()))?;
        file.set_len(total)
            .context("预分配磁盘空间失败（磁盘可能已满）")?;
        drop(file);

        state.set_message(format!("{label}：{n_chunks} 线程并发下载中…"));

        let mut ranges: Vec<(u64, u64)> = Vec::new();
        let mut s = 0u64;
        while s < total {
            let e = (s + chunk_size - 1).min(total - 1);
            ranges.push((s, e));
            s = e + 1;
        }

        let dest_path = dest.to_path_buf();
        let results = futures_util::stream::iter(ranges.into_iter().map(|(s, e)| {
            let c = client.clone();
            let u = url.clone();
            let p = dest_path.clone();
            let st = state.clone();
            let cx = cancel.clone();
            async move { download_chunk(c, u, s, e, p, st, cx).await }
        }))
        .buffer_unordered(MAX_PARALLEL)
        .collect::<Vec<Result<()>>>()
        .await;

        let mut failed: Option<anyhow::Error> = None;
        for r in results {
            if let Err(e) = r {
                if e.to_string().contains("已取消") {
                    bail!("已取消");
                }
                failed = Some(e);
            }
        }

        if failed.is_none() {
            return Ok(total);
        }
        last_err = failed;
        let _ = std::fs::remove_file(dest);
    }

    Err(last_err.unwrap_or_else(|| anyhow!("下载失败，请重试")))
}

/// 只抓取远程文件的**前若干字节**（用于抽帧预览 / 水印检测，避免整段下载）。
pub async fn download_prefix(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    max_bytes: u64,
) -> Result<u64> {
    let resp = client
        .get(url)
        .header(reqwest::header::REFERER, REFERER)
        .header(reqwest::header::RANGE, format!("bytes=0-{}", max_bytes - 1))
        .send()
        .await
        .context("连接视频服务器失败")?;
    if !(resp.status().is_success() || resp.status().as_u16() == 206) {
        bail!("视频服务器返回 {}", resp.status());
    }

    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("无法创建临时文件 {}", dest.display()))?;
    let mut stream = resp.bytes_stream();
    let mut total = 0u64;
    while let Some(item) = stream.next().await {
        let bytes = item.context("下载中断")?;
        if total + bytes.len() as u64 > max_bytes {
            let keep = (max_bytes - total) as usize;
            tokio::io::AsyncWriteExt::write_all(&mut file, &bytes[..keep]).await?;
            total += keep as u64;
            break;
        }
        tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await?;
        total += bytes.len() as u64;
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    Ok(total)
}

/// 抓取前若干字节，依次尝试主地址与备用地址，并做多轮重试。
///
/// B 站 CDN 偶发 503（尤其短时间多次请求同一节点），所以这里必须有兜底。
pub async fn download_prefix_multi(
    client: &reqwest::Client,
    urls: &[String],
    dest: &Path,
    max_bytes: u64,
) -> Result<u64> {
    if urls.is_empty() {
        bail!("没有可用的下载地址");
    }
    let mut last_err: Option<anyhow::Error> = None;
    for round in 0..3u32 {
        for url in urls.iter().take(4) {
            match download_prefix(client, url, dest, max_bytes).await {
                Ok(n) if n >= 64 * 1024 => return Ok(n),
                Ok(n) => last_err = Some(anyhow!("只取到 {n} 字节")),
                Err(e) => last_err = Some(e),
            }
            let _ = std::fs::remove_file(dest);
        }
        if round < 2 {
            tokio::time::sleep(Duration::from_millis(700 * (round as u64 + 1))).await;
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("抓取视频数据失败")))
}

/// 清洗文件名，去掉 Windows 非法字符并限制长度。
pub fn sanitize_filename(name: &str, fallback: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"\/:*?"<>|"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();

    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s = s.trim().trim_matches('.').trim().to_string();

    // Windows 保留设备名
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&s.to_ascii_uppercase().as_str()) {
        s = format!("_{s}");
    }

    let max = 110usize;
    if s.chars().count() > max {
        s = s.chars().take(max).collect();
    }

    if s.is_empty() {
        fallback.to_string()
    } else {
        s
    }
}

/// 若目标文件已存在，自动加 (1) (2) 后缀，避免覆盖用户已有文件。
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> std::path::PathBuf {
    let mut candidate = dir.join(format!("{stem}.{ext}"));
    let mut i = 1;
    while candidate.exists() {
        candidate = dir.join(format!("{stem} ({i}).{ext}"));
        i += 1;
        if i > 999 {
            break;
        }
    }
    candidate
}

/// 格式化字节数为人类可读字符串。
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.2} {}", UNITS[i])
    }
}
