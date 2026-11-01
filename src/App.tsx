import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { Desktop } from "./shell/Desktop";
import { RootErrorBoundary } from "./shell/RootErrorBoundary";
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
    // 孤儿浏览器窗口回收（仅限无会话快照的场景）：崩溃恢复/无痕首次启动时，
    // sessionStorage 随进程消亡 → 无快照 → 前一次运行的浏览器会话成孤儿
    // （悬浮 webview 无法管理），在此回收。
    // ⚠️ 会话快照存在（wm.session 可解析）时必须整体跳过：刷新恢复流程会
    // 重建窗口并收养会话（子组件 effect 先于本 effect 执行），全量回收会把
    // 刚收养的多 tab 会话连带杀掉（webviews 2→1 实测两次）。快照存在 =
    // 窗口将被恢复并接管其会话，回收器无事可做。
    if (sessionStorage.getItem('wm.session') !== null) {
      log.info('检测到会话快照，跳过孤儿浏览器窗口回收（会话由恢复的窗口接管）');
      return;
    }
    invoke('browser_close_all_windows').catch((e) => log.error('孤儿浏览器窗口回收失败:', e));

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
    <RootErrorBoundary>
      <StorageInitializer>
        <Desktop />
      </StorageInitializer>
    </RootErrorBoundary>
  );
}

export default App;