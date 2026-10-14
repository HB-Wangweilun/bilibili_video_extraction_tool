"""把用户提供的 logo 图处理成符合 Tauri 要求的方形图标源图。

- 裁成正方形
- 从四角/四边泛洪，把连通到边界的白色背景抠成透明
- 保留 logo 内部的白色（泛洪到不了）
- 缩放为 1024x1024
"""
import os
import sys

from PIL import Image, ImageDraw

SRC = r"C:\Users\13093\.workbuddy\clipboard-images\clipboard-2026-10-14T14-38-19-362Z-c1394bf7.jpg"
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_ICON = os.path.join(ROOT, "scripts", "app-icon.png")
OUT_ASSET = os.path.join(ROOT, "src", "assets", "app-icon.png")
SIZE = 1024
FILL = (255, 0, 255)          # 泛洪用的哨兵色
THRESH = 34                   # 判定为"白"的容差


def main() -> int:
    im = Image.open(SRC).convert("RGB")
    w, h = im.size

    # 1. 居中裁成正方形
    side = min(w, h)
    left = (w - side) // 2
    top = (h - side) // 2
    im = im.crop((left, top, left + side, top + side))
    print(f"裁成正方形: {im.size}")

    # 2. 从四角 + 四边中点泛洪，只吃掉连通到边界的白色
    work = im.copy()
    step = max(1, side // 40)
    seeds = []
    for x in range(0, side, step):
        seeds += [(x, 0), (x, side - 1)]
    for y in range(0, side, step):
        seeds += [(0, y), (side - 1, y)]

    seeds = [(px, py) for (px, py) in seeds if sum(work.getpixel((px, py))) / 3 > 255 - THRESH]
    print(f"泛洪种子点: {len(seeds)} 个")

    for px, py in seeds:
        try:
            ImageDraw.floodfill(work, (px, py), FILL, thresh=THRESH)
        except Exception:
            pass

    # 3. 生成 alpha（注意：要在泛洪后的 work 上取像素）
    rgba = work.convert("RGBA")
    px = rgba.load()
    bg_count = 0
    for y in range(side):
        for x in range(side):
            if px[x, y][:3] == FILL:
                px[x, y] = (0, 0, 0, 0)
                bg_count += 1

    ratio = bg_count / (side * side)
    print(f"抠成透明的比例: {ratio * 100:.1f}%")

    if ratio > 0.62:
        print("!! 泛洪可能泄漏到了 logo 内部（透明比例过高），保留白底版本")
        rgba = im.convert("RGBA")

    # 4. 缩放
    rgba = rgba.resize((SIZE, SIZE), Image.LANCZOS)
    for path in (OUT_ICON, OUT_ASSET):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        rgba.save(path, "PNG")
        print("已写出:", path, os.path.getsize(path), "bytes")

    # 报告实际生效的透明比例，便于判断是否合理
    a = rgba.getchannel("A")
    px1024 = a.load()
    n = sum(1 for y in range(0, SIZE, 4) for x in range(0, SIZE, 4) if px1024[x, y] < 8)
    print(f"最终图中全透明像素占比: {n / (SIZE // 4) ** 2 * 100:.1f}%")
    return 0


if __name__ == "__main__":
    sys.exit(main())
