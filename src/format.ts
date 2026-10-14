export function humanSize(bytes: number): string {
  if (!bytes || bytes <= 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return i === 0 ? `${bytes} B` : `${v.toFixed(2)} ${units[i]}`;
}

export function humanSpeed(bps: number): string {
  if (!bps || bps <= 0) return "";
  return `${humanSize(bps)}/s`;
}

export function humanDuration(sec: number): string {
  if (!sec || sec <= 0) return "00:00";
  const s = Math.floor(sec % 60);
  const m = Math.floor((sec / 60) % 60);
  const h = Math.floor(sec / 3600);
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

export function humanCount(n: number): string {
  if (!n) return "0";
  if (n >= 100000000) return `${(n / 100000000).toFixed(1)} 亿`;
  if (n >= 10000) return `${(n / 10000).toFixed(1)} 万`;
  return String(n);
}

export function humanDate(ts: number): string {
  if (!ts) return "";
  const d = new Date(ts * 1000);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(
    d.getDate()
  ).padStart(2, "0")}`;
}

/** 清洗文件名，和后端保持一致的规则（用于前端预览） */
export function sanitizeFilename(name: string, fallback = "bilibili_video"): string {
  let s = name.replace(/[\\/:*?"<>|\u0000-\u001f]/g, "_");
  s = s.split(/\s+/).filter(Boolean).join(" ");
  s = s.replace(/^[.\s]+|[.\s]+$/g, "");
  if (s.length > 110) s = s.slice(0, 110);
  return s || fallback;
}

export function buildFileName(
  videoTitle: string,
  partTitle: string,
  pageIndex: number,
  totalPages: number,
  bvid: string
): string {
  const base =
    totalPages > 1
      ? `${videoTitle}_P${pageIndex}_${partTitle}`
      : videoTitle || bvid;
  return sanitizeFilename(base, `bilibili_${bvid}`) + ".mp4";
}
