import { useState, useEffect } from "react";
import { getSettingsStorage, getServersStorage } from "../utils/storage";
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
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const initializeStorage = async () => {
      try {
        log.info('开始预加载存储...');
        
        // 预加载所有存储
        await Promise.all([
          getSettingsStorage(),
          getServersStorage(),
        ]);
        
        log.info('存储预加载完成');
        setInitialized(true);
      } catch (e) {
        log.error('存储预加载失败:', e);
        setError(e instanceof Error ? e.message : String(e));
        // 即使失败也继续渲染，使用默认值
        setInitialized(true);
      }
    };

    initializeStorage();
  }, []);

  if (error) {
    log.warn('存储加载出错，使用默认值:', error);
  }

  // 等待存储初始化完成后再渲染
  // 这样 Zustand persist 中间件就能正确读取数据
  if (!initialized) {
    return null; // 或者显示加载界面
  }

  return <>{children}</>;
}