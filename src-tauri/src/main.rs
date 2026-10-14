// Windows 发布版不弹出控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 无界面自检：BiliVideoExtractor.exe --selftest [--wm] <链接> [输出目录]
    // 走一遍真实的解析 / 下载 / 合并（可选去水印）流程，用于排查问题。
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) == Some("--selftest") {
        let wm = args.iter().any(|a| a == "--wm");
        let rest: Vec<&str> = args
            .iter()
            .filter(|a| !a.starts_with("--"))
            .map(|s| s.as_str())
            .collect();
        let url = rest
            .first()
            .copied()
            .unwrap_or("https://www.bilibili.com/video/BV1gjpR68Ejh/");
        let out = rest.get(1).copied();
        std::process::exit(bili_video_extractor_lib::selftest(url, out, wm));
    }

    bili_video_extractor_lib::run()
}
