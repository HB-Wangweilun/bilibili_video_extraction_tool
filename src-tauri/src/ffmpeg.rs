//! ffmpeg 定位、一键安装（多源下载）与 MP4 封装。

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 构造 ffmpeg 命令：吞掉 stdin、捕获 stdout/stderr，Windows 下隐藏控制台窗口。
///
/// 先用 `std::process::Command` 组装（这样才能用 `creation_flags`），
/// 再交给 tokio 异步执行。
fn base_command(ffmpeg: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// 异步执行 ffmpeg 命令并收集输出。
async fn run_ffmpeg(
    cmd: std::process::Command,
    timeout_secs: u64,
    what: &str,
) -> Result<std::process::Output> {
    let fut = tokio::process::Command::from(cmd).output();
    tokio::time::timeout(Duration::from_secs(timeout_secs), fut)
        .await
        .map_err(|_| anyhow!("{what}超时"))?
        .map_err(|e| anyhow!("启动 ffmpeg 失败：{e}"))
}

/// 下载源（按顺序尝试）。
///
/// 1. 国内镜像的 tar.gz 包，解压出 `<包名>/ffmpeg.exe`，速度通常几 MB/s；
/// 2. GitHub 加速代理的裸 exe；
/// 3. GitHub 直连裸 exe。
const SOURCES: &[(&str, SourceKind)] = &[
    (
        "https://registry.npmmirror.com/@ffmpeg-installer/win32-x64/-/win32-x64-4.1.0.tgz",
        SourceKind::TarGz,
    ),
    (
        "https://ghfast.top/https://github.com/eugeneware/ffmpeg-static/releases/download/b6.0/ffmpeg-win32-x64",
        SourceKind::RawExe,
    ),
    (
        "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.0/ffmpeg-win32-x64",
        SourceKind::RawExe,
    ),
];

#[derive(Clone, Copy)]
enum SourceKind {
    TarGz,
    RawExe,
}

#[derive(Serialize, Clone)]
pub struct FfmpegStatus {
    pub found: bool,
    pub path: Option<String>,
    pub source: Option<String>,
    pub version: Option<String>,
}

#[derive(Serialize, Clone)]
struct InstallProgress {
    downloaded: u64,
    total: u64,
    percent: f64,
    message: String,
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

/// 应用数据目录下的 ffmpeg 路径。
pub fn managed_ffmpeg_path(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow!("无法定位应用数据目录：{e}"))?
        .join("bin");
    Ok(dir.join(exe_name()))
}

/// 在「应用内置目录 → 应用数据目录 → exe 同级 → 常见安装位置 → 系统 PATH」中查找 ffmpeg。
///
/// `extra_dirs` 由调用方（Tauri 层）提供，元素既可以是目录也可以是 ffmpeg 的完整路径。
pub fn find_ffmpeg_in(extra_dirs: &[PathBuf]) -> Option<(PathBuf, String)> {
    let mut candidates: Vec<(PathBuf, String)> = Vec::new();

    for d in extra_dirs {
        candidates.push((d.clone(), "应用内置".into()));
        candidates.push((d.join(exe_name()), "应用内置".into()));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push((dir.join(exe_name()), "程序同级目录".into()));
            candidates.push((dir.join("bin").join(exe_name()), "程序同级目录".into()));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push((cwd.join(exe_name()), "当前目录".into()));
        candidates.push((cwd.join("bin").join(exe_name()), "当前目录".into()));
        candidates.push((cwd.join(".tools").join(exe_name()), "开发目录 .tools".into()));
        if let Some(parent) = cwd.parent() {
            candidates.push((
                parent.join(".tools").join(exe_name()),
                "开发目录 .tools".into(),
            ));
        }
    }

    // 常见第三方安装位置
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        candidates.push((
            PathBuf::from(&local)
                .join(r"Microsoft\WinGet\Links")
                .join(exe_name()),
            "WinGet".into(),
        ));
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        candidates.push((
            PathBuf::from(&profile).join(r"scoop\shims").join(exe_name()),
            "Scoop".into(),
        ));
    }
    candidates.push((
        PathBuf::from(r"C:\ffmpeg\bin").join(exe_name()),
        "C:\\ffmpeg".into(),
    ));
    candidates.push((
        PathBuf::from(r"C:\ProgramData\chocolatey\bin").join(exe_name()),
        "Chocolatey".into(),
    ));

    for (p, src) in candidates {
        if p.is_file() {
            return Some((p, src));
        }
    }

    // 系统 PATH
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(exe_name());
            if p.is_file() {
                return Some((p, "系统 PATH".into()));
            }
        }
    }

    None
}

/// Tauri 层入口：附加应用自身的资源目录与应用数据目录。
pub fn find_ffmpeg(app: &AppHandle) -> Option<(PathBuf, String)> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        dirs.push(res);
    }
    if let Ok(p) = managed_ffmpeg_path(app) {
        dirs.push(p);
    }
    find_ffmpeg_in(&dirs)
}

/// 读取 ffmpeg 版本（首行）。
pub async fn ffmpeg_version(path: &Path) -> Option<String> {
    let mut cmd = base_command(path);
    cmd.arg("-version");
    let out = run_ffmpeg(cmd, 15, "读取版本").await.ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

/// 收集当前 ffmpeg 状态（供前端展示）。
pub async fn status(app: &AppHandle) -> FfmpegStatus {
    match find_ffmpeg(app) {
        Some((path, source)) => {
            let version = ffmpeg_version(&path).await;
            FfmpegStatus {
                found: true,
                path: Some(path.to_string_lossy().to_string()),
                source: Some(source),
                version,
            }
        }
        None => FfmpegStatus {
            found: false,
            path: None,
            source: None,
            version: None,
        },
    }
}

/// 下载并安装 ffmpeg 到应用数据目录，成功返回安装后的路径。
pub async fn install(app: &AppHandle, client: &reqwest::Client) -> Result<PathBuf> {
    let dest = managed_ffmpeg_path(app)?;
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("无法创建目录 {}", parent.display()))?;
    }

    let mut errors: Vec<String> = Vec::new();

    for (idx, (url, kind)) in SOURCES.iter().enumerate() {
        let tmp = dest.with_extension(format!("part{idx}"));
        let _ = tokio::fs::remove_file(&tmp).await;

        let label = match kind {
            SourceKind::TarGz => "压缩包（国内镜像）",
            SourceKind::RawExe if idx == 1 => "可执行文件（加速线路）",
            SourceKind::RawExe => "可执行文件（直连）",
        };

        let attempt = async {
            download_to(app, client, url, &tmp, label).await?;
            if matches!(kind, SourceKind::TarGz) {
                let tmp2 = tmp.clone();
                let dest2 = dest.clone();
                tokio::task::spawn_blocking(move || extract_targz_ffmpeg(&tmp2, &dest2))
                    .await
                    .context("解压任务异常")??;
            } else {
                tokio::fs::rename(&tmp, &dest)
                    .await
                    .context("保存 ffmpeg 失败")?;
            }
            if !validate_exe(&dest).await {
                bail!("下载到的文件不是有效的 ffmpeg 可执行文件");
            }
            Ok::<(), anyhow::Error>(())
        }
        .await;

        let _ = tokio::fs::remove_file(&tmp).await;

        match attempt {
            Ok(()) => {
                let _ = app.emit(
                    "ffmpeg-progress",
                    InstallProgress {
                        downloaded: 1,
                        total: 1,
                        percent: 100.0,
                        message: "安装完成".into(),
                    },
                );
                return Ok(dest);
            }
            Err(e) => {
                errors.push(format!("{label}：{e}"));
                let _ = tokio::fs::remove_file(&dest).await;
            }
        }
    }

    Err(anyhow!(
        "ffmpeg 下载失败，已尝试 {} 个下载源：\n{}\n\n可以手动下载 ffmpeg.exe 放到：{}",
        SOURCES.len(),
        errors.join("\n"),
        dest.display()
    ))
}

/// 校验文件头是否为 Windows PE（MZ）且体积合理。
async fn validate_exe(path: &Path) -> bool {
    match tokio::fs::metadata(path).await {
        Ok(m) if m.len() > 5 * 1024 * 1024 => {}
        _ => return false,
    }
    let mut buf = [0u8; 2];
    match tokio::fs::File::open(path).await {
        Ok(mut f) => {
            use tokio::io::AsyncReadExt;
            matches!(f.read_exact(&mut buf).await, Ok(_)) && buf == *b"MZ"
        }
        Err(_) => false,
    }
}

fn extract_targz_ffmpeg(tgz: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(tgz).context("打开压缩包失败")?;
    let gz = flate2::read::GzDecoder::new(std::io::BufReader::new(file));
    let mut archive = tar::Archive::new(gz);

    for entry in archive.entries().context("解析压缩包失败")? {
        let mut entry = entry.context("读取压缩包条目失败")?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().context("读取条目路径失败")?.into_owned();
        let name = path.to_string_lossy().replace('\\', "/");
        if name.ends_with("ffmpeg.exe") {
            let mut out = std::fs::File::create(dest).context("写入 ffmpeg 失败")?;
            std::io::copy(&mut entry, &mut out).context("解压 ffmpeg 失败")?;
            return Ok(());
        }
    }
    bail!("压缩包里没有找到 ffmpeg.exe")
}

/// 下载文件并推送进度事件。
async fn download_to(
    app: &AppHandle,
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    label: &str,
) -> Result<()> {
    let resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, "BiliVideoExtractor/0.1")
        .send()
        .await
        .context("连接下载服务器失败")?;
    if !resp.status().is_success() {
        bail!("服务器返回 {}", resp.status());
    }

    let total = resp.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("无法创建文件 {}", dest.display()))?;
    let mut stream = resp.bytes_stream();
    let mut downloaded = 0u64;
    let mut last_emit = std::time::Instant::now();

    while let Some(item) = stream.next().await {
        let bytes = item.context("下载中断")?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await?;
        downloaded += bytes.len() as u64;
        if last_emit.elapsed() >= Duration::from_millis(250) {
            last_emit = std::time::Instant::now();
            let _ = app.emit(
                "ffmpeg-progress",
                InstallProgress {
                    downloaded,
                    total,
                    percent: if total > 0 {
                        (downloaded as f64 / total as f64 * 100.0).min(100.0)
                    } else {
                        0.0
                    },
                    message: format!("正在下载 {label}…"),
                },
            );
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    Ok(())
}

/// 合并视频流与音频流为 mp4（不重新编码）。
pub async fn merge_to_mp4(
    ffmpeg: &Path,
    video: &Path,
    audio: Option<&Path>,
    out: &Path,
    video_codecid: u32,
) -> Result<()> {
    let mut cmd = base_command(ffmpeg);
    cmd.arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-nostdin")
        .arg("-i")
        .arg(video);
    if let Some(a) = audio {
        cmd.arg("-i").arg(a);
    }
    cmd.arg("-map").arg("0:v:0");
    if audio.is_some() {
        cmd.arg("-map").arg("1:a:0");
    }
    cmd.arg("-c").arg("copy");
    if video_codecid == 12 {
        // HEVC 打上 hvc1 标记，提升播放器兼容性
        cmd.arg("-tag:v").arg("hvc1");
    }
    cmd.arg("-movflags").arg("+faststart");
    cmd.arg("-f").arg("mp4");
    cmd.arg(out);

    let res = run_ffmpeg(cmd, 7200, "合并").await?;
    if !res.status.success() {
        bail!("ffmpeg 合并失败：{}", last_error_line(&res.stderr));
    }
    if !out.is_file() {
        bail!("ffmpeg 没有生成输出文件");
    }
    Ok(())
}

/// 单流直接封装（无音轨或纯音频场景）。
pub async fn remux_single(ffmpeg: &Path, input: &Path, out: &Path) -> Result<()> {
    let mut cmd = base_command(ffmpeg);
    cmd.arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-nostdin")
        .arg("-i")
        .arg(input)
        .arg("-c")
        .arg("copy")
        .arg("-movflags")
        .arg("+faststart")
        .arg("-f")
        .arg("mp4")
        .arg(out);

    let res = run_ffmpeg(cmd, 3600, "封装").await?;
    if !res.status.success() {
        bail!("ffmpeg 封装失败：{}", last_error_line(&res.stderr));
    }
    Ok(())
}

/// 拼接多个分段为单个 mp4（durl 多段场景）。
pub async fn concat_mp4(ffmpeg: &Path, segments: &[PathBuf], out: &Path) -> Result<()> {
    if segments.is_empty() {
        bail!("没有可拼接的分段");
    }
    if segments.len() == 1 {
        return remux_single(ffmpeg, &segments[0], out).await;
    }

    // 生成 concat 清单（路径统一用正斜杠，避免 Windows 反斜杠转义问题）
    let list_path = out.with_extension("concat.txt");
    let mut content = String::new();
    for seg in segments {
        let p = seg
            .to_string_lossy()
            .replace('\\', "/")
            .replace('\'', "'\\''");
        content.push_str(&format!("file '{p}'\n"));
    }
    tokio::fs::write(&list_path, content)
        .await
        .context("写入拼接清单失败")?;

    let mut cmd = base_command(ffmpeg);
    cmd.arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-nostdin")
        .arg("-f")
        .arg("concat")
        .arg("-safe")
        .arg("0")
        .arg("-i")
        .arg(&list_path)
        .arg("-c")
        .arg("copy")
        .arg("-movflags")
        .arg("+faststart")
        .arg("-f")
        .arg("mp4")
        .arg(out);

    let res = run_ffmpeg(cmd, 3600, "拼接").await;

    let _ = tokio::fs::remove_file(&list_path).await;
    let res = res?;

    if !res.status.success() {
        bail!("ffmpeg 拼接失败：{}", last_error_line(&res.stderr));
    }
    Ok(())
}

fn last_error_line(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .last()
        .unwrap_or("未知错误")
        .to_string()
}

// ---------------------------------------------------------------------------
// 抽帧（用于水印检测与预览）
// ---------------------------------------------------------------------------

/// 按固定帧率抽取若干帧，输出 **raw RGB24** 字节。
///
/// 用 raw 而不是 PNG，是为了在 Rust 侧直接读像素做分析，避免引入图像解码库。
/// 返回的字节长度 = 帧数 × out_w × out_h × 3。
pub async fn extract_frames_raw(
    ffmpeg: &Path,
    input: &Path,
    fps: f64,
    out_w: u32,
    out_h: u32,
    max_frames: usize,
) -> Result<Vec<u8>> {
    let mut cmd = base_command(ffmpeg);
    cmd.arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-nostdin")
        .arg("-i")
        .arg(input)
        .arg("-vf")
        .arg(format!("fps={fps},scale={out_w}:{out_h}"))
        .arg("-frames:v")
        .arg(max_frames.to_string())
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("rgb24")
        .arg("-");

    let out = run_ffmpeg(cmd, 600, "抽帧").await?;
    if !out.status.success() {
        bail!("抽帧失败：{}", last_error_line(&out.stderr));
    }
    if out.stdout.is_empty() {
        bail!("没有抽到任何画面");
    }
    Ok(out.stdout)
}

/// 抽取单帧并编码为 PNG（用于前端可视化框选）。`scale_w` 为 Some 时按宽度缩放。
pub async fn extract_frame_png(
    ffmpeg: &Path,
    input: &Path,
    at_secs: f64,
    scale_w: Option<u32>,
) -> Result<Vec<u8>> {
    let seeks: &[f64] = if at_secs > 0.0 {
        &[at_secs, 0.0]
    } else {
        &[0.0]
    };

    let mut last_err = String::new();
    for s in seeks {
        let mut cmd = base_command(ffmpeg);
        cmd.arg("-y")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-nostdin");
        if *s > 0.0 {
            cmd.arg("-ss").arg(format!("{s}"));
        }
        cmd.arg("-i").arg(input);
        if let Some(w) = scale_w {
            cmd.arg("-vf").arg(format!("scale={w}:-2"));
        }
        cmd.arg("-frames:v")
            .arg("1")
            .arg("-f")
            .arg("image2pipe")
            .arg("-c:v")
            .arg("png")
            .arg("-");

        if let Ok(out) = run_ffmpeg(cmd, 180, "抽帧").await {
            if out.status.success() && !out.stdout.is_empty() {
                return Ok(out.stdout);
            }
            last_err = last_error_line(&out.stderr);
        }
    }
    bail!(
        "无法从视频流中提取画面（可能下载的前置数据太少）{}",
        if last_err.is_empty() {
            String::new()
        } else {
            format!("：{last_err}")
        }
    )
}

// ---------------------------------------------------------------------------
// 去水印合并（delogo 必须重编码视频）
// ---------------------------------------------------------------------------

/// 合并音视频，并用 `delogo` 抹掉指定区域的水印。
///
/// 与普通合并的区别：视频必须**重新编码**（水印是烧进像素的，无法 copy 掉），
/// 音频仍然直接复制。重编码较慢，因此这里解析 ffmpeg 的 progress 输出驱动进度条。
#[allow(clippy::too_many_arguments)]
pub async fn merge_with_delogo(
    ffmpeg: &Path,
    video: &Path,
    audio: Option<&Path>,
    out: &Path,
    region: (u32, u32, u32, u32),
    frame: (u32, u32),
    duration: f64,
    progress: &std::sync::Arc<crate::download::ProgressState>,
) -> Result<()> {
    use tokio::io::AsyncBufReadExt;

    // delogo 需要从框外一圈取样，因此框必须严格落在画面内部
    let (fw, fh) = (frame.0.max(2), frame.1.max(2));
    let mut x = region.0.min(fw.saturating_sub(2));
    let mut y = region.1.min(fh.saturating_sub(2));
    let mut w = region.2;
    let mut h = region.3;
    if w < 2 {
        w = 2;
    }
    if h < 2 {
        h = 2;
    }
    if x + w > fw.saturating_sub(1) {
        x = fw.saturating_sub(1).saturating_sub(w);
        if x < 1 {
            x = 1;
            w = fw.saturating_sub(2).max(2);
        }
    }
    if y + h > fh.saturating_sub(1) {
        y = fh.saturating_sub(1).saturating_sub(h);
        if y < 1 {
            y = 1;
            h = fh.saturating_sub(2).max(2);
        }
    }
    let filter = format!("delogo=x={x}:y={y}:w={w}:h={h}");

    let mut cmd = base_command(ffmpeg);
    cmd.arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-nostdin")
        .arg("-i")
        .arg(video);
    if let Some(a) = audio {
        cmd.arg("-i").arg(a);
    }
    cmd.arg("-map").arg("0:v:0");
    if audio.is_some() {
        cmd.arg("-map").arg("1:a:0");
    }
    cmd.arg("-vf")
        .arg(&filter)
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("veryfast")
        .arg("-crf")
        .arg("20")
        .arg("-pix_fmt")
        .arg("yuv420p");
    if audio.is_some() {
        cmd.arg("-c:a").arg("copy");
    }
    cmd.arg("-movflags")
        .arg("+faststart")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg("-f")
        .arg("mp4")
        .arg(out);

    let mut child = tokio::process::Command::from(cmd)
        .spawn()
        .map_err(|e| anyhow!("启动 ffmpeg 失败：{e}"))?;

    // stderr 单独读完，避免管道写满导致死锁
    let err_buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    if let Some(mut e) = child.stderr.take() {
        let buf = err_buf.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut s = String::new();
            let _ = e.read_to_string(&mut s).await;
            if let Ok(mut g) = buf.lock() {
                *g = s;
            }
        });
    }

    if let Some(stdout) = child.stdout.take() {
        let mut lines = tokio::io::BufReader::new(stdout).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if let Some(v) = line.strip_prefix("out_time_us=") {
                        if let Ok(us) = v.trim().parse::<i64>() {
                            if duration > 0.0 && us > 0 {
                                progress.set_percent_progress(
                                    us as f64 / 1_000_000.0 / duration * 100.0,
                                );
                            }
                        }
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    }

    let status = child.wait().await.context("等待 ffmpeg 结束失败")?;
    if !status.success() {
        let msg = err_buf.lock().map(|g| g.clone()).unwrap_or_default();
        bail!(
            "ffmpeg 去水印合并失败：{}",
            if msg.trim().is_empty() {
                "未知错误".to_string()
            } else {
                last_error_line(msg.as_bytes())
            }
        );
    }
    if !out.is_file() {
        bail!("ffmpeg 没有生成输出文件");
    }
    Ok(())
}
