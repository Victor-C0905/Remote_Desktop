// src/components/window-shell/WindowControls.tsx
// 窗口控制按钮 - Adwaita 风格：黄绿红圆点

import { memo } from "react";
import { useTranslation } from "react-i18next";
import styles from "./WindowControls.module.css";

export interface WindowControlsProps {
  onClose: () => void;
  onMinimize: () => void;
  onMaximize: () => void;
}

/**
 * WindowControls - 窗口控制按钮
 *
 * Adwaita 风格：黄绿红圆点（最小化/最大化/关闭）
 * 完全独立，不受应用样式影响
 */
export const WindowControls = memo(function WindowControls({
  onClose,
  onMinimize,
  onMaximize,
}: WindowControlsProps) {
  const { t } = useTranslation();
  return (
    // ✅ 阻止 mousedown 冒泡到 HeaderBar:避免点控制按钮时误触发标题栏拖拽(tear-off)。
    //    根因:HeaderBar 绑了 onMouseDown={handleDragStart},snap/最大化状态下 mousedown 会触发 tear-off,
    //    导致点最小化/最大化/关闭时窗口被错误还原成 snap/最大化前的尺寸。
    <div
      className={styles.windowControls}
      onMouseDown={(e) => e.stopPropagation()}
    >
      {/* 最小化按钮 - 黄色 */}
      <button
        className={`${styles.windowControlBtn} ${styles.windowControlBtnMinimize}`}
        onClick={onMinimize}
        title={t("windowShell.minimize")}
        aria-label={t("windowShell.minimizeWindow")}
      />
      {/* 最大化按钮 - 绿色 */}
      <button
        className={`${styles.windowControlBtn} ${styles.windowControlBtnMaximize}`}
        onClick={onMaximize}
        title={t("windowShell.maximize")}
        aria-label={t("windowShell.maximizeWindow")}
      />
      {/* 关闭按钮 - 红色 */}
      <button
        className={`${styles.windowControlBtn} ${styles.windowControlBtnClose}`}
        onClick={onClose}
        title={t("windowShell.close")}
        aria-label={t("windowShell.closeWindow")}
      />
    </div>
  );
});