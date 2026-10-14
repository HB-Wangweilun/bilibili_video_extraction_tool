"""检测视频里的静态水印位置。

原理：B 站水印是半透明白色叠加层，位置固定；视频内容在动。
所以「在绝大多数采样帧里都是白色/浅色」的像素，聚成的紧凑区块就是水印。
"""
import glob
import os
import shutil
import subprocess
import sys

from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FFMPEG = os.path.join(ROOT, ".tools", "ffmpeg.exe")
SRC = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, ".tools", "e2e", "output.mp4")
OUT = os.path.join(ROOT, ".tools", "wm_frames")
W = 640


def main():
    if not os.path.isfile(SRC):
        print("找不到源视频:", SRC)
        return 1
    shutil.rmtree(OUT, ignore_errors=True)
    os.makedirs(OUT, exist_ok=True)

    print(f"源视频: {SRC}")
    r = subprocess.run(
        [FFMPEG, "-y", "-hide_banner", "-loglevel", "error",
         "-i", SRC, "-vf", f"fps=1/4,scale={W}:-2", os.path.join(OUT, "%03d.png")],
        capture_output=True, text=True,
    )
    if r.returncode != 0:
        print("抽帧失败:", r.stderr[:400])
        return 1

    files = sorted(glob.glob(os.path.join(OUT, "*.png")))
    print(f"采样帧数: {len(files)}")
    if len(files) < 4:
        print("帧太少，无法判断")
        return 1

    frames = [Image.open(f).convert("RGB") for f in files]
    width, height = frames[0].size
    print(f"分析尺寸: {width}x{height}")

    n = len(frames)
    # 逐像素统计"浅色且低饱和"的比例
    white_ratio = [[0.0] * width for _ in range(height)]
    for y in range(height):
        for x in range(width):
            cnt = 0
            for f in frames:
                r_, g_, b_ = f.getpixel((x, y))
                mx, mn = max(r_, g_, b_), min(r_, g_, b_)
                if mn > 165 and (mx - mn) < 46:      # 接近白/浅灰
                    cnt += 1
            white_ratio[y][x] = cnt / n

    # 标记"持续浅色"的像素
    persist = [[white_ratio[y][x] >= 0.75 for x in range(width)] for y in range(height)]

    # 在四个角落区域里找最大连通块的外接框（用简单的 BFS）
    corners = {
        "右上": (width // 2, width, 0, height // 3),
        "左上": (0, width // 2, 0, height // 3),
        "右下": (width // 2, width, height * 2 // 3, height),
        "左下": (0, width // 2, height * 2 // 3, height),
    }
    results = {}
    for name, (x0, x1, y0, y1) in corners.items():
        pts = [(x, y) for y in range(y0, y1) for x in range(x0, x1) if persist[y][x]]
        if not pts:
            results[name] = None
            continue
        # 取该区域所有持续浅色像素的外接框（水印笔画本身是分散的，用整体框更稳）
        xs = [p[0] for p in pts]
        ys = [p[1] for p in pts]
        results[name] = (min(xs), min(ys), max(xs), max(ys), len(pts))

    print("\n=== 各角落持续浅色像素分布 ===")
    for name, box in results.items():
        if box is None:
            print(f"  {name}: 无")
        else:
            bx0, by0, bx1, by1, cnt = box
            print(f"  {name}: 外接框 (x{bx0},y{by0})-({bx1},{by1})  "
                  f"尺寸 {bx1-bx0+1}x{by1-by0+1}  像素数 {cnt}  "
                  f"占该角落 {cnt/((bx1-bx0+1)*(by1-by0+1))*100:.1f}%")

    # 全局：持续浅色像素总数
    total = sum(1 for y in range(height) for x in range(width) if persist[y][x])
    print(f"\n全图持续浅色像素: {total} ({total/(width*height)*100:.2f}%)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
