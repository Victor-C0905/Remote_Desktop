import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { Desktop } from "./shell/Desktop";
import { StorageInitializer } from "./components/StorageInitializer";
import { createLogger } from './utils/logger';
import "./styles/variables.css";    // ✅ 层 1：全局变量定义
import "./styles/base.css";          // ✅ 层 2：最小 Reset
import "./styles/typography.css";    // ✅ 层 3：Typography 工具类
import "./styles/scrollbar.css";     // ✅ 层 4：Scrollbar 全局样式
import "./styles/skeleton.css";      // ✅ 层 5：Skeleton 全局样式

const log = createLogger('App');

function App() {
  useEffect(() => {
    const handleBeforeUnload = (_e: BeforeUnloadEvent) => {
      // 通知后端准备关闭
      invoke('prepare_shutdown').catch((e) => log.error('prepare_shutdown 调用失败:', e));
    };

    window.addEventListener('beforeunload', handleBeforeUnload);

    return () => {
      window.removeEventListener('beforeunload', handleBeforeUnload);
    };
  }, []);

  // ── OS 最小化闸门（Tier1）────────────────────────────────────
  // 透明窗口破坏 WebView2 遮挡检测：最小化后页面仍被判定"可见"而持续渲染。
  // Rust 侧探测真实最小化状态发出 app-minimize-changed，此处切根元素 class，
  // CSS 侧（base.css）暂停全部动画并关闭毛玻璃滤镜（backdrop-filter 常驻 GPU 大头）
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    listen<boolean>('app-minimize-changed', (event) => {
      if (cancelled) return;
      document.documentElement.classList.toggle('os-minimized', event.payload === true);
    }).then((fn) => {
      // 竞态防护：监听建立前组件已卸载，立即注销
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return (
    <StorageInitializer>
      <Desktop />
    </StorageInitializer>
  );
}

export default App;