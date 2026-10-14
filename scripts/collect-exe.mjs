/**
 * 把 `tauri build` 产出的可执行文件收集到项目根目录的 release/ 下，
 * 方便直接拿走去双击运行。
 *
 * 产出：release/BiliVideoExtractor.exe  —— 单文件、双击即用，无需安装。
 */
import { copyFileSync, existsSync, mkdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const EXE_NAME = "BiliVideoExtractor.exe";

const candidates = [
  join(ROOT, "src-tauri", "target", "release", EXE_NAME),
  // 万一主二进制名被改回去，也兜一下
  join(ROOT, "src-tauri", "target", "release", "bili-video-extractor.exe"),
];

const src = candidates.find((p) => existsSync(p));
if (!src) {
  console.error("✗ 没有找到编译产物，请先确认 `tauri build` 成功。");
  console.error("  已查找：");
  for (const c of candidates) console.error("   -", c);
  process.exit(1);
}

const outDir = join(ROOT, "release");
mkdirSync(outDir, { recursive: true });
const dest = join(outDir, EXE_NAME);
copyFileSync(src, dest);

const mb = (statSync(dest).size / 1048576).toFixed(2);
console.log("");
console.log("┌─────────────────────────────────────────────");
console.log("│ ✅ 可直接运行的程序已就绪");
console.log("│");
console.log(`│    ${dest}`);
console.log(`│    大小：${mb} MB`);
console.log("│");
console.log("│    双击即可运行，无需安装。");
console.log("│    首次运行若提示缺少 WebView2，装一下微软的");
console.log("│    WebView2 运行时即可（Win11 和较新的 Win10 自带）。");
console.log("└─────────────────────────────────────────────");

const setup = join(
  ROOT,
  "src-tauri",
  "target",
  "release",
  "bundle",
  "nsis"
);
if (existsSync(setup)) {
  console.log("");
  console.log("（附带还生成了安装包，仅在想给别人装的时候用：");
  console.log(`  ${setup}）`);
}
