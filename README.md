# B站视频解析工具

> 粘贴一个 B 站链接，导出一份能直接播放的 `.mp4`。

基于 **Tauri 2** 的 Windows 桌面应用。前端 React + TypeScript，后端 Rust。
不依赖浏览器插件、不改 hosts、不需要你自己配 ffmpeg —— 打开就能用。

```
┌──────────────────────────────────────────────────────────┐
│  ① 粘贴链接 →  ② 选清晰度 / 编码 / 分P / 保存位置        │
│  ③ 开始下载 →  ④ 拿到 MP4                                │
└──────────────────────────────────────────────────────────┘
```

---

## 功能

| 功能 | 说明 |
| --- | --- |
| **链接解析** | 完整链接（带一堆跟踪参数也行）、`b23.tv` 短链、纯 `BV` 号、`av` 号，也能从分享文案里自动认出来 |
| **视频信息预览** | 封面、标题、UP 主、时长、播放/点赞数 |
| **清晰度选择** | 走 DASH 流，拿该视频实际可用的最高画质（登录后最高 4K） |
| **编码选择** | 同一画质可选 `AVC (H.264)` / `HEVC (H.265)` / `AV1`，默认 AVC（兼容性最好） |
| **音轨选择** | 普通音轨 / 杜比全景声 / Hi-Res 无损 |
| **分 P 支持** | 多 P 视频可逐个选择下载 |
| **并发下载** | 8 分片 Range 并发，失败断点续传，主地址挂自动切备用 CDN |
| **实时进度** | 下载进度、速度、剩余量；可随时取消 |
| **去水印** | 自动定位画面中的水印并抹除，也可在画面上手动拖框修正 |
| **ffmpeg 一键安装** | 内置多个下载源，国内镜像优先，不用自己配环境 |
| **登录 Cookie** | 填 `SESSDATA` 解锁 1080P / 4K |
| 贴心细节 | 同名文件自动加序号（不覆盖）、文件名非法字符自动清洗、完成后可直接播放 |

---

## 下载与运行

在 [Releases](../../releases) 页面下载：

| 文件 | 用途 |
| --- | --- |
| **`BiliVideoExtractor.exe`** | 单文件，**双击直接运行**，不需要安装。推荐 |
| `BiliVideoExtractor_0.1.0_x64-setup.exe` | 安装版，会创建开始菜单与卸载项 |

**运行环境**

- Windows 10 1809+ / Windows 11（64 位）
- [WebView2 运行时](https://developer.microsoft.com/microsoft-edge/webview2/) —— Win11 与较新版 Win10 已自带
- 高清模式需要 ffmpeg —— **程序内「设置」里可以一键下载**，无需手动配置

---

## 使用说明

1. 双击 `BiliVideoExtractor.exe`
2. 把链接粘进输入框，回车（或点「解析」）
   - 直接 Ctrl+V 粘贴会自动触发解析
   - 不想找链接就点「用示例」试试
3. 选分 P、清晰度、编码、音轨，确认保存位置
4. 点「开始下载 MP4」
5. 完成后可以「播放」或「打开所在文件夹」

### 想下 1080P / 4K？

B 站对未登录用户限制到 480P。填入登录凭证即可解锁：

> 浏览器登录 B 站 → 按 `F12` → `Application` → `Cookies` → `https://www.bilibili.com`
> → 找到 `SESSDATA` → 复制它的值 → 粘到本程序「设置」里 → 保存

> ⚠️ `SESSDATA` 等同于你的登录凭证，**只保存在本机**，不要分享给任何人。

### 两种下载模式

| 模式 | 原理 | 需要 ffmpeg | 画质 |
| --- | --- | --- | --- |
| **高清模式**（默认） | 下载 DASH 分离的视频流与音频流，再用 ffmpeg **无损封装**成 MP4 | ✅ | 高（可到 4K） |
| **兼容模式** | B 站直发的单个 MP4 文件，下完即用 | ❌ | 较低（未登录通常 720P 封顶） |

### 去水印

水印是**烧进画面像素**的半透明叠加层，不是单独的一层，所以没有任何"无损去除"的办法。
本工具用 ffmpeg 的 `delogo` 滤镜，拿水印**四周的像素做插值把这块抹平**：

- 会自动分析多帧画面，找出"位置固定不变"的浅色区域，定位水印
- 也可以在抓取到的画面上**手动拖框**修正（拖方框移动，拖右下角圆点改大小）
- 因为要重写像素，**必须重新编码视频** → 明显比普通下载慢；被抹区域会有轻微涂抹感
- 框选范围要**贴合水印本身**，框太大会糊掉更多正常画面
- 画面中的**动态 / 漂移水印**无法处理

---

## 工作原理

```
粘贴链接
   │
   ├─ 正则提取 BV / av 号；b23.tv 短链跟随 302 重定向还原
   │
   ├─ GET  x/web-interface/view          → 标题、封面、UP 主、分P(cid) 列表
   │
   ├─ GET  x/player/playurl
   │        ├─ fnval=4048 → DASH：video / audio 分离的 .m4s（画质高）
   │        └─ fnval=1    → durl：单个 .mp4（画质低，但不用合并）
   │
   ├─ 8 分片 Range 并发下载（失败重试 → 备用 CDN 回退）
   │
   └─ ffmpeg -c copy 无损封装 → .mp4
        └─ 若开启去水印：delogo 插值 + 重新编码 → .mp4
```

几个关键点：

- 所有请求都必须带桌面浏览器 `User-Agent` 和 `Referer: https://www.bilibili.com`，
  否则接口或 CDN 会返回 **403**。
- 画质列表由 `dash.video` 里实际存在的流 id 推导，
  **不采信接口的 `quality` 字段**（它会虚报）。
- DASH 音视频是分开的，`-c copy` 只重新封装、不重编码，所以合并几乎不损失画质。

---

## 从源码构建

**前置要求**

- [Rust](https://rustup.rs/)（stable，MSVC 工具链）+ Visual Studio 生成工具（含 C++ 桌面开发）
- [Node.js](https://nodejs.org/) 18+
- Windows SDK（装 VS 生成工具时一起勾选）

```bash
npm install

# 出单文件 exe（收集到 release/BiliVideoExtractor.exe）+ 安装包
npm run tauri:build

# 只要单文件 exe，不生成安装程序（更快）
npm run tauri:exe

# 开发模式（前端热更新 + 桌面调试）
npm run tauri:dev

# 只跑前端
npm run dev
```

**产物位置**

- 单文件程序：`release/BiliVideoExtractor.exe`
- 安装包：`src-tauri/target/release/bundle/nsis/`
- 中间产物：`src-tauri/target/release/BiliVideoExtractor.exe`

> 首次 `cargo build` 要编译全部依赖，大约 10～20 分钟；之后增量只需几秒到几十秒。

---

## 项目结构

```
├── index.html
├── package.json
├── vite.config.ts
├── RELEASES.md                    版本发布记录
├── release/                       构建产物（已被 gitignore）
│   └── BiliVideoExtractor.exe     ← 双击即用的单文件程序
│
├── src/                           前端（React + TS，深色主题，无 UI 框架）
│   ├── main.tsx                   入口
│   ├── App.tsx                    全部界面与交互
│   ├── api.ts                     Tauri 命令封装
│   ├── types.ts                   与 Rust 结构体一一对应的类型
│   ├── format.ts                  字节/时长/速度等格式化
│   ├── styles.css                 样式
│   └── assets/app-icon.png        应用内顶部栏图标
│
├── src-tauri/                     Rust 后端
│   ├── tauri.conf.json            窗口、权限、打包配置
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs                程序入口（含 --selftest）
│       ├── lib.rs                 Tauri 命令编排、下载流程、去水印命令
│       ├── bilibili.rs            链接解析 / 视频信息 / 播放地址
│       ├── download.rs            分片并发下载、进度事件、文件名清洗
│       ├── ffmpeg.rs              ffmpeg 定位与安装、合并、抽帧、delogo
│       └── watermark.rs           静态水印自动定位
│
└── scripts/                       辅助脚本
    ├── collect-exe.mjs            收集构建产物到 release/
    ├── make_icon_from_logo.py     把 logo 处理成透明底方形图标
    ├── detect_watermark.py        独立的水印检测（对照/验证用）
    └── check_exe_icon.py          解析 PE 资源，确认 exe 内嵌图标
```

---

## 命令行自检

排查问题时可以不开界面，直接跑完整链路（需要用带控制台的 debug 构建）：

```bash
# 解析 → 下载 → 合并
src-tauri/target/debug/BiliVideoExtractor.exe --selftest "<B站链接>" "<输出目录>"

# 额外自动检测并去除水印
src-tauri/target/debug/BiliVideoExtractor.exe --selftest --wm "<B站链接>" "<输出目录>"
```

输出会打印每一步的结果（视频信息、可用清晰度、检测到的水印区域、下载进度、产物路径）。

---

## 常见问题

**Q：为什么最高只有 480P？**
A：未登录时 B 站只放 480P。到「设置」里填 `SESSDATA` 即可解锁 1080P / 4K。

**Q：提示需要 ffmpeg / 高清模式下不了？**
A：高清模式要靠 ffmpeg 把分离的音视频合并成一个 MP4。
到「设置」点「一键安装 ffmpeg」，或者先切到「兼容模式」直接下 MP4。

**Q：提示 403 / 风控 / 503？**
A：多半是短时间请求太频繁被 CDN 限流。稍等一会儿重试即可；
程序本身已内置备用线路回退与重试。

**Q：去水印为什么这么慢？**
A：去水印必须重新编码整段视频，而普通下载只是重新封装（不解码）。
1080P 的视频去水印通常要几分钟，属正常现象。

**Q：下载的视频没有声音？**
A：检查是否误选了「无音轨」；若音频流下载失败，程序会输出无声视频并在日志里提示，
重新下载一次通常就好。

**Q：能批量下载多 P 吗？**
A：暂时不能，一次下载一个分 P。已在后续计划中。

---

## 已知限制

- 不支持付费、大会员专享、仅客户端可播放等受保护内容。
- 去水印无法处理动态水印；被抹区域会有涂抹感。
- 多 P 视频暂不支持批量下载。
- 暂不支持弹幕、字幕导出。
- 内置播放按钮调用系统默认播放器，程序内没有播放器。

---

## 免责声明

本工具仅用于解析 B 站**公开视频流**，便于**个人离线备份与观看**。

下载内容的**版权归原作者所有**，请勿用于商业用途、二次分发或其他侵犯创作者权益的场景。
请尊重创作者，支持正版。
