// src/components/window-shell/WindowControls.tsx
// 窗口控制按钮 - GNOME 风格：黄绿红圆点

import './window-controls.css';

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
export function WindowControls({ onClose, onMinimize, onMaximize }: WindowControlsProps) {
  return (
    <div className="window-controls">
      <button className="window-control-btn minimize" onClick={onMinimize} title="最小化" />
      <button className="window-control-btn maximize" onClick={onMaximize} title="最大化" />
      <button className="window-control-btn close" onClick={onClose} title="关闭" />
    </div>
  );
}