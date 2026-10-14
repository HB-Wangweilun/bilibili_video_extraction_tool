"""端到端验证：解析 B 站链接 -> 下载 DASH 音视频流 -> ffmpeg 合并为 mp4。

不依赖 Tauri，用于提前验证核心链路是否可行。
"""
import json
import os
import subprocess
import sys
import urllib.request

UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
)
BV = "BV1Wy7n6yEA5"
WORK = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".tools", "e2e")
FFMPEG = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".tools", "ffmpeg.exe")


def api(url, referer=f"https://www.bilibili.com/video/{BV}"):
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Referer": referer})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.loads(r.read().decode("utf-8"))


def download(url, path, referer=f"https://www.bilibili.com/video/{BV}"):
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Referer": referer})
    with urllib.request.urlopen(req, timeout=120) as r, open(path, "wb") as f:
        total = 0
        while True:
            chunk = r.read(1 << 16)
            if not chunk:
                break
            f.write(chunk)
            total += len(chunk)
    return total


def main():
    os.makedirs(WORK, exist_ok=True)

    print("== 1. 视频信息 ==")
    view = api(f"https://api.bilibili.com/x/web-interface/view?bvid={BV}")
    assert view["code"] == 0, view
    d = view["data"]
    cid = d["cid"]
    print(f"  标题: {d['title']}")
    print(f"  UP主: {d['owner']['name']}  时长: {d['duration']}s  cid: {cid}")

    print("== 2. 播放地址 ==")
    purl = api(
        f"https://api.bilibili.com/x/player/playurl?bvid={BV}&cid={cid}"
        f"&fnval=4048&fnver=0&fourk=1&platform=pc&qn=127"
    )
    assert purl["code"] == 0, purl
    dash = purl["data"]["dash"]
    vids = dash["video"]
    auds = dash["audio"]
    print(f"  视频流: {[(v['id'], v['codecid'], v.get('width'), v.get('height')) for v in vids]}")
    print(f"  音频流: {[(a['id'], a.get('bandwidth')) for a in auds]}")

    # 选 AVC(7) 优先、画质最高的视频流
    avc = [v for v in vids if v.get("codecid") == 7]
    v = max(avc or vids, key=lambda x: (x["id"], x.get("bandwidth", 0)))
    a = max(auds, key=lambda x: x.get("bandwidth", 0))
    print(f"  选用: 视频 id={v['id']} codecid={v['codecid']} / 音频 id={a['id']}")

    print("== 3. 下载 ==")
    vpath = os.path.join(WORK, "video.mp4")
    apath = os.path.join(WORK, "audio.mp4")
    nv = download(v["baseUrl"], vpath)
    na = download(a["baseUrl"], apath)
    print(f"  video.mp4 = {nv/1048576:.2f} MB")
    print(f"  audio.mp4 = {na/1048576:.2f} MB")

    print("== 4. ffmpeg 合并 ==")
    out = os.path.join(WORK, "output.mp4")
    cmd = [
        FFMPEG, "-y", "-hide_banner", "-loglevel", "error",
        "-i", vpath, "-i", apath,
        "-map", "0:v:0", "-map", "1:a:0",
        "-c", "copy", "-movflags", "+faststart", "-f", "mp4", out,
    ]
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        print("  ffmpeg 失败:", r.stderr)
        return 1
    print(f"  output.mp4 = {os.path.getsize(out)/1048576:.2f} MB")

    print("== 5. 校验产物 ==")
    r = subprocess.run([FFMPEG, "-hide_banner", "-i", out], capture_output=True, text=True)
    for line in r.stderr.splitlines():
        if "Duration" in line or "Stream #" in line:
            print("  " + line.strip())

    # 完整解码一遍，确保没有损坏
    r = subprocess.run(
        [FFMPEG, "-v", "error", "-i", out, "-f", "null", "-"],
        capture_output=True, text=True,
    )
    if r.returncode != 0 or r.stderr.strip():
        print("  ⚠ 解码校验报错:", r.stderr[:500])
        return 1
    print("  ✅ 完整解码通过，文件可正常播放")
    return 0


if __name__ == "__main__":
    sys.exit(main())
