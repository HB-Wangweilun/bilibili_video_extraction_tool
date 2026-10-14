/** 与 Rust 端结构体一一对应的类型定义 */

export interface PageInfo {
  cid: number;
  page: number;
  part: string;
  duration: number;
}

export interface VideoInfo {
  bvid: string;
  aid: number;
  title: string;
  cover: string | null;
  owner_name: string;
  owner_face: string | null;
  duration: number;
  pubdate: number;
  view: number;
  like: number;
  pages: PageInfo[];
  selected_page: number;
}

export interface VideoStream {
  codecid: number;
  codec_label: string;
  width: number;
  height: number;
  bandwidth: number;
  est_size: number;
  frame_rate: string | null;
}

export interface QualityGroup {
  id: number;
  label: string;
  streams: VideoStream[];
}

export interface AudioStream {
  id: number;
  label: string;
  bandwidth: number;
  est_size: number;
  kind: string;
}

export interface PlayInfo {
  mode: string;
  qualities: QualityGroup[];
  audios: AudioStream[];
  current_quality: number;
  duration: number;
  logged_in: boolean;
  durl_total_size: number;
}

export interface DownloadRequest {
  bvid: string;
  cid: number;
  videoTitle: string;
  partTitle: string;
  pageIndex: number;
  totalPages: number;
  qualityId: number;
  codecid: number;
  audioId: number;
  mode: string;
  sessdata: string | null;
  outputDir: string;
  estVideoSize: number;
  estAudioSize: number;
  videoWidth: number;
  videoHeight: number;
  /** 要去除的水印区域（源像素坐标）；null 表示不去水印 */
  watermark: WmBox | null;
}

/** 水印区域，坐标为视频原始像素 */
export interface WmBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface WatermarkPreview {
  /** 预览画面（PNG data url） */
  frame: string | null;
  srcWidth: number;
  srcHeight: number;
  dispWidth: number;
  dispHeight: number;
  /** 自动检测到的水印区域 */
  detected: WmBox | null;
  /** 检测失败等提示 */
  note: string;
}

export interface DownloadResult {
  filePath: string;
  fileName: string;
  dir: string;
  size: number;
  sizeText: string;
  elapsed: number;
  qualityText: string;
}

export interface DownloadProgress {
  stage: string;
  phase: string;
  downloaded: number;
  total: number;
  percent: number;
  speed: number;
  message: string;
}

export interface FfmpegStatus {
  found: boolean;
  path: string | null;
  source: string | null;
  version: string | null;
}

export interface InstallProgress {
  downloaded: number;
  total: number;
  percent: number;
  message: string;
}
