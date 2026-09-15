// src/window-system/hooks/useOpenApp.ts
// "聚焦或创建"应用打开逻辑（提取自 Desktop createApp，供应用内按钮复用）
// 行为：已打开→恢复/聚焦；未打开→创建新窗口

import { useCallback } from "react";
import { useWindowManager } from "../WindowManagerContext";

export function useOpenApp() {
  const { manager } = useWindowManager();

  return useCallback(
    async (appId: string) => {
      const existing = manager.getByAppId(appId);
      if (existing.length > 0) {
        if (existing[0].minimized) {
          manager.restore(existing[0].id);
        }
        manager.focus(existing[0].id);
        return;
      }
      await manager.create(appId);
    },
    [manager]
  );
}
