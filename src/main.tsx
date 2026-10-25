import "./i18n";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { isTauri, suppressTauriBridgeErrors } from "./utils/environment";
import { invoke } from "@tauri-apps/api/core";
import { probeKeyring } from "./utils/credentials";
import { useSettingsStore } from "./stores/settingsStore";
import { useWallpaperStore } from "./stores/wallpaperStore";

// 浏览器调试模式（无 Tauri 运行时）：静默桥接类报错，桌面会显示「浏览器模式」横幅
if (!isTauri()) {
  suppressTauriBridgeErrors();
}

// Hydration 门控：persist 存储底层是异步 Tauri IPC，不等 rehydrate 就渲染
// 会让首帧落在出厂默认值上（默认壁纸蓝渐变/默认主题），随后跳变——
// 这里等关键视觉 store 恢复完成再挂载，首帧即用户的真实壁纸与主题。
// allSettled：非 Tauri 调试环境下 storage 不可用也不阻塞渲染。
async function boot() {
  await Promise.allSettled([
    useSettingsStore.persist.rehydrate(),
    useWallpaperStore.persist.rehydrate(),
  ]);

  // 壁纸为缓存图片时，读字节生成会话 blob URL（渲染主通道，见 wallpaperStore）
  void useWallpaperStore.getState().loadCachedWallpaper();

  // keyring 健康探测（不可用时通知；含凭据的表单提交将前置拦截）
  if (isTauri()) void probeKeyring();

  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );

  // 主窗口以 visible(false) 创建（治启动闪现）：首帧渲染完成后显示。
  // 双 rAF 确保挂载后的第一帧已提交合成器。
  requestAnimationFrame(() => requestAnimationFrame(() => {
    if (isTauri()) invoke("show_main_window").catch(() => {});
  }));
}

void boot();

// ── resize 闪屏闸门（A2）──────────────────────────────
// 透明窗口 resize 期间 WebView2 重绘 + 整桌面重排叠加导致掉帧闪烁：
// resize 进行中给根节点挂 .resizing（base.css 中降级渲染——暂停动画与
// backdrop-filter，与 os-minimized 闸门同思路），结束后 150ms 恢复。
if (typeof window !== "undefined") {
  let resizeTimer: number | undefined;
  window.addEventListener("resize", () => {
    if (!document.documentElement.classList.contains("resizing")) {
      document.documentElement.classList.add("resizing");
    }
    window.clearTimeout(resizeTimer);
    resizeTimer = window.setTimeout(() => {
      document.documentElement.classList.remove("resizing");
    }, 150);
  });
}
