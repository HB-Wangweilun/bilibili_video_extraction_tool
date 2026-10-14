//! B站视频解析工具 —— Tauri 后端入口与命令实现。

mod bilibili;
mod download;
mod ffmpeg;
mod watermark;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

use download::{human_size, ProgressState, Reporter};

/// 全局状态：HTTP 客户端 + 取消标志 + 忙碌标志。
pub struct AppState {
    client: reqwest::Client,
    cancel: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
}

fn err_text(e: anyhow::Error) -> String {
    format!("{e:#}")
}

// ---------------------------------------------------------------------------
// 解析类命令
// ---------------------------------------------------------------------------

/// 解析用户粘贴的链接，返回视频信息。
#[tauri::command]
async fn parse_video(
    app: AppHandle,
    input: String,
    sessdata: Option<String>,
) -> Result<bilibili::VideoInfo, String> {
    let client = app.state::<AppState>().client.clone();

    let parsed = bilibili::parse_input(&client, &input)
        .await
        .map_err(err_text)?;

    let (data, _bvid) = bilibili::fetch_view(
        &client,
        parsed.bvid.as_deref(),
        parsed.aid,
        sessdata.as_deref(),
    )
    .await
    .map_err(err_text)?;

    Ok(bilibili::build_video_info(&client, data, parsed.page).await)
}

/// 获取某个分 P 的可选清晰度列表。
#[tauri::command]
async fn get_play_info(
    app: AppHandle,
    bvid: String,
    cid: u64,
    mode: String,
    sessdata: Option<String>,
) -> Result<bilibili::PlayInfo, String> {
    let client = app.state::<AppState>().client.clone();
    let mode = if mode == "durl" { "durl" } else { "dash" };

    // 用 127 请求，服务端会返回当前账号实际可用的最高画质
    let data = bilibili::fetch_play_url(
        &client,
        &bvid,
        cid,
        mode,
        127,
        sessdata.as_deref().filter(|s| !s.trim().is_empty()),
    )
    .await
    .map_err(err_text)?;

    Ok(bilibili::build_play_info(&data, mode, sessdata.as_deref()))
}

// ---------------------------------------------------------------------------
// 下载
// ---------------------------------------------------------------------------

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub bvid: String,
    pub cid: u64,
    pub video_title: String,
    pub part_title: String,
    pub page_index: u32,
    pub total_pages: u32,
    /// 目标画质 id（dash 模式）
    pub quality_id: u32,
    /// 目标编码 codecid（7=AVC 12=HEVC 13=AV1）
    pub codecid: u32,
    /// 目标音轨 id，0 表示自动选最佳
    pub audio_id: u32,
    /// "dash" 高清（音视频合并）| "durl" 兼容（单文件 mp4）
    pub mode: String,
    pub sessdata: Option<String>,
    pub output_dir: String,
    #[serde(default)]
    pub est_video_size: u64,
    #[serde(default)]
    pub est_audio_size: u64,
    /// 视频流原始宽高（用于校验去水印区域是否越界）
    #[serde(default)]
    pub video_width: u32,
    #[serde(default)]
    pub video_height: u32,
    /// 要去除的水印区域（源像素坐标）；None 表示不去水印
    #[serde(default)]
    pub watermark: Option<watermark::WmBox>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DownloadResult {
    pub file_path: String,
    pub file_name: String,
    pub dir: String,
    pub size: u64,
    pub size_text: String,
    pub elapsed: f64,
    pub quality_text: String,
}

/// 开始下载。
#[tauri::command]
async fn start_download(app: AppHandle, req: DownloadRequest) -> Result<DownloadResult, String> {
    run_download(app, req).await.map_err(err_text)
}

/// 取消当前下载。
#[tauri::command]
fn cancel_download(app: AppHandle) {
    let st = app.state::<AppState>();
    st.cancel.store(true, Ordering::SeqCst);
}

/// 查询是否有任务在跑。
#[tauri::command]
fn is_busy(app: AppHandle) -> bool {
    app.state::<AppState>().busy.load(Ordering::SeqCst)
}

async fn run_download(app: AppHandle, req: DownloadRequest) -> Result<DownloadResult> {
    let started = Instant::now();
    let (client, cancel, busy) = {
        let st = app.state::<AppState>();
        (st.client.clone(), st.cancel.clone(), st.busy.clone())
    };

    if busy.swap(true, Ordering::SeqCst) {
        bail!("已有任务正在进行，请等它结束或先点「取消」");
    }
    cancel.store(false, Ordering::SeqCst);

    let progress = Arc::new(ProgressState::new());
    let reporter = Reporter::start(app.clone(), progress.clone());

    // 缓存目录拿不到就退回系统临时目录，保证下载仍能进行
    let cache_root = app
        .path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("bili-video-extractor"));
    let ffmpeg_path = ffmpeg::find_ffmpeg(&app).map(|(p, _)| p);

    let outcome = download_job(
        &cache_root,
        ffmpeg_path.as_deref(),
        &client,
        &cancel,
        &progress,
        &req,
    )
    .await;

    match &outcome {
        Ok(_) => {
            progress.set_stage("done");
            progress.set_message("下载完成");
        }
        Err(e) => {
            if cancel.load(Ordering::SeqCst) {
                progress.set_stage("cancelled");
                progress.set_message("已取消下载");
            } else {
                progress.set_stage("error");
                progress.set_message(format!("{e:#}"));
            }
        }
    }
    // 留出时间让进度上报任务把最终状态推给前端
    tokio::time::sleep(Duration::from_millis(450)).await;
    reporter.stop();
    busy.store(false, Ordering::SeqCst);

    let (out_path, quality_text) = outcome?;
    let size = tokio::fs::metadata(&out_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(DownloadResult {
        dir: out_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        file_name: out_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        file_path: out_path.to_string_lossy().to_string(),
        size,
        size_text: human_size(size),
        elapsed: started.elapsed().as_secs_f64(),
        quality_text,
    })
}

/// 下载主流程（与 Tauri 解耦，便于无界面自检复用）。
async fn download_job(
    cache_root: &Path,
    ffmpeg_path: Option<&Path>,
    client: &reqwest::Client,
    cancel: &Arc<AtomicBool>,
    progress: &Arc<ProgressState>,
    req: &DownloadRequest,
) -> Result<(PathBuf, String)> {
    // ---- 输出位置 ----
    let out_dir = PathBuf::from(req.output_dir.trim());
    if req.output_dir.trim().is_empty() {
        bail!("请先选择保存位置");
    }
    tokio::fs::create_dir_all(&out_dir)
        .await
        .with_context(|| format!("无法创建保存目录：{}", out_dir.display()))?;

    let base_name = if req.total_pages > 1 {
        format!("{}_P{}_{}", req.video_title, req.page_index, req.part_title)
    } else {
        req.video_title.clone()
    };
    let stem = download::sanitize_filename(&base_name, &format!("bilibili_{}", req.bvid));
    let final_path = download::unique_path(&out_dir, &stem, "mp4");
    let part_path = final_path.with_extension("part");

    // ---- 临时目录 ----
    let tmp = cache_root.join(format!("job_{}_{}", req.bvid, req.cid));
    let _ = tokio::fs::remove_dir_all(&tmp).await;
    tokio::fs::create_dir_all(&tmp)
        .await
        .context("无法创建临时目录")?;

    let result = download_job_inner(
        ffmpeg_path, client, cancel, progress, req, &tmp, &final_path, &part_path,
    )
    .await;

    let _ = tokio::fs::remove_dir_all(&tmp).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&part_path).await;
    }

    result
}

async fn download_job_inner(
    ffmpeg_path: Option<&Path>,
    client: &reqwest::Client,
    cancel: &Arc<AtomicBool>,
    progress: &Arc<ProgressState>,
    req: &DownloadRequest,
    tmp: &Path,
    final_path: &Path,
    part_path: &Path,
) -> Result<(PathBuf, String)> {
    progress.set_stage("preparing");
    progress.set_message("正在获取播放地址…");

    let sessdata = req.sessdata.as_deref().filter(|s| !s.trim().is_empty());
    let play = bilibili::fetch_play_url(
        client,
        &req.bvid,
        req.cid,
        if req.mode == "durl" { "durl" } else { "dash" },
        req.quality_id,
        sessdata,
    )
    .await?;

    // =====================================================================
    // 兼容模式：durl 单文件 MP4
    // =====================================================================
    if req.mode == "durl" {
        let segments = bilibili::pick_durl(&play)?;
        let api_total: u64 = segments.iter().map(|d| d.size).sum();
        progress.start_fresh(if api_total > 0 {
            api_total
        } else {
            req.est_video_size
        });

        let mut paths: Vec<PathBuf> = Vec::new();
        for (i, seg) in segments.iter().enumerate() {
            if cancel.load(Ordering::SeqCst) {
                bail!("已取消");
            }
            let p = tmp.join(format!("seg_{i:03}.mp4"));
            let urls = bilibili::durl_urls(seg);
            let label = if segments.len() > 1 {
                format!("第 {}/{} 段", i + 1, segments.len())
            } else {
                "视频文件".to_string()
            };
            download::download_resource(
                client, &urls, &p, progress, cancel, "file", &label,
            )
            .await?;
            paths.push(p);
        }

        progress.set_stage("merging");
        progress.set_message("正在整理输出文件…");

        if paths.len() == 1 {
            move_file(&paths[0], final_path).await?;
        } else {
            let ff = ffmpeg_path.ok_or_else(|| {
                anyhow!("该视频被 B 站切分成多段，需要 ffmpeg 才能拼接为一个 MP4。请在「设置」中安装 ffmpeg。")
            })?;
            progress.set_message("正在拼接视频分段…");
            ffmpeg::concat_mp4(ff, &paths, final_path).await?;
        }

        return Ok((final_path.to_path_buf(), "兼容模式 · 单文件 MP4".to_string()));
    }

    // =====================================================================
    // 高清模式：DASH 音视频分离，需要 ffmpeg 合并
    // =====================================================================
    let ffmpeg_path = ffmpeg_path.ok_or_else(|| {
        anyhow!("高清模式需要 ffmpeg 来合并音视频流。请在「设置」里一键安装 ffmpeg，或切换到「兼容模式」直接下载 MP4。")
    })?;

    let video = bilibili::pick_video_stream(&play, req.quality_id, req.codecid)?;
    let video_urls = video.urls();
    let video_codecid = video.codecid;
    let video_quality = video.id;
    let quality_text = format!(
        "{} · {}",
        bilibili::quality_label(video_quality),
        bilibili::codec_label(video.codecid, video.codecs.as_deref())
    );

    if video_urls.is_empty() {
        bail!("没有拿到视频流地址，请重试");
    }

    let audio = bilibili::pick_audio_stream(&play, req.audio_id);
    let audio_urls = audio.map(|a| a.urls()).unwrap_or_default();
    let audio_label = audio
        .map(|a| bilibili::audio_label(a.id))
        .unwrap_or_else(|| "无音轨".to_string());

    // 预先探测体积，让进度条从一开始就是准的
    progress.set_stage("downloading");
    progress.set_message("正在探测文件大小…");
    let mut grand_total = 0u64;
    if let Ok((s, _)) = download::probe_size(client, &video_urls[0]).await {
        grand_total += s;
    }
    if let Some(u) = audio_urls.first() {
        if let Ok((s, _)) = download::probe_size(client, u).await {
            grand_total += s;
        }
    }
    if grand_total == 0 {
        grand_total = req.est_video_size + req.est_audio_size;
    }
    progress.start_fresh(grand_total);

    // 下载视频流
    let video_file = tmp.join("video.mp4");
    let size_v = download::download_resource(
        client,
        &video_urls,
        &video_file,
        progress,
        cancel,
        "video",
        "视频流",
    )
    .await?;

    // 下载音频流
    let mut audio_file: Option<PathBuf> = None;
    if !audio_urls.is_empty() {
        let af = tmp.join("audio.mp4");
        match download::download_resource(
            client, &audio_urls, &af, progress, cancel, "audio", "音频流",
        )
        .await
        {
            Ok(_) => audio_file = Some(af),
            Err(e) => {
                // 音频失败不算致命：至少把画面给用户
                progress.set_message(format!("音频下载失败（将输出无声视频）：{e}"));
            }
        }
    }

    // 合并
    progress.set_stage("merging");
    progress.set_phase("merge");

    // 选了去水印就走 delogo 重编码路径
    let wm = req.watermark.filter(|b| b.w > 1 && b.h > 1);
    if let Some(b) = wm {
        let duration =
            bilibili::build_play_info(&play, "dash", None).duration as f64;
        let frame = if req.video_width > 1 && req.video_height > 1 {
            (req.video_width, req.video_height)
        } else {
            (video.width, video.height)
        };
        progress.set_message("正在去除水印并重新编码视频（会比普通下载慢很多，请耐心等待）…");
        progress.set_percent_progress(0.0);
        ffmpeg::merge_with_delogo(
            ffmpeg_path,
            &video_file,
            audio_file.as_deref(),
            part_path,
            (b.x, b.y, b.w, b.h),
            frame,
            duration,
            progress,
        )
        .await?;
    } else {
        progress.set_message("正在合并音视频（不重新编码，很快）…");
        if let Some(af) = audio_file.as_ref() {
            ffmpeg::merge_to_mp4(
                ffmpeg_path,
                &video_file,
                Some(af),
                part_path,
                video_codecid,
            )
            .await?;
        } else {
            ffmpeg::remux_single(ffmpeg_path, &video_file, part_path).await?;
        }
    }

    if size_v == 0 {
        bail!("视频流下载为空，请重试");
    }

    move_file(part_path, final_path).await?;

    Ok((
        final_path.to_path_buf(),
        format!(
            "{quality_text} · {audio_label}{}",
            if wm.is_some() { " · 已去水印" } else { "" }
        ),
    ))
}

/// 同一目录内改名；跨盘时退化为复制 + 删除。
async fn move_file(src: &Path, dst: &Path) -> Result<()> {
    if tokio::fs::rename(src, dst).await.is_ok() {
        return Ok(());
    }
    tokio::fs::copy(src, dst)
        .await
        .with_context(|| format!("写入文件失败：{}", dst.display()))?;
    let _ = tokio::fs::remove_file(src).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// ffmpeg / 系统工具命令
// ---------------------------------------------------------------------------

#[tauri::command]
async fn check_ffmpeg(app: AppHandle) -> ffmpeg::FfmpegStatus {
    ffmpeg::status(&app).await
}

#[tauri::command]
async fn install_ffmpeg(app: AppHandle) -> Result<String, String> {
    let client = app.state::<AppState>().client.clone();
    let path = ffmpeg::install(&app, &client).await.map_err(err_text)?;
    Ok(path.to_string_lossy().to_string())
}

/// 默认保存目录：系统下载目录，失败则退回应用数据目录。
#[tauri::command]
fn get_default_output_dir(app: AppHandle) -> String {
    if let Ok(p) = app.path().download_dir() {
        return p.to_string_lossy().to_string();
    }
    if let Ok(p) = app.path().app_data_dir() {
        let d = p.join("downloads");
        let _ = std::fs::create_dir_all(&d);
        return d.to_string_lossy().to_string();
    }
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// 在文件管理器中定位文件（若传目录则直接打开目录）。
#[tauri::command]
fn open_in_folder(path: String) -> Result<(), String> {
    let raw = path.trim().to_string();
    if raw.is_empty() {
        return Err("没有可打开的路径".into());
    }
    let p = PathBuf::from(&raw);

    // 1) 首选系统原生接口（SHOpenFolderAndSelectItems），能在资源管理器中直接选中该文件。
    if tauri_plugin_opener::reveal_item_in_dir(&p).is_ok() {
        return Ok(());
    }

    // 2) 退一步：直接打开它所在的目录。
    let dir = if p.is_dir() {
        p.clone()
    } else {
        p.parent().map(|x| x.to_path_buf()).unwrap_or_else(|| p.clone())
    };
    if tauri_plugin_opener::open_path(&dir, Option::<&str>::None).is_ok() {
        return Ok(());
    }

    // 3) 最后兜底：explorer。
    #[cfg(windows)]
    {
        return explorer_open(&p, &dir);
    }
    #[cfg(not(windows))]
    {
        Err(format!("打开文件夹失败：{}", dir.display()))
    }
}

/// 用 explorer 打开目录 / 选中文件。
///
/// 关键：**路径本身不要带引号**。Rust 在 Windows 上按 VC++ 规则转义参数，
/// 若我们自己在参数里写 `"`，会被转成 `\"`，而 explorer 不认这套规则，
/// 解析失败后会退回打开默认目录（看起来就像"打开了桌面"）。
#[cfg(windows)]
fn explorer_open(file: &Path, dir: &Path) -> Result<(), String> {
    let arg = if file.is_file() {
        format!("/select,{}", file.display())
    } else {
        dir.display().to_string()
    };
    std::process::Command::new("explorer")
        .arg(arg)
        .spawn()
        .map_err(|e| format!("打开文件夹失败：{e}"))?;
    Ok(())
}

/// 用系统默认播放器打开下载好的视频。
#[tauri::command]
fn play_file(path: String) -> Result<(), String> {
    let p = PathBuf::from(path.trim());
    if !p.is_file() {
        return Err("文件不存在或已被移动".into());
    }
    // 交给系统按默认关联程序打开，跨平台且不会被命令行转义坑到
    tauri_plugin_opener::open_path(&p, Option::<&str>::None)
        .map_err(|e| format!("无法打开文件：{e}"))
}

// ---------------------------------------------------------------------------
// 水印：抽帧预览 + 自动定位
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WatermarkPreview {
    /// 预览画面（PNG data url）
    pub frame: Option<String>,
    /// 视频流原始像素尺寸
    pub src_width: u32,
    pub src_height: u32,
    /// 预览画面的实际像素尺寸
    pub disp_width: u32,
    pub disp_height: u32,
    /// 自动检测到的水印区域（源像素坐标）
    pub detected: Option<watermark::WmBox>,
    /// 附加说明（例如检测失败的原因）
    pub note: String,
}

/// 从 PNG 头部读尺寸。
fn png_dims(buf: &[u8]) -> Option<(u32, u32)> {
    if buf.len() < 24 || &buf[0..8] != b"\x89PNG\r\n\x1a\n" || &buf[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]);
    let h = u32::from_be_bytes([buf[20], buf[21], buf[22], buf[23]]);
    Some((w, h))
}

#[tauri::command]
async fn detect_watermark(
    app: AppHandle,
    bvid: String,
    cid: u64,
    quality_id: u32,
    codecid: u32,
    sessdata: Option<String>,
) -> Result<WatermarkPreview, String> {
    let client = app.state::<AppState>().client.clone();
    let (ffmpeg, _) = ffmpeg::find_ffmpeg(&app).ok_or_else(|| {
        "水印检测与去除都需要 ffmpeg，请先在「设置」里安装 ffmpeg。".to_string()
    })?;
    let cache = app
        .path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("bili-video-extractor"))
        .join("wm_preview");

    detect_watermark_core(
        &cache,
        &ffmpeg,
        &client,
        &bvid,
        cid,
        quality_id,
        codecid,
        sessdata.as_deref(),
    )
    .await
    .map_err(err_text)
}

/// 水印检测核心（与 Tauri 解耦，自检也能复用）。
#[allow(clippy::too_many_arguments)]
async fn detect_watermark_core(
    cache: &Path,
    ffmpeg: &Path,
    client: &reqwest::Client,
    bvid: &str,
    cid: u64,
    quality_id: u32,
    codecid: u32,
    sessdata: Option<&str>,
) -> Result<WatermarkPreview> {
    let _ = tokio::fs::remove_dir_all(cache).await;
    tokio::fs::create_dir_all(cache)
        .await
        .context("无法创建预览缓存目录")?;

    let result = detect_watermark_body(cache, ffmpeg, client, bvid, cid, quality_id, codecid, sessdata)
        .await;
    let _ = tokio::fs::remove_dir_all(cache).await;
    result
}

#[allow(clippy::too_many_arguments)]
async fn detect_watermark_body(
    cache: &Path,
    ffmpeg: &Path,
    client: &reqwest::Client,
    bvid: &str,
    cid: u64,
    quality_id: u32,
    codecid: u32,
    sessdata: Option<&str>,
) -> Result<WatermarkPreview> {
    let play =
        bilibili::fetch_play_url(client, bvid, cid, "dash", quality_id, sessdata).await?;
    let video = bilibili::pick_video_stream(&play, quality_id, codecid)?;
    let urls = video.urls();
    if urls.is_empty() {
        bail!("没有拿到视频流地址");
    }

    // 只抓前面一段数据用于抽帧，不整段下载
    const PREFIX: u64 = 18 * 1024 * 1024;
    let part = cache.join("prefix.mp4");
    let got = download::download_prefix_multi(client, &urls, &part, PREFIX).await?;
    if got < 64 * 1024 {
        bail!("抓取到的视频数据太少，无法检测水印");
    }

    let (src_w, src_h) = if video.width > 1 && video.height > 1 {
        (video.width, video.height)
    } else {
        (1920, 1080)
    };

    // 1) 预览画面（过宽的缩到 900，便于前端显示）
    let preview_w = if src_w > 900 { Some(900u32) } else { None };
    let png = ffmpeg::extract_frame_png(ffmpeg, &part, 1.5, preview_w).await?;
    let (disp_w, disp_h) = png_dims(&png).unwrap_or((src_w, src_h));
    // 顺便把预览图落盘一份，便于排查
    let _ = tokio::fs::write(cache.join("preview.png"), &png).await;
    let frame = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&png)
    );

    // 2) 多帧自动定位
    const AW: u32 = 480;
    let ah = (((src_h as f64 * AW as f64 / src_w as f64) / 2.0).round() * 2.0).max(2.0) as u32;
    let detected = match ffmpeg::extract_frames_raw(ffmpeg, &part, 2.0, AW, ah, 14).await {
        Ok(frames) => {
            let frame_len = (AW * ah * 3) as usize;
            let n = frames.len() / frame_len;
            println!("[watermark] 分析 {n} 帧，尺寸 {AW}x{ah}");
            if n >= 3 {
                watermark::detect(&frames[..n * frame_len], n, AW, ah)
                    .map(|b| watermark::scale_and_pad(&b, AW, src_w, src_h))
            } else {
                None
            }
        }
        Err(e) => {
            println!("[watermark] 抽帧失败：{e:#}");
            None
        }
    };

    let note = if detected.is_some() {
        String::new()
    } else {
        "没能自动定位水印：可能这一段画面比较静止，或该视频本身没有水印。可以手动框选。".into()
    };

    Ok(WatermarkPreview {
        frame: Some(frame),
        src_width: src_w,
        src_height: src_h,
        disp_width: disp_w,
        disp_height: disp_h,
        detected,
        note,
    })
}

// ---------------------------------------------------------------------------
// 无界面自检：`BiliVideoExtractor.exe --selftest <链接> [输出目录]`
// 用真实的下载 / 合并代码跑一遍完整链路，方便排查问题。
// ---------------------------------------------------------------------------

pub fn selftest(url: &str, out_dir: Option<&str>, remove_watermark: bool) -> i32 {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[selftest] 创建运行时失败：{e}");
            return 1;
        }
    };
    match rt.block_on(selftest_inner(url, out_dir, remove_watermark)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("[selftest] 失败：{e:#}");
            1
        }
    }
}

async fn selftest_inner(url: &str, out_dir: Option<&str>, remove_watermark: bool) -> Result<()> {
    let client = bilibili::build_client()?;

    println!("[1/5] 解析链接：{url}");
    let parsed = bilibili::parse_input(&client, url).await?;
    println!(
        "      bvid={:?}  aid={:?}  page={:?}",
        parsed.bvid, parsed.aid, parsed.page
    );

    println!("[2/5] 获取视频信息");
    let (data, bvid) =
        bilibili::fetch_view(&client, parsed.bvid.as_deref(), parsed.aid, None).await?;
    let info = bilibili::build_video_info(&client, data, parsed.page).await;
    println!("      标题：{}", info.title);
    println!(
        "      UP主：{}  时长：{}s  分P：{}",
        info.owner_name,
        info.duration,
        info.pages.len()
    );
    println!(
        "      封面：{}",
        if info.cover.is_some() {
            "已获取（base64）"
        } else {
            "无"
        }
    );

    let page = info
        .pages
        .get(info.selected_page.saturating_sub(1) as usize)
        .or_else(|| info.pages.first())
        .ok_or_else(|| anyhow!("该视频没有分P信息"))?
        .clone();

    println!("[3/5] 获取播放地址");
    let play = bilibili::fetch_play_url(&client, &bvid, page.cid, "dash", 127, None).await?;
    let pinfo = bilibili::build_play_info(&play, "dash", None);
    println!(
        "      可用清晰度：{:?}",
        pinfo
            .qualities
            .iter()
            .map(|q| format!("{}({})", q.label, q.id))
            .collect::<Vec<_>>()
    );
    println!(
        "      可用音轨：{:?}",
        pinfo
            .audios
            .iter()
            .map(|a| format!("{}({})", a.label, a.id))
            .collect::<Vec<_>>()
    );

    let video = bilibili::pick_video_stream(&play, 127, 7)?;
    println!("      首选视频流：id={} codecid={}", video.id, video.codecid);

    println!("[4/5] 定位 ffmpeg");
    let ffmpeg_path = ffmpeg::find_ffmpeg_in(&[]).map(|(p, s)| {
        println!("      {}  （来源：{s}）", p.display());
        p
    });
    if ffmpeg_path.is_none() {
        println!("      ⚠ 未找到 ffmpeg，高清模式会失败（可用兼容模式 durl）");
    }

    // 可选：自动检测水印并准备去除
    let mut watermark_box = None;
    if remove_watermark {
        println!("[4.5] 自动检测水印位置");
        match ffmpeg_path.as_ref() {
            None => println!("      ⚠ 没有 ffmpeg，跳过"),
            Some(ff) => {
                let cache = std::env::temp_dir().join("bili_selftest_wm");
                match detect_watermark_core(
                    &cache, ff, &client, &bvid, page.cid, video.id, video.codecid, None,
                )
                .await
                {
                    Ok(p) => {
                        println!(
                            "      源尺寸 {}x{}  预览 {}x{}",
                            p.src_width, p.src_height, p.disp_width, p.disp_height
                        );
                        match p.detected {
                            Some(b) => {
                                println!(
                                    "      ✅ 检测到水印区域 x={} y={} w={} h={}",
                                    b.x, b.y, b.w, b.h
                                );
                                println!("      预览图：{}", cache.join("preview.png").display());
                                watermark_box = Some(b);
                            }
                            None => println!("      ⚠ 未检测到水印：{}", p.note),
                        }
                    }
                    Err(e) => println!("      ⚠ 检测失败：{e:#}"),
                }
            }
        }
    }

    let dir = out_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("bili_selftest_out"));
    let cache = std::env::temp_dir().join("bili_selftest_cache");
    println!("      输出目录：{}", dir.display());

    let req = DownloadRequest {
        bvid: bvid.clone(),
        cid: page.cid,
        video_title: info.title.clone(),
        part_title: page.part.clone(),
        page_index: page.page,
        total_pages: info.pages.len() as u32,
        quality_id: 127,
        codecid: 7,
        audio_id: 0,
        mode: "dash".into(),
        sessdata: None,
        output_dir: dir.to_string_lossy().to_string(),
        est_video_size: 0,
        est_audio_size: 0,
        video_width: video.width,
        video_height: video.height,
        watermark: watermark_box,
    };

    let progress = Arc::new(ProgressState::new());
    let cancel = Arc::new(AtomicBool::new(false));

    let printer = {
        let p = progress.clone();
        tokio::spawn(async move {
            let mut last = String::new();
            loop {
                tokio::time::sleep(Duration::from_millis(400)).await;
                let (stage, phase, dl, total, msg) = p.snapshot();
                let pct = if total > 0 {
                    dl as f64 / total as f64 * 100.0
                } else {
                    0.0
                };
                let line = if phase == "merge" {
                    format!("      {stage}/{phase} {pct:.1}%  {msg}")
                } else {
                    format!(
                        "      {stage}/{phase} {pct:.1}%  {}/{}  {msg}",
                        human_size(dl),
                        human_size(total)
                    )
                };
                if line != last {
                    println!("{line}");
                    last = line;
                }
            }
        })
    };

    println!("[5/5] 下载并合并为 MP4 …");
    let (path, quality) = download_job(
        &cache,
        ffmpeg_path.as_deref(),
        &client,
        &cancel,
        &progress,
        &req,
    )
    .await?;
    printer.abort();

    let size = tokio::fs::metadata(&path).await.map(|m| m.len()).unwrap_or(0);
    println!("      完成：{}", path.display());
    println!("      体积：{}  （{quality}）", human_size(size));
    Ok(())
}

// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let client = bilibili::build_client().expect("初始化网络客户端失败");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            client,
            cancel: Arc::new(AtomicBool::new(false)),
            busy: Arc::new(AtomicBool::new(false)),
        })
        .invoke_handler(tauri::generate_handler![
            parse_video,
            get_play_info,
            detect_watermark,
            start_download,
            cancel_download,
            is_busy,
            check_ffmpeg,
            install_ffmpeg,
            get_default_output_dir,
            open_in_folder,
            play_file,
        ])
        .run(tauri::generate_context!())
        .expect("启动应用失败");
}
