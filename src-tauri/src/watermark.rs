//! 静态水印位置检测。
//!
//! 原理：B 站水印是半透明的浅色叠加层，**位置固定**，而视频内容在动。
//! 于是「在绝大多数采样帧里都是浅色且低饱和」的像素，聚成的紧凑区块就是水印。
//!
//! 只做纯字节运算，不依赖任何图像解码库。

use serde::{Deserialize, Serialize};

/// 水印区域（像素坐标）。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct WmBox {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// 判定为"浅色"的阈值：三通道最小值 > 165 且最大最小差 < 46
const MIN_LIGHT: u8 = 165;
const MAX_CHROMA: u8 = 46;
/// 至少要在这么多比例的采样帧里都是浅色，才算"持续浅色"
const PERSIST_RATIO: f64 = 0.75;
/// 一个角落至少要有这么多持续浅色像素才认为检测到了水印
const MIN_PIXELS: usize = 24;
/// 水印笔画是文字/图形，密度不会太低也不会接近实心
const MIN_DENSITY: f64 = 0.04;
const MAX_DENSITY: f64 = 0.45;
/// 只看画面上部多大比例（B 站水印固定在顶部）
const TOP_BAND: f64 = 0.42;

/// 在 raw RGB24 帧序列中检测水印。
///
/// * `frames` — 连续的 `n` 帧，每帧 `w*h*3` 字节
/// * 返回 **分析坐标** 下的外接框（调用方负责按源分辨率缩放）
pub fn detect(frames: &[u8], n: usize, w: u32, h: u32) -> Option<WmBox> {
    let (w, h) = (w as usize, h as usize);
    let frame_len = w * h * 3;
    if n < 3 || frames.len() < frame_len * n {
        return None;
    }

    // 1. 逐像素统计"浅色"出现的帧数
    let mut hits = vec![0u16; w * h];
    for f in 0..n {
        let base = f * frame_len;
        let slice = &frames[base..base + frame_len];
        for i in 0..w * h {
            let r = slice[i * 3];
            let g = slice[i * 3 + 1];
            let b = slice[i * 3 + 2];
            let mx = r.max(g).max(b);
            let mn = r.min(g).min(b);
            if mn > MIN_LIGHT && (mx - mn) < MAX_CHROMA {
                hits[i] += 1;
            }
        }
    }

    let need = ((n as f64) * PERSIST_RATIO).ceil() as u16;
    let persist = |x: usize, y: usize| hits[y * w + x] >= need;

    // 2. 只看画面上部的左右两个角。
    //
    // B 站水印固定在画面顶部（左上/右上）；画面下部常见字幕、台标、贴片等
    // 同样"持续静止"的元素，若一起参与比较会把它们误判成水印。
    let band = ((h as f64) * TOP_BAND) as usize;
    let regions: [(&str, usize, usize, usize, usize); 2] = [
        ("左上", 0, w / 2, 0, band),
        ("右上", w / 2, w, 0, band),
    ];

    let mut best: Option<(usize, WmBox)> = None;
    for (name, x0, x1, y0, y1) in regions {
        let mut xs: Vec<u32> = Vec::new();
        let mut ys: Vec<u32> = Vec::new();
        for y in y0..y1.min(h) {
            for x in x0..x1.min(w) {
                if persist(x, y) {
                    xs.push(x as u32);
                    ys.push(y as u32);
                }
            }
        }
        if xs.len() < MIN_PIXELS {
            continue;
        }
        // 用分位数裁掉零散噪点，取更稳的外接框
        xs.sort_unstable();
        ys.sort_unstable();
        let lo = (xs.len() as f64 * 0.01) as usize;
        let hi = ((xs.len() as f64 * 0.99) as usize).min(xs.len() - 1);
        let bx0 = xs[lo];
        let bx1 = xs[hi];
        let by0 = ys[lo];
        let by1 = ys[hi];
        if bx1 <= bx0 || by1 <= by0 {
            continue;
        }
        let bw = bx1 - bx0 + 1;
        let bh = by1 - by0 + 1;

        // 水印是一条不会太窄的横带
        if (bw as usize) < (w / 25).max(6) {
            continue;
        }
        // 密度过滤：文字/图形水印是"镂空"的，实心亮块（字幕底板、纯白贴片）会被排除
        let density = xs.len() as f64 / (bw * bh) as f64;
        if !(MIN_DENSITY..=MAX_DENSITY).contains(&density) {
            println!(
                "[watermark] 跳过 {name}：x={bx0} y={by0} w={bw} h={bh} \
                 像素 {} 密度 {:.1}%（不像水印）",
                xs.len(),
                density * 100.0
            );
            continue;
        }

        let candidate = WmBox {
            x: bx0,
            y: by0,
            w: bw,
            h: bh,
        };
        println!(
            "[watermark] 候选 {name}: x={bx0} y={by0} w={bw} h={bh} \
             像素 {} 密度 {:.1}%",
            xs.len(),
            density * 100.0
        );
        if best.as_ref().map(|(c, _)| xs.len() > *c).unwrap_or(true) {
            best = Some((xs.len(), candidate));
        }
    }

    best.map(|(_, b)| b)
}

/// 把分析坐标下的框换算到源分辨率，并向外扩一点余量（水印边缘有抗锯齿/羽化）。
pub fn scale_and_pad(b: &WmBox, analysis_w: u32, src_w: u32, src_h: u32) -> WmBox {
    let k = src_w as f64 / analysis_w as f64;
    let pad_x = (b.x as f64 * k).floor() as i64 - (6.0 * k).round() as i64;
    let pad_y = (b.y as f64 * k).floor() as i64 - (5.0 * k).round() as i64;
    let x = pad_x.max(1);
    let y = pad_y.max(1);
    let right = ((b.x + b.w) as f64 * k).ceil() as i64 + (6.0 * k).round() as i64;
    let bottom = ((b.y + b.h) as f64 * k).ceil() as i64 + (5.0 * k).round() as i64;
    let x = x.min(src_w as i64 - 3).max(1);
    let y = y.min(src_h as i64 - 3).max(1);
    let w = (right - x).clamp(4, src_w as i64 - 1 - x).max(4);
    let h = (bottom - y).clamp(4, src_h as i64 - 1 - y).max(4);
    WmBox {
        x: x as u32,
        y: y as u32,
        w: w as u32,
        h: h as u32,
    }
}
