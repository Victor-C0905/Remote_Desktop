/**
 * 宿主页面刷新守卫。
 *
 * 背景：壳页面（Quirel 宿主 webview）没有正当的用户刷新场景，而刷新会
 * 重启整个 React 树——§5.1 的三次会话丢失事故均由「刷新引爆 cleanup」
 * 触发。WebView2 层已同步关闭 AreBrowserAcceleratorKeysEnabled
 * （src-tauri lib.rs / webview_host.rs），本模块在键盘路径兜底：
 * 拦截 F5 / Ctrl+R / Ctrl+Shift+R / Meta+R 的默认行为。
 * dev 构建不安装（开发调试需要刷新）；受控重载入口保留在
 * RootErrorBoundary 的「重置界面」按钮（程序化 reload 不经此拦截）。
 */

/** 判定是否为宿主页面刷新类按键（纯函数，便于测试） */
export function isUiRefreshKey(e: {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}): boolean {
  const k = e.key.toLowerCase();
  if (k === "f5") return true;
  if ((e.ctrlKey || e.metaKey) && k === "r") return true;
  return false;
}

/** 全局安装键盘拦截（capture 阶段，先于页面内任何 handler） */
export function installUiRefreshGuard(target: Window = window): void {
  target.addEventListener(
    "keydown",
    (e) => {
      if (isUiRefreshKey(e)) {
        e.preventDefault();
        e.stopPropagation();
      }
    },
    { capture: true },
  );
}
