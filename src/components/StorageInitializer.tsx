import { useState, useEffect } from "react";
import { getSettingsStorage, getServersStorage, getNotificationsStorage } from "../utils/storage";
import { invoke } from "@tauri-apps/api/core";
import { createLogger } from "../utils/logger";

const log = createLogger('StorageInitializer');

interface StorageInitializerProps {
  children: React.ReactNode;
}

/**
 * 存储初始化组件
 * 
 * 在应用启动时预加载所有 Tauri Store，
 * 确保 Zustand persist 中间件能够正确读取数据
 */
export function StorageInitializer({ children }: StorageInitializerProps) {
  const [initialized, setInitialized] = useState(false);

  useEffect(() => {
    const initializeStorage = async () => {
      try {
        log.info('开始预加载存储...');
        
        // 预加载所有存储
        await Promise.all([
          getSettingsStorage(),
          getServersStorage(),
          getNotificationsStorage(),
        ]);

        // ── 进程纪元校验（会话镜像的生命周期闸门）──────────
        // WebView2 的 sessionStorage 会在进程重启后存活（Chromium 崩溃恢复，
        // 同 user-data + 同源即保留）——纪元不一致 = 进程已重启，清空上一
        // 世代的会话镜像（窗口快照 wm.session / 连接镜像 sm.session / 挂载
        // 代际 browser.mountGen.*），保证「关闭项目 = 干净桌面」；
        // 纪元一致（仅页面刷新）→ 镜像保留，窗口/连接/通知照常恢复。
        const epoch = await invoke<string>("app_process_epoch");
        if (sessionStorage.getItem("app.epoch") !== epoch) {
          sessionStorage.removeItem("wm.session");
          sessionStorage.removeItem("sm.session");
          const stale: string[] = [];
          for (let i = 0; i < sessionStorage.length; i++) {
            const k = sessionStorage.key(i);
            if (k?.startsWith("browser.mountGen.")) stale.push(k);
          }
          stale.forEach((k) => sessionStorage.removeItem(k));
          sessionStorage.setItem("app.epoch", epoch);
          log.info("检测到进程重启，已清空上一世代的会话镜像");
        }
        
        log.info('存储预加载完成');
        setInitialized(true);
      } catch (e) {
        log.error('存储预加载失败:', e);
        // 即使失败也继续渲染，使用默认值
        setInitialized(true);
      }
    };

    initializeStorage();
  }, []);

  // 等待存储初始化完成后再渲染
  // （预加载失败已在上方 catch 以 error 级记录，渲染体不再重复打 warn）
  // 这样 Zustand persist 中间件就能正确读取数据
  if (!initialized) {
    return null; // 或者显示加载界面
  }

  return <>{children}</>;
}