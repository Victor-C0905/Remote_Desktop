---
name: quirel-redlines-window
description: Quirel 窗口坏帧红线。触碰 lib.rs 窗口创建/Resized/NCHITTEST 子类/no_erase_proc、fullscreen.rs 全屏切换、壁纸系统（wallpaperStore/WallpaperContext）、browser webview bounds（webview_host.rs/browser_window.rs）、升级 tauri/wry/tao 版本时必须先加载本技能。
---

# 窗口坏帧红线（AGENTS.md §5 红线表第 2 行）

## 改动前（强制）
1. 通读 `docs/window-flash-rules.md`（根因链、已否定方案、硬红线 §5、探针回归 §7）。
2. 对照硬红线自查：主窗口 WM_ERASEBKGND 必须以 WINDOW_BG_COLORREF 填色；子窗口链
   必须维持「不擦」；壁纸变更必须经 set_window_background_color 同步双层背景色；
   主窗口必须 transparent(false)。

## 改动后（强制回归）
- 按 `docs/window-flash-rules.md` §7 用 dupcap 探针（DXGI Desktop Duplication 帧级
  抓屏）回归最大化/缩放路径；判定标准见该文档（frames.csv h0 无 >55000 尖峰）。
- 无法在本机执行探针时：如实声明未回归，禁止声称「已验证」。

## 背景速览
最大化/缩放时窗口暴露区在 WebView2 新帧就位前必然存在坏帧（13~55ms，与刷新率无关），
现方案为「以当前壁纸平均色填充暴露区」（实验 G 定稿）。任何「简化」该链路的改动
都曾引发闪黑回归。
