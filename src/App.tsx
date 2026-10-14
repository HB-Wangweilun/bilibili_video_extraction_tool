import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import appIcon from "./assets/app-icon.png";
import {
  cancelDownload,
  checkFfmpeg,
  detectWatermark,
  getDefaultOutputDir,
  getPlayInfo,
  installFfmpeg,
  onDownloadProgress,
  onFfmpegProgress,
  openInFolder,
  parseVideo,
  playFile,
  startDownload,
} from "./api";
import {
  buildFileName,
  humanCount,
  humanDate,
  humanDuration,
  humanSize,
  humanSpeed,
} from "./format";
import type {
  AudioStream,
  DownloadProgress,
  DownloadResult,
  FfmpegStatus,
  InstallProgress,
  PlayInfo,
  QualityGroup,
  VideoInfo,
  WatermarkPreview,
  WmBox,
} from "./types";

const SAMPLE_URL =
  "https://www.bilibili.com/video/BV1gjpR68Ejh/?vd_source=c253793c656a4fb9e58041579b11ce22";

/* ------------------------------------------------------------------ 图标 */

const IconPlay = () => (
  <svg viewBox="0 0 24 24" fill="currentColor">
    <path d="M8 5.5v13l11-6.5-11-6.5z" />
  </svg>
);

const IconSearch = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2">
    <circle cx="11" cy="11" r="7" />
    <path d="M20 20l-3.5-3.5" strokeLinecap="round" />
  </svg>
);

const IconDownload = () => (
  <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2">
    <path d="M12 3v12" strokeLinecap="round" />
    <path d="M7.5 10.5L12 15l4.5-4.5" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M4 18v1.5A1.5 1.5 0 005.5 21h13a1.5 1.5 0 001.5-1.5V18" strokeLinecap="round" />
  </svg>
);

const IconFolder = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
    <path d="M3 7.5A1.5 1.5 0 014.5 6h4l2 2.5h9A1.5 1.5 0 0121 10v8a1.5 1.5 0 01-1.5 1.5h-15A1.5 1.5 0 013 18V7.5z" />
  </svg>
);

const IconGear = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9">
    <circle cx="12" cy="12" r="3.2" />
    <path d="M12 2.8l1.4 2.2 2.6-.5.5 2.6 2.2 1.4-1.3 2.3 1.3 2.3-2.2 1.4-.5 2.6-2.6-.5L12 21.2l-1.4-2.2-2.6.5-.5-2.6-2.2-1.4L6.6 13 5.3 10.7l2.2-1.4.5-2.6 2.6.5z" />
  </svg>
);

const IconCheck = () => (
  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4">
    <path d="M5 13l4.5 4.5L19 7" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);

const IconWarn = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
    <path d="M12 8.5v5" strokeLinecap="round" />
    <circle cx="12" cy="16.6" r="1" fill="currentColor" stroke="none" />
    <path d="M10.3 3.9L2.6 17.4A1.9 1.9 0 004.3 20.3h15.4a1.9 1.9 0 001.7-2.9L13.7 3.9a1.9 1.9 0 00-3.4 0z" />
  </svg>
);

const IconInfo = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
    <circle cx="12" cy="12" r="9" />
    <path d="M12 11v5.2" strokeLinecap="round" />
    <circle cx="12" cy="7.9" r="1" fill="currentColor" stroke="none" />
  </svg>
);

const IconX = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2">
    <path d="M6 6l12 12M18 6L6 18" strokeLinecap="round" />
  </svg>
);

const IconFilm = () => (
  <svg width="46" height="46" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.3">
    <rect x="2.5" y="5" width="19" height="14" rx="2.2" />
    <path d="M7.2 5v14M16.8 5v14M2.5 12h19" />
  </svg>
);

/* ------------------------------------------------------------------ 主组件 */

export default function App() {
  const [input, setInput] = useState("");
  const [parsing, setParsing] = useState(false);
  const [info, setInfo] = useState<VideoInfo | null>(null);
  const [pageIdx, setPageIdx] = useState(1);

  const [mode, setMode] = useState<"dash" | "durl">("dash");
  const [playInfo, setPlayInfo] = useState<PlayInfo | null>(null);
  const [loadingPlay, setLoadingPlay] = useState(false);
  const [qualityId, setQualityId] = useState(0);
  const [codecid, setCodecid] = useState(7);
  const [audioId, setAudioId] = useState(0);

  const [outputDir, setOutputDir] = useState("");
  const [ffmpeg, setFfmpeg] = useState<FfmpegStatus | null>(null);
  const [showSettings, setShowSettings] = useState(false);

  const [sessdata, setSessdata] = useState(
    () => localStorage.getItem("bili_sessdata") || ""
  );
  const [sessdataActive, setSessdataActive] = useState(sessdata);

  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [result, setResult] = useState<DownloadResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [installing, setInstalling] = useState(false);
  const [installProg, setInstallProg] = useState<InstallProgress | null>(null);

  // 去水印
  const [wmEnabled, setWmEnabled] = useState(false);
  const [wmLoading, setWmLoading] = useState(false);
  const [wmPreview, setWmPreview] = useState<WatermarkPreview | null>(null);
  const [wmBox, setWmBox] = useState<WmBox | null>(null);
  const [wmKey, setWmKey] = useState("");
  const [showWmPicker, setShowWmPicker] = useState(false);

  const resultRef = useRef<HTMLDivElement>(null);
  const ffmpegOk = !!ffmpeg?.found;

  /* ---------------------------------------------------------- 初始化 */

  useEffect(() => {
    getDefaultOutputDir()
      .then((d) => setOutputDir((cur) => cur || d))
      .catch(() => void 0);
    checkFfmpeg().then(setFfmpeg).catch(() => void 0);
  }, []);

  useEffect(() => {
    const p = onDownloadProgress((payload) => setProgress(payload));
    const q = onFfmpegProgress((payload) => setInstallProg(payload));
    return () => {
      p.then((un) => un());
      q.then((un) => un());
    };
  }, []);

  /* ---------------------------------------------------------- 加载播放信息 */

  useEffect(() => {
    if (!info) {
      setPlayInfo(null);
      return;
    }
    const page = info.pages[pageIdx - 1];
    if (!page) return;

    let dead = false;
    setLoadingPlay(true);
    getPlayInfo(info.bvid, page.cid, mode, sessdataActive)
      .then((p) => {
        if (dead) return;
        setPlayInfo(p);
        const best = p.qualities[0];
        if (best) {
          setQualityId(best.id);
          const avc =
            best.streams.find((s) => s.codecid === 7) ?? best.streams[0];
          setCodecid(avc.codecid);
        } else {
          setQualityId(0);
        }
        setAudioId(p.audios[0]?.id ?? 0);
      })
      .catch((e) => {
        if (!dead) setError(cleanError(String(e)));
      })
      .finally(() => {
        if (!dead) setLoadingPlay(false);
      });

    return () => {
      dead = true;
    };
  }, [info, pageIdx, mode, sessdataActive]);

  /* ---------------------------------------------------------- 解析 */

  const doParse = useCallback(
    async (raw?: string) => {
      const text = (raw ?? input).trim();
      if (!text) {
        setError("请先粘贴 B 站视频链接");
        return;
      }
      setParsing(true);
      setError(null);
      setResult(null);
      setProgress(null);
      try {
        const v = await parseVideo(text, sessdataActive);
        setInfo(v);
        setPageIdx(v.selected_page || 1);
        setInput(text);
      } catch (e) {
        setInfo(null);
        setPlayInfo(null);
        setError(cleanError(String(e)));
      } finally {
        setParsing(false);
      }
    },
    [input, sessdataActive]
  );

  const handlePaste = useCallback(
    (e: React.ClipboardEvent<HTMLInputElement>) => {
      const text = e.clipboardData.getData("text");
      if (text && /BV[0-9A-Za-z]{10}|b23\.tv|av\d+/i.test(text)) {
        e.preventDefault();
        setInput(text);
        void doParse(text);
      }
    },
    [doParse]
  );

  /* ---------------------------------------------------------- 下载 */

  const currentPage = info?.pages[pageIdx - 1] ?? null;

  const estSizes = useMemo(() => {
    const q = playInfo?.qualities.find((g) => g.id === qualityId);
    const v =
      q?.streams.find((s) => s.codecid === codecid)?.est_size ??
      q?.streams[0]?.est_size ??
      0;
    const a = playInfo?.audios.find((x) => x.id === audioId)?.est_size ?? 0;
    return { video: v, audio: a };
  }, [playInfo, qualityId, codecid, audioId]);

  /** 当前选中视频流的原始像素尺寸（去水印区域的坐标系） */
  const srcSize = useMemo(() => {
    const q = playInfo?.qualities.find((g) => g.id === qualityId);
    const s = q?.streams.find((x) => x.codecid === codecid) ?? q?.streams[0];
    return { w: s?.width ?? 0, h: s?.height ?? 0 };
  }, [playInfo, qualityId, codecid]);

  /* ------------------------------------------------- 去水印：自动定位 */

  // 水印区域和分辨率强相关，切清晰度 / 换分P 后必须重新定位
  const wmSignature =
    info && currentPage
      ? `${info.bvid}-${currentPage.cid}-${qualityId}-${codecid}`
      : "";

  useEffect(() => {
    if (!wmEnabled || !info || !currentPage || !wmSignature) return;
    if (wmSignature === wmKey) return;

    let dead = false;
    setWmLoading(true);
    setWmBox(null);
    detectWatermark(
      info.bvid,
      currentPage.cid,
      qualityId,
      codecid,
      sessdataActive
    )
      .then((p) => {
        if (dead) return;
        setWmPreview(p);
        setWmKey(wmSignature);
        if (p.detected) {
          setWmBox(p.detected);
        } else {
          setWmBox(defaultWmBox(p.srcWidth, p.srcHeight));
          setShowWmPicker(true);
        }
      })
      .catch((e) => {
        if (!dead) setError(cleanError(String(e)));
      })
      .finally(() => {
        if (!dead) setWmLoading(false);
      });

    return () => {
      dead = true;
    };
  }, [wmEnabled, info, currentPage, wmSignature, wmKey, qualityId, codecid, sessdataActive]);

  const canPickRegion = wmEnabled && !!currentPage && !loadingPlay && !downloading;

  const redetect = useCallback(() => {
    setWmPreview(null);
    setWmBox(null);
    setWmKey("");
    setShowWmPicker(false);
  }, []);

  const fileName = useMemo(() => {
    if (!info || !currentPage) return "";
    return buildFileName(
      info.title,
      currentPage.part,
      currentPage.page,
      info.pages.length,
      info.bvid
    );
  }, [info, currentPage]);

  const doDownload = useCallback(async () => {
    if (!info || !currentPage || !outputDir) {
      if (!outputDir) setError("请先选择保存位置");
      return;
    }
    setError(null);
    setResult(null);
    setDownloading(true);
    setProgress({
      stage: "preparing",
      phase: "",
      downloaded: 0,
      total: 0,
      percent: 0,
      speed: 0,
      message: "正在准备…",
    });
    try {
      const res = await startDownload({
        bvid: info.bvid,
        cid: currentPage.cid,
        videoTitle: info.title,
        partTitle: currentPage.part,
        pageIndex: currentPage.page,
        totalPages: info.pages.length,
        qualityId,
        codecid,
        audioId,
        mode,
        sessdata: sessdataActive || null,
        outputDir,
        estVideoSize: estSizes.video,
        estAudioSize: estSizes.audio,
        videoWidth: srcSize.w,
        videoHeight: srcSize.h,
        watermark: wmEnabled ? wmBox : null,
      });
      setResult(res);
      setTimeout(
        () => resultRef.current?.scrollIntoView({ behavior: "smooth", block: "nearest" }),
        60
      );
    } catch (e) {
      setError(cleanError(String(e)));
    } finally {
      setDownloading(false);
    }
  }, [
    info,
    currentPage,
    outputDir,
    qualityId,
    codecid,
    audioId,
    mode,
    sessdataActive,
    estSizes,
    srcSize,
    wmEnabled,
    wmBox,
  ]);

  const doCancel = useCallback(() => {
    cancelDownload().catch(() => void 0);
  }, []);

  /* ---------------------------------------------------------- 保存位置 */

  const pickDir = useCallback(async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        directory: true,
        multiple: false,
        title: "选择视频保存位置",
        defaultPath: outputDir || undefined,
      });
      if (typeof picked === "string") setOutputDir(picked);
    } catch (e) {
      setError(String(e));
    }
  }, [outputDir]);

  /* ---------------------------------------------------------- ffmpeg */

  const doInstallFfmpeg = useCallback(async () => {
    setInstalling(true);
    setError(null);
    try {
      await installFfmpeg();
      const st = await checkFfmpeg();
      setFfmpeg(st);
    } catch (e) {
      setError(cleanError(String(e)));
    } finally {
      setInstalling(false);
      setInstallProg(null);
    }
  }, []);

  const saveSessdata = useCallback(() => {
    localStorage.setItem("bili_sessdata", sessdata.trim());
    setSessdataActive(sessdata.trim());
    setError(null);
  }, [sessdata]);

  /* ---------------------------------------------------------- 渲染 */

  const qualityGroups: QualityGroup[] = playInfo?.qualities ?? [];
  const audios: AudioStream[] = playInfo?.audios ?? [];
  const activeGroup = qualityGroups.find((g) => g.id === qualityId);
  const showResult = !!result;
  const showProgress = downloading || (progress && progress.stage !== "preparing") || showResult;

  return (
    <div className="shell">
      {/* 顶栏 */}
      <header className="topbar">
        <div className="brand">
          <div className="brand-logo">
            <img src={appIcon} alt="B站视频解析工具" />
          </div>
          <div>
            <div className="brand-name">B站视频解析工具</div>
            <div className="brand-sub">粘贴链接 · 导出 MP4</div>
          </div>
        </div>
        <div className="spacer" />
        {ffmpeg !== null &&
          (ffmpegOk ? (
            <span className="chip chip-ok" title={ffmpeg.path ?? ""}>
              <span className="dot" />
              ffmpeg 就绪
            </span>
          ) : (
            <button
              className="chip chip-warn clickable"
              onClick={() => setShowSettings(true)}
            >
              <span className="dot" />
              ffmpeg 未安装
            </button>
          ))}
        <button className="btn btn-sm btn-ghost" onClick={() => setShowSettings(true)}>
          <IconGear />
          设置
        </button>
      </header>

      <main className="content">
        {/* 1. 链接输入 */}
        <section className="card">
          <div className="card-title">
            <span className="idx">1</span>
            粘贴 B 站视频链接
            <span className="hint">
              支持完整链接 / b23.tv 短链 / BV号 / av号 / 分享文案
            </span>
          </div>
          <div className="input-row">
            <input
              className="text-input"
              placeholder="https://www.bilibili.com/video/BV..."
              value={input}
              spellCheck={false}
              disabled={parsing || downloading}
              onChange={(e) => setInput(e.target.value)}
              onPaste={handlePaste}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !parsing && !downloading) void doParse();
              }}
            />
            <button
              className="btn btn-primary"
              disabled={parsing || downloading || !input.trim()}
              onClick={() => void doParse()}
            >
              {parsing ? <span className="spin" /> : <IconSearch />}
              {parsing ? "解析中" : "解析"}
            </button>
            {!input && (
              <button
                className="btn"
                disabled={parsing || downloading}
                onClick={() => {
                  setInput(SAMPLE_URL);
                  void doParse(SAMPLE_URL);
                }}
              >
                用示例
              </button>
            )}
          </div>
        </section>

        {error && (
          <div className="alert alert-error fade-in">
            <IconWarn />
            <div>{error}</div>
          </div>
        )}

        {/* 2. 视频信息 */}
        {info && (
          <section className="card fade-in">
            <div className="card-title">
              <span className="idx">2</span>
              视频信息
              <span className="hint">
                {info.pages.length > 1
                  ? `共 ${info.pages.length} 个分 P，点击选择`
                  : "单 P 视频"}
              </span>
            </div>
            <div className="video-card">
              {info.cover ? (
                <img className="cover" src={info.cover} alt="封面" />
              ) : (
                <div className="cover" />
              )}
              <div className="video-meta">
                <div className="video-title">{info.title}</div>
                <div className="meta-row">
                  {info.owner_face && (
                    <img className="avatar" src={info.owner_face} alt="" />
                  )}
                  <span>{info.owner_name}</span>
                  <span className="meta-sep">·</span>
                  <span>{humanDate(info.pubdate)}</span>
                  <span className="meta-sep">·</span>
                  <span>{humanDuration(info.duration)}</span>
                </div>
                <div className="meta-row">
                  <span className="bvid-tag">{info.bvid}</span>
                  <span className="meta-sep">·</span>
                  <span>{humanCount(info.view)} 播放</span>
                  <span className="meta-sep">·</span>
                  <span>{humanCount(info.like)} 点赞</span>
                </div>

                {info.pages.length > 1 && (
                  <div className="pages-wrap">
                    {info.pages.map((p, i) => (
                      <button
                        key={p.cid}
                        className={`page-chip${i + 1 === pageIdx ? " active" : ""}`}
                        title={p.part}
                        disabled={downloading}
                        onClick={() => setPageIdx(i + 1)}
                      >
                        P{p.page} {p.part}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            </div>
          </section>
        )}

        {/* 3. 下载设置 */}
        {info && (
          <section className="card fade-in">
            <div className="card-title">
              <span className="idx">3</span>
              下载设置
              {loadingPlay && <span className="spin" />}
            </div>

            <div className="field">
              <div className="field-label">
                下载方式
                <span className="tip">高清模式画质更高，但需要 ffmpeg 合并音视频</span>
              </div>
              <div className="seg">
                <button
                  className={`seg-item${mode === "dash" ? " active" : ""}`}
                  disabled={downloading}
                  onClick={() => setMode("dash")}
                >
                  高清模式 · 音视频合并
                </button>
                <button
                  className={`seg-item${mode === "durl" ? " active" : ""}`}
                  disabled={downloading}
                  onClick={() => setMode("durl")}
                >
                  兼容模式 · 单文件 MP4
                </button>
              </div>
              {mode === "dash" && !ffmpegOk && ffmpeg !== null && (
                <div className="help" style={{ color: "var(--warn)" }}>
                  还没检测到 ffmpeg，高清模式无法合并音视频。请到「设置」一键安装，
                  或先切到「兼容模式」。
                </div>
              )}
              {mode === "durl" && (
                <div className="help">
                  兼容模式由 B 站直接下发单个 .mp4，无需 ffmpeg；未登录时通常最高 720P。
                </div>
              )}
            </div>

            <div className="field">
              <div className="field-label">
                清晰度
                {playInfo && (
                  <span className="tip">
                    {playInfo.logged_in
                      ? `当前账号最高可获取 ${highestLabel(qualityGroups)}`
                      : "未登录，最高通常只有 480P；填 Cookie 可解锁 1080P/4K"}
                  </span>
                )}
              </div>
              <div className="opt-grid">
                {qualityGroups.length === 0 && (
                  <div className="help">
                    {loadingPlay ? "正在获取可用清晰度…" : "没有拿到可用清晰度"}
                  </div>
                )}
                {qualityGroups.map((g) => {
                  const s = g.streams.find((x) => x.codecid === codecid) ?? g.streams[0];
                  return (
                    <button
                      key={g.id}
                      className={`opt${g.id === qualityId ? " active" : ""}`}
                      disabled={downloading}
                      onClick={() => {
                        setQualityId(g.id);
                        if (!g.streams.some((x) => x.codecid === codecid)) {
                          const avc =
                            g.streams.find((x) => x.codecid === 7) ?? g.streams[0];
                          setCodecid(avc.codecid);
                        }
                      }}
                    >
                      <span className="opt-label">{g.label}</span>
                      <span className="opt-sub">
                        {mode === "durl"
                          ? humanSize(s?.est_size ?? 0)
                          : `${s?.width || "?"}×${s?.height || "?"} · ${humanSize(
                              s?.est_size ?? 0
                            )}`}
                      </span>
                    </button>
                  );
                })}
              </div>
            </div>

            {mode === "dash" && activeGroup && activeGroup.streams.length > 1 && (
              <div className="field">
                <div className="field-label">
                  视频编码
                  <span className="tip">AVC 兼容性最好，HEVC/AV1 体积更小</span>
                </div>
                <div className="opt-grid">
                  {activeGroup.streams.map((s) => (
                    <button
                      key={s.codecid}
                      className={`opt${s.codecid === codecid ? " active" : ""}`}
                      disabled={downloading}
                      onClick={() => setCodecid(s.codecid)}
                    >
                      <span className="opt-label">{s.codec_label}</span>
                      <span className="opt-sub">
                        {s.frame_rate ? `${s.frame_rate}fps · ` : ""}
                        {humanSize(s.est_size)}
                      </span>
                    </button>
                  ))}
                </div>
              </div>
            )}

            {mode === "dash" && audios.length > 0 && (
              <div className="field">
                <div className="field-label">音轨</div>
                <div className="opt-grid">
                  {audios.map((a) => (
                    <button
                      key={a.id}
                      className={`opt${a.id === audioId ? " active" : ""}`}
                      disabled={downloading}
                      onClick={() => setAudioId(a.id)}
                    >
                      <span className="opt-label">{a.label}</span>
                      <span className="opt-sub">{humanSize(a.est_size)}</span>
                    </button>
                  ))}
                </div>
              </div>
            )}

            {mode === "dash" && (
              <div className="field">
                <div className="field-label">
                  去水印
                  <span className="tip">
                    抹掉画面里烧录的 B 站水印（需重新编码，耗时较长）
                  </span>
                </div>

                <div className="wm-row">
                  <label className="switch">
                    <input
                      type="checkbox"
                      checked={wmEnabled}
                      disabled={downloading}
                      onChange={(e) => setWmEnabled(e.target.checked)}
                    />
                    <span className="slider" />
                    <span className="switch-text">
                      {wmEnabled ? "已开启" : "已关闭"}
                    </span>
                  </label>

                  {wmEnabled && wmLoading && (
                    <span className="chip">
                      <span className="spin" />
                      正在分析画面并定位水印…
                    </span>
                  )}

                  {wmEnabled && !wmLoading && wmBox && (
                    <button
                      className="btn btn-sm"
                      disabled={downloading}
                      onClick={() => setShowWmPicker(true)}
                    >
                      预览 / 调整区域
                    </button>
                  )}

                  {wmEnabled && !wmLoading && (
                    <button
                      className="btn btn-sm"
                      disabled={!canPickRegion || !wmBox}
                      onClick={redetect}
                    >
                      重新检测
                    </button>
                  )}
                </div>

                {wmEnabled && wmPreview?.frame && wmBox && (
                  <div className="wm-thumb">
                    <img src={wmPreview.frame} alt="画面预览" />
                    <div
                      className="wm-thumb-box"
                      style={{
                        left: `${(wmBox.x / Math.max(wmPreview.srcWidth, 1)) * 100}%`,
                        top: `${(wmBox.y / Math.max(wmPreview.srcHeight, 1)) * 100}%`,
                        width: `${(wmBox.w / Math.max(wmPreview.srcWidth, 1)) * 100}%`,
                        height: `${(wmBox.h / Math.max(wmPreview.srcHeight, 1)) * 100}%`,
                      }}
                    />
                  </div>
                )}

                {wmEnabled && !wmLoading && !wmBox && (
                  <div className="help" style={{ color: "var(--warn)" }}>
                    {wmPreview?.note || "还没定位到水印区域，点「重新检测」试试。"}
                  </div>
                )}

                {wmEnabled && (
                  <div className="help">
                    水印是烧进画面的像素，无法无损还原。这里用 ffmpeg 的 delogo
                    做邻域插值修复，被抹掉的区域会变糊，所以框选范围尽量贴合水印本身、别画太大。
                  </div>
                )}
              </div>
            )}

            <div className="field">
              <div className="field-label">
                保存位置
                {fileName && <span className="tip">文件名：{fileName}</span>}
              </div>
              <div className="input-row">
                <input
                  className="text-input path-input"
                  value={outputDir}
                  spellCheck={false}
                  disabled={downloading}
                  placeholder="选择或直接输入保存目录"
                  onChange={(e) => setOutputDir(e.target.value)}
                />
                <button className="btn" disabled={downloading} onClick={pickDir}>
                  <IconFolder />
                  浏览
                </button>
              </div>
            </div>

            <div className="input-row" style={{ marginTop: 18 }}>
              {downloading ? (
                <button className="btn btn-danger btn-lg" onClick={doCancel}>
                  <IconX />
                  取消下载
                </button>
              ) : (
                <button
                  className="btn btn-primary btn-lg"
                  disabled={
                    !currentPage ||
                    loadingPlay ||
                    !qualityGroups.length ||
                    (wmEnabled && (!wmBox || wmLoading))
                  }
                  onClick={() => void doDownload()}
                >
                  <IconDownload />
                  {wmEnabled ? "下载 MP4（去水印）" : "开始下载 MP4"}
                </button>
              )}
              {mode === "dash" && (
                <span
                  className="help"
                  style={{ alignSelf: "center", marginTop: 0 }}
                >
                  预计大小 ≈{" "}
                  <b style={{ color: "var(--dim)" }}>
                    {humanSize(estSizes.video + estSizes.audio)}
                  </b>
                </span>
              )}
            </div>
          </section>
        )}

        {/* 4. 进度 */}
        {showProgress && progress && (
          <section className="card fade-in">
            <div className="card-title">
              <span className="idx">4</span>
              {progress.stage === "done"
                ? "下载完成"
                : progress.stage === "error"
                ? "下载失败"
                : progress.stage === "cancelled"
                ? "已取消"
                : "下载进度"}
            </div>
            <div className="progress-shell">
              <div className="progress-head">
                <span className="progress-percent">
                  {progress.stage === "merging"
                    ? "合并中"
                    : `${progress.percent.toFixed(1)}%`}
                </span>
                <span className="progress-msg">{progress.message}</span>
                {progress.speed > 0 && (
                  <span className="progress-speed">
                    {humanSpeed(progress.speed)}
                  </span>
                )}
              </div>
              <div className="bar">
                <div
                  className={`bar-fill ${
                    progress.stage === "done"
                      ? "done"
                      : progress.stage === "merging"
                      ? "merging"
                      : progress.stage === "error"
                      ? "error"
                      : ""
                  }`}
                  style={{
                    width: `${
                      progress.stage === "merging"
                        ? 100
                        : Math.max(progress.percent, progress.stage === "done" ? 100 : 1.5)
                    }%`,
                  }}
                />
              </div>
              <div className="stats-row">
                <span>
                  已下载 <b>{humanSize(progress.downloaded)}</b>
                </span>
                {progress.total > 0 && (
                  <span>
                    总大小 <b>{humanSize(progress.total)}</b>
                  </span>
                )}
                {progress.phase && (
                  <span>
                    阶段{" "}
                    <b>
                      {progress.phase === "video"
                        ? "视频流"
                        : progress.phase === "audio"
                        ? "音频流"
                        : progress.phase === "merge"
                        ? "合并"
                        : "文件"}
                    </b>
                  </span>
                )}
              </div>
            </div>
          </section>
        )}

        {/* 5. 结果 */}
        {result && (
          <section className="card result-card fade-in" ref={resultRef}>
            <div className="card-title">
              <span className="idx" style={{ background: "rgba(63,185,80,.16)", color: "#7ee787" }}>
                <IconCheck />
              </span>
              MP4 已生成
            </div>
            <div className="result-row">
              <div className="result-icon">
                <IconCheck />
              </div>
              <div className="result-main">
                <div className="result-name" title={result.fileName}>
                  {result.fileName}
                </div>
                <div className="result-sub" title={result.filePath}>
                  {result.sizeText} · {result.qualityText} · 用时{" "}
                  {result.elapsed.toFixed(1)} 秒 · {result.dir}
                </div>
              </div>
              <button className="btn" onClick={() => playFile(result.filePath)}>
                <IconPlay />
                播放
              </button>
              <button className="btn" onClick={() => openInFolder(result.filePath)}>
                <IconFolder />
                打开所在文件夹
              </button>
            </div>
          </section>
        )}

        {!info && !parsing && (
          <div className="empty">
            <IconFilm />
            <div>
              把 B 站视频链接粘到上面的输入框，回车即可解析
              <br />
              解析后可选择清晰度、分 P 与保存位置，一键导出 .mp4
            </div>
          </div>
        )}
      </main>

      {/* 设置抽屉 */}
      {showSettings && (
        <div className="overlay" onClick={() => setShowSettings(false)}>
          <div className="drawer" onClick={(e) => e.stopPropagation()}>
            <div className="drawer-head">
              设置
              <div className="spacer" />
              <button
                className="btn btn-sm btn-ghost"
                onClick={() => setShowSettings(false)}
              >
                <IconX />
              </button>
            </div>
            <div className="drawer-body">
              {/* Cookie */}
              <div>
                <div className="field-label">登录 Cookie（可选）</div>
                <input
                  className="text-input"
                  style={{ width: "100%" }}
                  placeholder="粘贴 SESSDATA 的值"
                  value={sessdata}
                  spellCheck={false}
                  onChange={(e) => setSessdata(e.target.value)}
                  onBlur={saveSessdata}
                />
                <div className="help">
                  不填也能用，但未登录时最高只有 480P。
                  <br />
                  获取方法：浏览器登录 B 站 → 按 <code>F12</code> →{" "}
                  <code>Application</code> → <code>Cookies</code> →{" "}
                  <code>https://www.bilibili.com</code> → 复制{" "}
                  <code>SESSDATA</code> 的值粘到这里。
                  <br />
                  <span style={{ color: "var(--warn)" }}>
                    该值等同于你的登录凭证，仅保存在本机，请勿分享给他人。
                  </span>
                </div>
                <div style={{ marginTop: 10, display: "flex", gap: 8 }}>
                  <button className="btn btn-sm" onClick={saveSessdata}>
                    保存并重新加载画质
                  </button>
                  <button
                    className="btn btn-sm"
                    onClick={() => {
                      setSessdata("");
                      localStorage.removeItem("bili_sessdata");
                      setSessdataActive("");
                    }}
                  >
                    清除
                  </button>
                </div>
              </div>

              {/* ffmpeg */}
              <div>
                <div className="field-label">
                  ffmpeg
                  {ffmpeg?.found && (
                    <span className="tip" style={{ color: "var(--ok)" }}>
                      已就绪
                    </span>
                  )}
                </div>
                <div className="kv">
                  <span className="k">状态</span>
                  <span className="v">
                    {ffmpeg
                      ? ffmpeg.found
                        ? `已找到（${ffmpeg.source}）`
                        : "未找到"
                      : "检测中…"}
                  </span>
                </div>
                {ffmpeg?.path && (
                  <div className="kv">
                    <span className="k">路径</span>
                    <span className="v">{ffmpeg.path}</span>
                  </div>
                )}
                {ffmpeg?.version && (
                  <div className="kv">
                    <span className="k">版本</span>
                    <span className="v">{ffmpeg.version}</span>
                  </div>
                )}

                {installing ? (
                  <div style={{ marginTop: 12 }}>
                    <div className="progress-head" style={{ marginBottom: 7 }}>
                      <span className="progress-msg">
                        {installProg?.message ?? "正在下载 ffmpeg…"}
                      </span>
                      <span className="progress-speed">
                        {humanSize(installProg?.downloaded ?? 0)}
                        {installProg && installProg.total > 0
                          ? ` / ${humanSize(installProg.total)}`
                          : ""}
                      </span>
                    </div>
                    <div className="bar">
                      <div
                        className="bar-fill"
                        style={{ width: `${installProg?.percent ?? 0}%` }}
                      />
                    </div>
                  </div>
                ) : (
                  <>
                    <div className="help">
                      高清模式需要 ffmpeg 把分离的音视频流合并成 MP4。
                      点击下方按钮自动下载（约 22–80 MB，内置多个下载源）。
                    </div>
                    <button
                      className="btn btn-primary btn-sm"
                      style={{ marginTop: 10 }}
                      onClick={() => void doInstallFfmpeg()}
                    >
                      <IconDownload />
                      {ffmpegOk ? "重新下载 ffmpeg" : "一键安装 ffmpeg"}
                    </button>
                  </>
                )}

                <div className="help" style={{ marginTop: 10 }}>
                  也可以自己安装 ffmpeg 并把它的目录加入系统 PATH，
                  或把 <code>ffmpeg.exe</code> 放到本程序同级目录。
                </div>
              </div>

              <div className="alert alert-info" style={{ fontSize: 12 }}>
                <IconInfo />
                <div>
                  本工具仅解析 B 站公开视频流用于个人离线保存，请勿用于商业传播。
                  下载内容的版权归原作者所有。
                </div>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* 水印区域框选 */}
      {showWmPicker && wmPreview?.frame && (
        <WatermarkPicker
          preview={wmPreview}
          value={wmBox}
          onChange={setWmBox}
          onClose={() => setShowWmPicker(false)}
        />
      )}
    </div>
  );
}

/* ------------------------------------------------------------------ 水印区域框选 */

function WatermarkPicker({
  preview,
  value,
  onChange,
  onClose,
}: {
  preview: WatermarkPreview;
  value: WmBox | null;
  onChange: (b: WmBox) => void;
  onClose: () => void;
}) {
  const stageRef = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<{
    mode: "move" | "size";
    x: number;
    y: number;
    box: WmBox;
  } | null>(null);

  const srcW = Math.max(preview.srcWidth || 1, 1);
  const srcH = Math.max(preview.srcHeight || 1, 1);
  const box = value ?? defaultWmBox(srcW, srcH);

  // CSS 像素 -> 源像素 的换算
  const ratio = () => {
    const el = stageRef.current;
    if (!el) return { kx: 1, ky: 1 };
    const r = el.getBoundingClientRect();
    return { kx: srcW / (r.width || 1), ky: srcH / (r.height || 1) };
  };

  const start = (e: React.PointerEvent, mode: "move" | "size") => {
    e.preventDefault();
    e.stopPropagation();
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    setDrag({ mode, x: e.clientX, y: e.clientY, box });
  };

  const move = (e: React.PointerEvent) => {
    if (!drag) return;
    const { kx, ky } = ratio();
    const dx = (e.clientX - drag.x) * kx;
    const dy = (e.clientY - drag.y) * ky;
    if (drag.mode === "move") {
      onChange({
        ...drag.box,
        x: Math.round(clamp(drag.box.x + dx, 1, srcW - drag.box.w - 2)),
        y: Math.round(clamp(drag.box.y + dy, 1, srcH - drag.box.h - 2)),
      });
    } else {
      onChange({
        ...drag.box,
        w: Math.round(clamp(drag.box.w + dx, 12, srcW - drag.box.x - 2)),
        h: Math.round(clamp(drag.box.h + dy, 8, srcH - drag.box.y - 2)),
      });
    }
  };

  const end = () => setDrag(null);

  const preset = (pos: "tl" | "tr" | "bl" | "br") => {
    const w = Math.round(srcW * 0.24);
    const h = Math.round(srcH * 0.1);
    const mx = Math.round(srcW * 0.012);
    const my = Math.round(srcH * 0.02);
    onChange({
      x: pos === "tl" || pos === "bl" ? mx : srcW - w - mx,
      y: pos === "tl" || pos === "tr" ? my : srcH - h - my,
      w,
      h,
    });
  };

  const pct = (v: number, total: number) => `${(v / total) * 100}%`;

  return (
    <div className="overlay overlay-center" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="drawer-head">
          框选水印区域
          <span className="tip" style={{ fontWeight: 400, marginLeft: 10 }}>
            拖动方框可移动，拖右下角圆点可改大小
          </span>
          <div className="spacer" />
          <button className="btn btn-sm btn-ghost" onClick={onClose}>
            <IconX />
          </button>
        </div>
        <div className="modal-body">
          <div className="wm-stage" ref={stageRef}>
            <img src={preview.frame ?? ""} alt="画面" draggable={false} />
            <div
              className="wm-box"
              style={{
                left: pct(box.x, srcW),
                top: pct(box.y, srcH),
                width: pct(box.w, srcW),
                height: pct(box.h, srcH),
              }}
              onPointerDown={(e) => start(e, "move")}
              onPointerMove={move}
              onPointerUp={end}
              onPointerCancel={end}
            >
              <span
                className="wm-handle"
                onPointerDown={(e) => start(e, "size")}
                onPointerMove={move}
                onPointerUp={end}
                onPointerCancel={end}
              />
            </div>
          </div>

          <div className="wm-bar">
            <span className="tip">
              原视频 {srcW}×{srcH}
            </span>
            <span className="bvid-tag">
              x {box.x} · y {box.y} · {box.w}×{box.h}
            </span>
            <div className="spacer" />
            <button className="btn btn-sm" onClick={() => preset("tl")}>
              左上
            </button>
            <button className="btn btn-sm" onClick={() => preset("tr")}>
              右上
            </button>
            <button className="btn btn-sm" onClick={() => preset("bl")}>
              左下
            </button>
            <button className="btn btn-sm" onClick={() => preset("br")}>
              右下
            </button>
            <button className="btn btn-sm btn-primary" onClick={onClose}>
              完成
            </button>
          </div>

          <div className="help">
            把红框贴合水印本身即可，不要放太大 —— 框内内容会被插值抹平（看起来变糊）。
            框太大会糊掉更多正常画面。
          </div>
        </div>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ 工具 */

function highestLabel(groups: QualityGroup[]): string {
  if (!groups.length) return "—";
  return groups[0].label;
}

/** 去掉错误串里多余的包装层级 */
function cleanError(s: string): string {
  return s.replace(/^Error:\s*/i, "").trim();
}

function clamp(v: number, lo: number, hi: number): number {
  if (hi < lo) return lo;
  return v < lo ? lo : v > hi ? hi : v;
}

/** 自动检测失败时的兜底框：B 站水印通常在左上角 */
function defaultWmBox(srcW: number, srcH: number): WmBox {
  const w = Math.max(40, Math.round(srcW * 0.22));
  const h = Math.max(16, Math.round(srcH * 0.09));
  return {
    x: Math.max(2, Math.round(srcW * 0.012)),
    y: Math.max(2, Math.round(srcH * 0.02)),
    w,
    h,
  };
}
