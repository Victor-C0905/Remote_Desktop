// src/window-system/components/Window.tsx

import { useWindowManager, useWindowState } from '../WindowManagerContext';
import { DraggableWindow } from '../../components/DraggableWindow';

/**
 * Window component - wraps DraggableWindow with window system state
 *
 * ⚠️ 注意：此组件使用 DraggableWindow（遗留窗口组件），
 * 当前 Desktop.tsx 使用 WindowShell + useWindowState 的方式渲染窗口。
 * 此组件保留作为 window-system 模块的完整导出。
 */
export function Window({ windowId }: { windowId: string }) {
  const { manager } = useWindowManager();
  const windowState = useWindowState(windowId);

  // 从 Window 实例获取不可由 useWindowState 派生的字段
  const win = manager.getById(windowId);
  const app = win ? manager.getApp(win.appId) : undefined;

  if (!windowState || !win || !app) {
    return null;
  }

  return (
    <DraggableWindow
      title={app.title}
      isActive={windowState.isActive}
      onClose={() => manager.close(windowId)}
      onMinimize={() => manager.minimize(windowId)}
      onFocus={() => manager.focus(windowId)}
      initialPosition={windowState.position}
      initialSize={windowState.size}
      minWidth={app.minSize.width}
      minHeight={app.minSize.height}
      zIndex={windowState.isActive ? 90 : 10}
    >
      {win.preloadState === 'loading' ? (
        <div>Loading...</div>
      ) : win.preloadState === 'error' ? (
        <div>Error loading data</div>
      ) : (
        <app.component windowId={windowId} preloadData={win.preloadData} />
      )}
    </DraggableWindow>
  );
}
