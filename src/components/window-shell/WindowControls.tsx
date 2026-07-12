// src/components/window-shell/WindowControls.tsx
// 窗口控制按钮 - GNOME 风格：黄绿红圆点

import { memo } from "react";
import styles from "./WindowControls.module.css";

export interface WindowControlsProps {
  onClose: () => void;
  onMinimize: () => void;
  onMaximize: () => void;
}

/**
 * WindowControls - 窗口控制按钮
 *
 * GNOME 风格：黄绿红圆点（最小化/最大化/关闭）
 * 完全独立，不受应用样式影响
 */
export const WindowControls = memo(function WindowControls({
  onClose,
  onMinimize,
  onMaximize,
}: WindowControlsProps) {
  return (
    <div className={styles.windowControls}>
      {/* 最小化按钮 - 黄色 */}
      <button
        className={`${styles.windowControlBtn} ${styles.windowControlBtnMinimize}`}
        onClick={onMinimize}
        title="最小化"
        aria-label="最小化窗口"
      />
      {/* 最大化按钮 - 绿色 */}
      <button
        className={`${styles.windowControlBtn} ${styles.windowControlBtnMaximize}`}
        onClick={onMaximize}
        title="最大化"
        aria-label="最大化窗口"
      />
      {/* 关闭按钮 - 红色 */}
      <button
        className={`${styles.windowControlBtn} ${styles.windowControlBtnClose}`}
        onClick={onClose}
        title="关闭"
        aria-label="关闭窗口"
      />
    </div>
  );
});