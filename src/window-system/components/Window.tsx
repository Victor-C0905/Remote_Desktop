// src/window-system/components/Window.tsx

import { useWindowManager } from '../hooks/useWindowManager';
import { useWindowState } from '../hooks/useWindowState';
import { DraggableWindow } from '../../components/DraggableWindow';

/**
 * Window component - wraps DraggableWindow with window system state
 */
export function Window({ windowId }: { windowId: string }) {
  const manager = useWindowManager();
  const state = useWindowState(windowId);
  const app = manager.getApp(state.appId);

  if (!app) {
    return null;
  }

  return (
    <DraggableWindow
      title={state.title}
      isActive={state.focused}
      onClose={() => manager.close(windowId)}
      onMinimize={() => manager.minimize(windowId)}
      onFocus={() => manager.focus(windowId)}
      initialPosition={state.position}
      initialSize={state.size}
      minWidth={app.minSize.width}
      minHeight={app.minSize.height}
      zIndex={state.focused ? 90 : 10}
    >
      {state.preloadState === 'loading' ? (
        <div>Loading...</div>
      ) : state.preloadState === 'error' ? (
        <div>Error loading data</div>
      ) : (
        <app.component windowId={windowId} preloadData={state.preloadData} />
      )}
    </DraggableWindow>
  );
}