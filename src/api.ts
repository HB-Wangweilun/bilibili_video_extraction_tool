import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  DownloadProgress,
  DownloadResult,
  FfmpegStatus,
  InstallProgress,
  PlayInfo,
  VideoInfo,
} from "./types";

/** 解析链接，拿到视频基础信息 */
export function parseVideo(input: string, sessdata: string): Promise<VideoInfo> {
  return invoke<VideoInfo>("parse_video", {
    input,
    sessdata: sessdata || null,
  });
}

/** 获取指定分 P 的清晰度 / 音轨列表 */
export function getPlayInfo(
  bvid: string,
  cid: number,
  mode: "dash" | "durl",
  sessdata: string
): Promise<PlayInfo> {
  return invoke<PlayInfo>("get_play_info", {
    bvid,
    cid,
    mode,
    sessdata: sessdata || null,
  });
}

/** 开始下载 */
export function startDownload(
  req: import("./types").DownloadRequest
): Promise<DownloadResult> {
  return invoke<DownloadResult>("start_download", { req });
}

export function cancelDownload(): Promise<void> {
  return invoke("cancel_download");
}

/** 抓取一帧画面并自动定位水印位置 */
export function detectWatermark(
  bvid: string,
  cid: number,
  qualityId: number,
  codecid: number,
  sessdata: string
): Promise<import("./types").WatermarkPreview> {
  return invoke<import("./types").WatermarkPreview>("detect_watermark", {
    bvid,
    cid,
    qualityId,
    codecid,
    sessdata: sessdata || null,
  });
}

export function checkFfmpeg(): Promise<FfmpegStatus> {
  return invoke<FfmpegStatus>("check_ffmpeg");
}

export function installFfmpeg(): Promise<string> {
  return invoke<string>("install_ffmpeg");
}

export function getDefaultOutputDir(): Promise<string> {
  return invoke<string>("get_default_output_dir");
}

export function openInFolder(path: string): Promise<void> {
  return invoke("open_in_folder", { path });
}

export function playFile(path: string): Promise<void> {
  return invoke("play_file", { path });
}

export function onDownloadProgress(
  cb: (p: DownloadProgress) => void
): Promise<UnlistenFn> {
  return listen<DownloadProgress>("download-progress", (e) => cb(e.payload));
}

export function onFfmpegProgress(
  cb: (p: InstallProgress) => void
): Promise<UnlistenFn> {
  return listen<InstallProgress>("ffmpeg-progress", (e) => cb(e.payload));
}
