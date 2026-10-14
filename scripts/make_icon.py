"""生成应用图标源图（1024x1024 PNG），无需第三方依赖。"""
import math
import os
import struct
import zlib

SIZE = 1024
RADIUS = int(SIZE * 0.225)

# B 站品牌色
C_BLUE = (0x00, 0xA1, 0xD6)
C_PINK = (0xFB, 0x72, 0x99)
C_DEEP = (0x0A, 0x6E, 0xA8)


def clamp01(v):
    return 0.0 if v < 0.0 else (1.0 if v > 1.0 else v)


def rounded_rect_sdf(px, py, half_w, half_r):
    """圆角正方形 SDF，中心在原点。"""
    qx = abs(px) - (half_w - half_r)
    qy = abs(py) - (half_w - half_r)
    ax = max(qx, 0.0)
    ay = max(qy, 0.0)
    return math.hypot(ax, ay) + min(max(qx, qy), 0.0) - half_r


def triangle_sdf(px, py, verts):
    """凸多边形近似 SDF（内部为负）。"""
    d = -1e9
    n = len(verts)
    for i in range(n):
        ax, ay = verts[i]
        bx, by = verts[(i + 1) % n]
        ex, ey = bx - ax, by - ay
        # 外法线（顶点按顺时针给出时）
        nx, ny = ey, -ex
        ln = math.hypot(nx, ny) or 1.0
        nx, ny = nx / ln, ny / ln
        d = max(d, (px - ax) * nx + (py - ay) * ny)
    return d


def mix(a, b, t):
    return tuple(a[i] + (b[i] - a[i]) * t for i in range(3))


def build_pixels():
    half = SIZE / 2.0
    # 播放三角形（顺时针）
    tri = [
        (half - 168, half - 214),
        (half - 168, half + 214),
        (half + 216, half),
    ]

    rows = []
    for y in range(SIZE):
        row = bytearray()
        for x in range(SIZE):
            px = x + 0.5 - half
            py = y + 0.5 - half

            # 背景：对角渐变 蓝 -> 深蓝 -> 粉
            t = clamp01((px / SIZE) * 0.5 + (py / SIZE) * 0.5 + 0.5)
            if t < 0.55:
                bg = mix(C_BLUE, C_DEEP, t / 0.55)
            else:
                bg = mix(C_DEEP, C_PINK, (t - 0.55) / 0.45)

            cov = clamp01(0.5 - rounded_rect_sdf(px, py, half, RADIUS))
            if cov <= 0.0:
                row += bytes((0, 0, 0, 0))
                continue

            # 播放按钮
            tc = clamp01(0.5 - triangle_sdf(px, py, tri))
            r = bg[0]
            g = bg[1]
            b = bg[2]
            if tc > 0:
                r = r + (255 - r) * tc
                g = g + (255 - g) * tc
                b = b + (255 - b) * tc

            row += bytes((int(r + 0.5), int(g + 0.5), int(b + 0.5), int(cov * 255 + 0.5)))
        rows.append(bytes(row))
    return rows


def write_png(path, rows):
    raw = b"".join(b"\x00" + r for r in rows)

    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(png)


if __name__ == "__main__":
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "app-icon.png")
    write_png(out, build_pixels())
    print("written:", out, os.path.getsize(out), "bytes")
