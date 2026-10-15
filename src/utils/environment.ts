/**
 * 运行环境检测
 *
 * 应用有两种运行环境：
 * - Tauri WebView（正式客户端）：完整的 IPC 能力（invoke / listen / plugin-store 等）
 * - 纯浏览器（直接访问 Vite dev server）：无 Tauri 运行时，IPC 不可用，
 *   仅用于 UI 开发调试
 *
 * 通过探测 Tauri v2 注入的 `window.__TAURI_INTERNALS__` 判断。
 */

/** 是否运行在 Tauri WebView 中 */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/**
 * 浏览器调试模式专用：静默吞掉 Tauri 桥接类错误
 *
 * 无 Tauri 运行时时，散布在各组件的 invoke / listen 调用会抛
 * "Cannot read properties of undefined (reading 'invoke'/'transformCallback')"。
 * 这类调用点分散且多为 fire-and-forget，逐一加 catch 成本高；
 * 此处按精确错误签名全局过滤（仅匹配 Tauri 桥接特征，不影响业务错误），
 * 配合桌面顶部的「浏览器模式」横幅告知功能受限。
 */
export function suppressTauriBridgeErrors(): void {
  const TAURI_BRIDGE_PATTERNS = [
    "reading 'invoke'",
    "reading 'transformCallback'",
    '__TAURI_INTERNALS__',
  ];

  window.addEventListener('unhandledrejection', (e) => {
    const reason = e.reason;
    const text =
      reason instanceof Error ? `${reason.message}` : typeof reason === 'string' ? reason : '';
    if (TAURI_BRIDGE_PATTERNS.some((p) => text.includes(p))) {
      e.preventDefault(); // 阻止 console 的 "Uncaught (in promise)" 报错
    }
  });
}
