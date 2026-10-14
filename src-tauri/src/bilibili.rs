//! B 站接口封装：链接解析、视频信息、播放地址获取。
//!
//! 所有请求统一携带桌面端 User-Agent 与 Referer，否则 CDN / 接口会返回 403。

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Duration;

pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
pub const REFERER: &str = "https://www.bilibili.com";

/// 构建通用 HTTP 客户端（API 调用用）。
pub fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(15))
        .pool_max_idle_per_host(16)
        .build()
        .context("初始化网络客户端失败")
}

// ---------------------------------------------------------------------------
// 链接解析
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ParsedInput {
    pub bvid: Option<String>,
    pub aid: Option<u64>,
    pub page: Option<u32>,
}

fn re_url() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"https?://[^\s\u{4e00}-\u{9fff}，、）】」"'<>]+"#).unwrap())
}
fn re_bv() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"BV[0-9A-Za-z]{10}").unwrap())
}
fn re_av() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)(?:^|[^0-9a-zA-Z])av(\d{1,19})").unwrap())
}
fn re_p() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[?&#]p=(\d+)").unwrap())
}

/// 把用户粘贴的任意文本解析成 BV 号 / av 号 + 分 P。
/// 支持：完整链接（带一堆跟踪参数）、b23.tv 短链、纯 BV 号、av 号、分享文案。
pub async fn parse_input(client: &reqwest::Client, raw: &str) -> Result<ParsedInput> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("请先粘贴 B 站视频链接");
    }

    let mut text = raw.to_string();

    // 1. 若是分享文案，先抽出其中第一个 URL
    if let Some(m) = re_url().find(&text) {
        text = m.as_str().to_string();
    }

    // 2. b23.tv 短链还原
    if text.contains("b23.tv") || text.contains("bili2233.cn") {
        let resolved = resolve_short_link(client, &text).await?;
        text = resolved;
    }

    let mut out = ParsedInput::default();

    if let Some(m) = re_bv().find(&text) {
        out.bvid = Some(m.as_str().to_string());
    } else if let Some(c) = re_av().captures(&text) {
        if let Ok(n) = c[1].parse::<u64>() {
            out.aid = Some(n);
        }
    }

    if out.bvid.is_none() && out.aid.is_none() {
        bail!("没能从这段内容里识别出 B 站视频号，请检查链接是否完整（应包含 BVxxxxxxxxxx）");
    }

    if let Some(c) = re_p().captures(&text) {
        if let Ok(n) = c[1].parse::<u32>() {
            if n >= 1 {
                out.page = Some(n);
            }
        }
    }

    Ok(out)
}

/// 跟随 b23.tv 短链跳转，拿到真实链接。
async fn resolve_short_link(client: &reqwest::Client, url: &str) -> Result<String> {
    let resp = client
        .get(url)
        .header(reqwest::header::REFERER, REFERER)
        .send()
        .await
        .context("展开短链失败，请检查网络")?;
    let final_url = resp.url().as_str().to_string();
    if final_url.contains("b23.tv") {
        bail!("短链展开失败，请直接把电脑浏览器地址栏里的完整链接粘过来");
    }
    Ok(final_url)
}

// ---------------------------------------------------------------------------
// 视频信息
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
pub struct PageInfo {
    pub cid: u64,
    pub page: u32,
    pub part: String,
    pub duration: u64,
}

#[derive(Serialize, Clone)]
pub struct VideoInfo {
    pub bvid: String,
    pub aid: u64,
    pub title: String,
    /// 封面（已转 base64 data url，前端无需联网即可显示）
    pub cover: Option<String>,
    pub owner_name: String,
    pub owner_face: Option<String>,
    pub duration: u64,
    pub pubdate: u64,
    pub view: u64,
    pub like: u64,
    pub pages: Vec<PageInfo>,
    /// 根据链接里的 ?p= 选中的分 P（1 起）
    pub selected_page: u32,
}

#[derive(Deserialize)]
struct ApiEnvelope<T> {
    code: i64,
    #[serde(default)]
    message: String,
    data: Option<T>,
}

#[derive(Deserialize)]
pub struct ViewData {
    bvid: String,
    aid: u64,
    title: String,
    #[serde(default)]
    pic: String,
    #[serde(default)]
    duration: u64,
    #[serde(default)]
    pubdate: u64,
    owner: ViewOwner,
    #[serde(default)]
    stat: Option<ViewStat>,
    #[serde(default)]
    pages: Vec<ViewPage>,
}

#[derive(Deserialize)]
struct ViewOwner {
    #[serde(default)]
    name: String,
    #[serde(default)]
    face: String,
}

#[derive(Deserialize)]
struct ViewStat {
    #[serde(default)]
    view: u64,
    #[serde(default)]
    like: u64,
}

#[derive(Deserialize)]
struct ViewPage {
    cid: u64,
    #[serde(default)]
    page: u32,
    #[serde(default)]
    part: String,
    #[serde(default)]
    duration: u64,
}

fn api_error(code: i64, message: &str) -> anyhow::Error {
    let hint = match code {
        -400 => "请求参数有误",
        -403 => "访问被拒绝（可能需要登录 Cookie）",
        -404 => "视频不存在或已被删除/设为私享",
        -352 => "触发了 B 站风控，请稍后重试，或填入 Cookie 后重试",
        -412 => "请求被 B 站拦截，请稍后重试",
        -10403 => "该视频当前地区不可访问",
        _ => "接口返回异常",
    };
    anyhow!("{hint}（code {code}{}）", if message.is_empty() { String::new() } else { format!(": {message}") })
}

/// 拉取视频基础信息。
pub async fn fetch_view(
    client: &reqwest::Client,
    bvid: Option<&str>,
    aid: Option<u64>,
    sessdata: Option<&str>,
) -> Result<(ViewData, String)> {
    let base = "https://api.bilibili.com/x/web-interface/view";
    let url = if let Some(b) = bvid {
        format!("{base}?bvid={b}")
    } else if let Some(a) = aid {
        format!("{base}?aid={a}")
    } else {
        bail!("缺少视频号");
    };

    let mut req = client.get(&url).header(reqwest::header::REFERER, REFERER);
    if let Some(cookie) = cookie_header(sessdata) {
        req = req.header(reqwest::header::COOKIE, cookie);
    }

    let resp = req.send().await.context("请求视频信息失败，请检查网络连接")?;
    let text = resp.text().await.context("读取视频信息失败")?;
    let env: ApiEnvelope<ViewData> =
        serde_json::from_str(&text).with_context(|| format!("解析视频信息失败：{}", truncate(&text, 200)))?;

    if env.code != 0 {
        return Err(api_error(env.code, &env.message));
    }
    let data = env.data.ok_or_else(|| anyhow!("接口未返回视频信息"))?;
    let bvid = data.bvid.clone();
    Ok((data, bvid))
}

/// 组装给前端的完整视频信息（含 base64 封面）。
pub async fn build_video_info(
    client: &reqwest::Client,
    data: ViewData,
    page_hint: Option<u32>,
) -> VideoInfo {
    let cover = fetch_data_url(client, &data.pic).await;
    let owner_face = fetch_data_url(client, &data.owner.face).await;

    let pages: Vec<PageInfo> = data
        .pages
        .iter()
        .map(|p| PageInfo {
            cid: p.cid,
            page: if p.page == 0 { 1 } else { p.page },
            part: p.part.clone(),
            duration: p.duration,
        })
        .collect();

    let selected_page = match page_hint {
        Some(n) if n >= 1 && (n as usize) <= pages.len() => n,
        _ => 1,
    };

    VideoInfo {
        bvid: data.bvid,
        aid: data.aid,
        title: data.title,
        cover,
        owner_name: data.owner.name,
        owner_face,
        duration: data.duration,
        pubdate: data.pubdate,
        view: data.stat.as_ref().map(|s| s.view).unwrap_or(0),
        like: data.stat.as_ref().map(|s| s.like).unwrap_or(0),
        pages,
        selected_page,
    }
}

/// 下载图片并转成 data url，避免 WebView 直连外网 / 混合内容问题。
async fn fetch_data_url(client: &reqwest::Client, url: &str) -> Option<String> {
    if url.is_empty() {
        return None;
    }
    // B 站图片给的是 http，换 https 更稳
    let url = if let Some(rest) = url.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        url.to_string()
    };

    let resp = client
        .get(&url)
        .header(reqwest::header::REFERER, REFERER)
        .timeout(Duration::from_secs(12))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();
    let bytes = resp.bytes().await.ok()?;
    if bytes.is_empty() || bytes.len() > 6 * 1024 * 1024 {
        return None;
    }
    let ctype = if ctype.starts_with("image/") {
        ctype
    } else {
        "image/jpeg".to_string()
    };
    Some(format!(
        "data:{};base64,{}",
        ctype,
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}

// ---------------------------------------------------------------------------
// 播放地址
// ---------------------------------------------------------------------------

#[derive(Deserialize, Clone)]
pub struct DashStream {
    #[serde(default)]
    pub id: u32,
    #[serde(rename = "baseUrl", default)]
    base_url: Option<String>,
    #[serde(rename = "base_url", default)]
    base_url_alt: Option<String>,
    #[serde(rename = "backupUrl", default)]
    backup_url: Option<Vec<String>>,
    #[serde(rename = "backup_url", default)]
    backup_url_alt: Option<Vec<String>>,
    #[serde(default)]
    pub bandwidth: u64,
    #[serde(rename = "mimeType", default)]
    #[allow(dead_code)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub codecs: Option<String>,
    #[serde(default)]
    pub codecid: u32,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(rename = "frameRate", default)]
    pub frame_rate: Option<String>,
}

impl DashStream {
    /// 主地址 + 备用 CDN 地址（去重）。
    pub fn urls(&self) -> Vec<String> {
        let mut v = Vec::new();
        if let Some(u) = self.base_url.as_ref().filter(|u| !u.is_empty()) {
            v.push(u.clone());
        }
        if let Some(u) = self.base_url_alt.as_ref().filter(|u| !u.is_empty()) {
            if !v.contains(u) {
                v.push(u.clone());
            }
        }
        for list in [self.backup_url.as_ref(), self.backup_url_alt.as_ref()]
            .into_iter()
            .flatten()
        {
            for u in list {
                if !u.is_empty() && !v.contains(u) {
                    v.push(u.clone());
                }
            }
        }
        v
    }
}

#[derive(Deserialize, Clone)]
struct DashAudioOnly {
    #[serde(default)]
    audio: Option<DashStream>,
}

#[derive(Deserialize, Clone)]
struct Dash {
    #[serde(default)]
    duration: u64,
    #[serde(default)]
    video: Vec<DashStream>,
    #[serde(default)]
    audio: Option<Vec<DashStream>>,
    #[serde(default)]
    dolby: Option<DashAudioOnly>,
    #[serde(default)]
    flac: Option<DashAudioOnly>,
}

#[derive(Deserialize, Clone)]
pub struct Durl {
    #[serde(default)]
    order: u32,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    url: String,
    #[serde(rename = "backup_url", default)]
    backup_url: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct PlayUrlData {
    #[serde(default)]
    quality: u32,
    #[serde(default)]
    accept_quality: Vec<u32>,
    #[serde(default)]
    accept_description: Vec<String>,
    #[serde(default)]
    timelength: u64,
    #[serde(default)]
    dash: Option<Dash>,
    #[serde(default)]
    durl: Option<Vec<Durl>>,
}

pub fn quality_label(id: u32) -> String {
    match id {
        127 => "8K 超高清".into(),
        126 => "杜比视界".into(),
        125 => "HDR 真彩".into(),
        120 => "4K 超清".into(),
        116 => "1080P 60帧".into(),
        112 => "1080P 高码率".into(),
        100 => "智能修复".into(),
        80 => "1080P 高清".into(),
        74 => "720P 60帧".into(),
        64 => "720P 高清".into(),
        32 => "480P 清晰".into(),
        16 => "360P 流畅".into(),
        6 => "240P 极速".into(),
        other => format!("画质 {other}"),
    }
}

pub fn codec_label(codecid: u32, codecs: Option<&str>) -> String {
    let base = match codecid {
        7 => "AVC / H.264",
        12 => "HEVC / H.265",
        13 => "AV1",
        _ => {
            if let Some(c) = codecs {
                if c.starts_with("hev") || c.starts_with("hvc") {
                    "HEVC / H.265"
                } else if c.starts_with("av01") {
                    "AV1"
                } else if c.starts_with("avc") {
                    "AVC / H.264"
                } else {
                    "未知编码"
                }
            } else {
                "未知编码"
            }
        }
    };
    base.to_string()
}

pub fn audio_label(id: u32) -> String {
    match id {
        30216 => "64K 普通".into(),
        30232 => "132K 标准".into(),
        30280 => "192K 高品质".into(),
        30250 => "杜比全景声".into(),
        30251 => "Hi-Res 无损".into(),
        30380 => "杜比音效".into(),
        other => format!("音轨 {other}"),
    }
}

#[derive(Serialize, Clone)]
pub struct VideoStream {
    pub codecid: u32,
    pub codec_label: String,
    pub width: u32,
    pub height: u32,
    pub bandwidth: u64,
    pub est_size: u64,
    pub frame_rate: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct QualityGroup {
    pub id: u32,
    pub label: String,
    pub streams: Vec<VideoStream>,
}

#[derive(Serialize, Clone)]
pub struct AudioStream {
    pub id: u32,
    pub label: String,
    pub bandwidth: u64,
    pub est_size: u64,
    pub kind: String,
}

#[derive(Serialize, Clone)]
pub struct PlayInfo {
    /// "dash" 或 "durl"
    pub mode: String,
    pub qualities: Vec<QualityGroup>,
    pub audios: Vec<AudioStream>,
    /// 当前账号实际能拿到的最高画质 id
    pub current_quality: u32,
    pub duration: u64,
    /// 无登录时最高只有 480P，用于前端提示
    pub logged_in: bool,
    /// durl 模式下的文件列表（顺序）大小之和
    pub durl_total_size: u64,
}

/// 请求播放地址。`mode` = "dash" | "durl"。
/// fnval: 16 = DASH；4048 = DASH+HDR+4K+杜比+8K+AV1；1 = 传统单文件 mp4
pub async fn fetch_play_url(
    client: &reqwest::Client,
    bvid: &str,
    cid: u64,
    mode: &str,
    qn: u32,
    sessdata: Option<&str>,
) -> Result<PlayUrlData> {
    let fnval = if mode == "durl" { 1 } else { 4048 };
    let mut url = format!(
        "https://api.bilibili.com/x/player/playurl?bvid={bvid}&cid={cid}&fnval={fnval}&fnver=0&fourk=1&platform=pc&high_quality=1"
    );
    if qn > 0 {
        url.push_str(&format!("&qn={qn}"));
    }

    let mut req = client.get(&url).header(reqwest::header::REFERER, REFERER);
    if let Some(cookie) = cookie_header(sessdata) {
        req = req.header(reqwest::header::COOKIE, cookie);
    }

    let resp = req.send().await.context("请求播放地址失败，请检查网络连接")?;
    let text = resp.text().await.context("读取播放地址失败")?;
    let env: ApiEnvelope<PlayUrlData> =
        serde_json::from_str(&text).with_context(|| format!("解析播放地址失败：{}", truncate(&text, 200)))?;

    if env.code != 0 {
        return Err(api_error(env.code, &env.message));
    }
    env.data.ok_or_else(|| anyhow!("接口未返回播放地址"))
}

/// 把接口原始数据整理成前端可用的结构。
pub fn build_play_info(data: &PlayUrlData, mode: &str, sessdata: Option<&str>) -> PlayInfo {
    let duration = if data.dash.as_ref().map(|d| d.duration).unwrap_or(0) > 0 {
        data.dash.as_ref().unwrap().duration
    } else {
        data.timelength / 1000
    };
    let dur = duration.max(1);

    let mut qualities: Vec<QualityGroup> = Vec::new();
    let mut audios: Vec<AudioStream> = Vec::new();
    let mut durl_total_size = 0u64;
    // 实际能拿到的最高画质：DASH 用流列表里的最大 id，durl 用接口返回的 quality
    let mut actual_quality = data.quality;

    if mode == "durl" {
        if let Some(list) = data.durl.as_ref() {
            for d in list {
                durl_total_size += d.size;
            }
        }
        // durl 模式的画质来自 accept_quality
        for (i, q) in data.accept_quality.iter().enumerate() {
            let label = data
                .accept_description
                .get(i)
                .cloned()
                .unwrap_or_else(|| quality_label(*q));
            qualities.push(QualityGroup {
                id: *q,
                label,
                streams: vec![VideoStream {
                    codecid: 7,
                    codec_label: "容器封装（无需合并）".into(),
                    width: 0,
                    height: 0,
                    bandwidth: if dur > 0 {
                        durl_total_size.saturating_mul(8) / dur
                    } else {
                        0
                    },
                    est_size: durl_total_size,
                    frame_rate: None,
                }],
            });
        }
    } else if let Some(dash) = data.dash.as_ref() {
        // 按画质 id 分组
        let mut map: Vec<(u32, Vec<VideoStream>)> = Vec::new();
        for v in &dash.video {
            let stream = VideoStream {
                codecid: v.codecid,
                codec_label: codec_label(v.codecid, v.codecs.as_deref()),
                width: v.width,
                height: v.height,
                bandwidth: v.bandwidth,
                est_size: v.bandwidth.saturating_mul(dur) / 8,
                frame_rate: v.frame_rate.clone(),
            };
            match map.iter_mut().find(|(id, _)| *id == v.id) {
                Some((_, list)) => list.push(stream),
                None => map.push((v.id, vec![stream])),
            }
        }
        // 码率高的画质在前
        map.sort_by(|a, b| b.0.cmp(&a.0));
        actual_quality = map.iter().map(|(id, _)| *id).max().unwrap_or(data.quality);
        for (id, streams) in map {
            // 同一画质内优先 AVC(H.264)，兼容性最好
            let mut streams = streams;
            streams.sort_by_key(|s| if s.codecid == 7 { 0 } else { s.codecid });
            qualities.push(QualityGroup {
                id,
                label: quality_label(id),
                streams,
            });
        }

        // 普通音轨
        if let Some(list) = dash.audio.as_ref() {
            for a in list {
                audios.push(AudioStream {
                    id: a.id,
                    label: audio_label(a.id),
                    bandwidth: a.bandwidth,
                    est_size: a.bandwidth.saturating_mul(dur) / 8,
                    kind: "normal".into(),
                });
            }
            audios.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));
        }
        if let Some(d) = dash.dolby.as_ref().and_then(|d| d.audio.as_ref()) {
            audios.insert(
                0,
                AudioStream {
                    id: 30250,
                    label: "杜比全景声".into(),
                    bandwidth: d.bandwidth,
                    est_size: d.bandwidth.saturating_mul(dur) / 8,
                    kind: "dolby".into(),
                },
            );
        }
        if let Some(f) = dash.flac.as_ref().and_then(|d| d.audio.as_ref()) {
            audios.insert(
                0,
                AudioStream {
                    id: 30251,
                    label: "Hi-Res 无损".into(),
                    bandwidth: f.bandwidth,
                    est_size: f.bandwidth.saturating_mul(dur) / 8,
                    kind: "flac".into(),
                },
            );
        }
    }

    PlayInfo {
        mode: mode.to_string(),
        qualities,
        audios,
        current_quality: actual_quality,
        duration,
        logged_in: sessdata.map(|s| !s.trim().is_empty()).unwrap_or(false),
        durl_total_size,
    }
}

/// 从 playurl 结果里挑出指定画质 + 编码的视频流。
pub fn pick_video_stream<'a>(
    data: &'a PlayUrlData,
    quality_id: u32,
    codecid: u32,
) -> Result<&'a DashStream> {
    let dash = data
        .dash
        .as_ref()
        .ok_or_else(|| anyhow!("该视频没有可用的 DASH 流"))?;
    if dash.video.is_empty() {
        bail!("没有解析到视频流");
    }

    // 1. 画质 + 编码都匹配
    if let Some(s) = dash
        .video
        .iter()
        .find(|v| v.id == quality_id && v.codecid == codecid)
    {
        return Ok(s);
    }
    // 2. 同画质下任意编码
    if let Some(s) = dash.video.iter().find(|v| v.id == quality_id) {
        return Ok(s);
    }
    // 3. 该画质不可用时，降级到实际可用的最高画质
    let fallback = dash.video.iter().map(|v| v.id).max().unwrap_or(0);
    dash.video
        .iter()
        .find(|v| v.id == fallback)
        .ok_or_else(|| anyhow!("没有找到可用的视频流"))
}

/// 挑选音轨。`audio_kind`: "auto" | "normal" | "flac" | "dolby"
pub fn pick_audio_stream<'a>(data: &'a PlayUrlData, audio_id: u32) -> Option<&'a DashStream> {
    let dash = data.dash.as_ref()?;

    match audio_id {
        30251 => {
            if let Some(s) = dash.flac.as_ref().and_then(|d| d.audio.as_ref()) {
                return Some(s);
            }
        }
        30250 => {
            if let Some(s) = dash.dolby.as_ref().and_then(|d| d.audio.as_ref()) {
                return Some(s);
            }
        }
        id if id > 0 => {
            if let Some(s) = dash.audio.as_ref().and_then(|l| l.iter().find(|a| a.id == id)) {
                return Some(s);
            }
        }
        _ => {}
    }

    // 自动：普通音轨里码率最高的
    dash.audio
        .as_ref()
        .and_then(|l| l.iter().max_by_key(|a| a.bandwidth))
}

/// 取 durl 列表（按 order 排序）。
pub fn pick_durl(data: &PlayUrlData) -> Result<Vec<Durl>> {
    let mut list = data
        .durl
        .clone()
        .ok_or_else(|| anyhow!("该视频没有可用的单文件地址，请改用高清(DASH)模式"))?;
    if list.is_empty() {
        bail!("没有解析到视频地址");
    }
    list.sort_by_key(|d| d.order);
    Ok(list)
}

pub fn durl_urls(d: &Durl) -> Vec<String> {
    let mut v = Vec::new();
    if !d.url.is_empty() {
        v.push(d.url.clone());
    }
    if let Some(list) = d.backup_url.as_ref() {
        for u in list {
            if !u.is_empty() && !v.contains(u) {
                v.push(u.clone());
            }
        }
    }
    v
}

// ---------------------------------------------------------------------------

pub fn cookie_header(sessdata: Option<&str>) -> Option<String> {
    let s = sessdata?.trim();
    if s.is_empty() {
        return None;
    }
    if s.to_ascii_lowercase().contains("sessdata=") {
        Some(s.to_string())
    } else {
        Some(format!("SESSDATA={s}"))
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}
