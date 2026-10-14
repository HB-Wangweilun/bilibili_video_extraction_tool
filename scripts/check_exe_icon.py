"""解析 PE 资源，列出 exe 内嵌的图标资源（RT_ICON=3 / RT_GROUP_ICON=14）。

用于确认 Tauri 是否把 src-tauri/icons/icon.ico 真正嵌进了 exe。
"""
import struct
import sys

EXE = sys.argv[1] if len(sys.argv) > 1 else (
    r"src-tauri\target\release\bili-video-extractor.exe"
)
ICO = sys.argv[2] if len(sys.argv) > 2 else r"src-tauri\icons\icon.ico"


def main():
    data = open(EXE, "rb").read()
    if data[:2] != b"MZ":
        print("不是 PE 文件")
        return 1
    pe_off = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe_off : pe_off + 4] != b"PE\0\0":
        print("PE 签名不对")
        return 1

    machine, nsec, _, _, _, size_opt, _ = struct.unpack_from("<HHIIIHH", data, pe_off + 4)
    opt_off = pe_off + 24
    magic = struct.unpack_from("<H", data, opt_off)[0]
    dd_off = opt_off + (96 if magic == 0x10B else 112)

    res_rva, res_size = struct.unpack_from("<II", data, dd_off + 2 * 8)
    print(f"Machine=0x{machine:04x}  段数={nsec}  资源目录 RVA=0x{res_rva:x} size={res_size}")
    if res_rva == 0:
        print("!! 该 exe 没有资源目录")
        return 1

    sec_off = opt_off + size_opt
    sections = []
    for i in range(nsec):
        o = sec_off + i * 40
        name = data[o : o + 8].rstrip(b"\0").decode("latin1")
        vsize, vaddr, rawsize, rawptr = struct.unpack_from("<IIII", data, o + 8)
        sections.append((name, vaddr, vsize, rawptr, rawsize))

    def rva2off(rva):
        for name, vaddr, vsize, rawptr, rawsize in sections:
            if vaddr <= rva < vaddr + max(vsize, rawsize):
                return rawptr + (rva - vaddr)
        return None

    res_off = rva2off(res_rva)
    if res_off is None:
        print("资源段定位失败")
        return 1

    def read_dir(off, depth, path):
        """遍历资源目录，返回 [(类型, 名称, 语言, 数据RVA, 大小)]"""
        n_named, n_id = struct.unpack_from("<HH", data, off + 12)
        total = n_named + n_id
        out = []
        for i in range(total):
            eo = off + 16 + i * 8
            name_id, offset = struct.unpack_from("<II", data, eo)
            if name_id & 0x80000000:
                # 命名字符串
                str_off = res_off + (name_id & 0x7FFFFFFF)
                ln = struct.unpack_from("<H", data, str_off)[0]
                key = data[str_off + 2 : str_off + 2 + ln * 2].decode("utf-16le")
            else:
                key = name_id
            if offset & 0x80000000:
                out += read_dir(res_off + (offset & 0x7FFFFFFF), depth + 1, path + [key])
            else:
                de = res_off + offset
                drva, dsize, _, _ = struct.unpack_from("<IIII", data, de)
                out.append((path[0], path[1] if len(path) > 1 else None, key, drva, dsize))
        return out

    entries = read_dir(res_off, 0, [])
    types = {}
    for t, n, lang, rva, size in entries:
        types.setdefault(t, []).append((n, lang, rva, size))

    print("\n=== 资源类型统计 ===")
    for t in sorted(types, key=lambda x: (isinstance(x, str), x)):
        label = {3: "RT_ICON", 14: "RT_GROUP_ICON", 16: "RT_VERSION", 24: "RT_MANIFEST"}.get(
            t, str(t)
        )
        print(f"  {label}: {len(types[t])} 项")

    icons = types.get(3, [])
    if not icons:
        print("\n!! 没有 RT_ICON 资源 —— exe 里没有图标，任务栏会显示默认图标")
        return 1

    print(f"\n=== RT_ICON 明细（共 {len(icons)} 张）===")
    for n, lang, rva, size in sorted(icons, key=lambda x: x[3]):
        off = rva2off(rva)
        blob = data[off : off + size] if off is not None else b""
        kind = "PNG" if blob[:8] == b"\x89PNG\r\n\x1a\n" else "BMP/DIB"
        # 尝试解析尺寸
        dim = ""
        if kind == "PNG" and len(blob) >= 24:
            w = struct.unpack_from(">I", blob, 16)[0]
            h = struct.unpack_from(">I", blob, 20)[0]
            dim = f"{w}x{h}"
        elif kind == "BMP/DIB" and len(blob) >= 16:
            w = struct.unpack_from("<i", blob, 4)[0]
            h = abs(struct.unpack_from("<i", blob, 8)[0]) // 2
            dim = f"{w}x{h}"
        print(f"  名称={n} 语言={lang} {size} bytes {kind} {dim}")

    # 与 icon.ico 里的图像比对
    ico = open(ICO, "rb").read()
    _, _, cnt = struct.unpack("<HHH", ico[:6])
    ico_imgs = []
    for i in range(cnt):
        o = 6 + i * 16
        w, h, _, _, _, _, size, doff = struct.unpack("<BBBBHHII", ico[o : o + 16])
        ico_imgs.append((w or 256, size, ico[doff : doff + size]))

    exe_blobs = []
    for n, lang, rva, size in icons:
        off = rva2off(rva)
        exe_blobs.append(data[off : off + size])

    print("\n=== 与 icon.ico 比对 ===")
    matched = 0
    for w, size, blob in ico_imgs:
        hit = any(blob == b for b in exe_blobs)
        # 再宽松一点：用中段 200 字节做特征
        probe = blob[len(blob) // 2 : len(blob) // 2 + 200]
        loose = any(probe in b for b in exe_blobs)
        if hit or loose:
            matched += 1
        print(f"  {w}x{w}: {size} bytes -> {'匹配 ✅' if (hit or loose) else '不匹配 ❌'}")
    print(f"\n结论：{matched}/{len(ico_imgs)} 张与 icon.ico 一致")
    return 0 if matched else 2


if __name__ == "__main__":
    sys.exit(main())
