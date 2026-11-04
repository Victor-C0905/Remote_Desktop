import { useState, useEffect } from "react";
import { getSettingsStorage, getServersStorage, getNotificationsStorage, preflightStores, type StoreBlock } from "../utils/storage";
import { invoke } from "@tauri-apps/api/core";
import { createLogger } from "../utils/logger";
import { Dialog } from "../components/Dialog/Dialog";
import { useTranslation } from "react-i18next";
import { formatUnknownError } from "../types/errors";

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
  // 被保护层阻断的 store 文件：非空时不渲染主界面，必须先由用户处理
  const [blocks, setBlocks] = useState<StoreBlock[] | null>(null);

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
        
        // 磁盘契约预检：版本过高/结构损坏 → 阻断进入主界面，阻止任何写回
        const blocked = await preflightStores();
        if (blocked.length > 0) {
          setBlocks(blocked);
          return;
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

  // 数据保护优先：阻断态不允许进入主界面
  if (blocks && blocks.length > 0) {
    return <StoreRecovery blocks={blocks} />;
  }

  // 等待存储初始化完成后再渲染
  // （预加载失败已在上方 catch 以 error 级记录，渲染体不再重复打 warn）
  // 这样 Zustand persist 中间件就能正确读取数据
  if (!initialized) {
    return null; // 或者显示加载界面
  }

  return <>{children}</>;
}

/**
 * 本地数据保护对话框：
 * - 备份并重启：把损坏/高版本文件移入 backups/，随后空配置重启
 * - 退出应用：不做任何改写，保留原文件
 */
function StoreRecovery({ blocks }: { blocks: StoreBlock[] }) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const quarantineAndRestart = async () => {
    setBusy(true);
    setError(null);
    try {
      for (const block of blocks) {
        await invoke<string | null>("store_quarantine", { name: block.path });
      }
      await invoke("app_restart");
    } catch (e) {
      setError(formatUnknownError(e));
      setBusy(false);
    }
  };

  const exit = () => {
    void invoke("app_exit");
  };

  return (
    <Dialog onClose={() => {}} disableDismiss width={520}>
      <div className="dg-title">{t("storageRecovery.title")}</div>
      <div className="dg-detail">{t("storageRecovery.body")}</div>
      <div className="dg-detail">
        {blocks.map((block) => (
          <div key={block.path}>
            {block.reason === "newer-version"
              ? t("storageRecovery.newerVersion", {
                  path: block.path,
                  version: block.version ?? "?",
                  expected: block.expected ?? "?",
                })
              : t("storageRecovery.unreadable", { path: block.path })}
          </div>
        ))}
      </div>
      {error && (
        <div className="dg-detail" style={{ color: "var(--quirel-danger-fg)" }}>
          {t("storageRecovery.failed", { message: error })}
        </div>
      )}
      <div className="dg-actions">
        <button className="dg-btn" onClick={exit} disabled={busy}>
          {t("storageRecovery.exit")}
        </button>
        <button className="dg-btn-primary" onClick={quarantineAndRestart} disabled={busy}>
          {busy ? t("storageRecovery.working") : t("storageRecovery.quarantineAndRestart")}
        </button>
      </div>
    </Dialog>
  );
}