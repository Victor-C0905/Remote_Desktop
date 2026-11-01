/**
 * ContextMenu — 共享右键菜单组件
 *
 * 与 ToolbarMenu（触发按钮下拉）不同，本组件用于「右键弹出」场景：
 * 位置由调用方给定的鼠标坐标决定，面板 portal 到 body、fixed 定位，
 * 挂载后按实测尺寸做视口越界翻转与钳制（沿用 FileManager 验证过的
 * 渲染后测量模式——菜单宽高随菜单项动态，预估值不可靠）。
 *
 * - 根元素带 data-browser-overlay：在浏览器窗口上方弹出时，BrowserApp
 *   的 MutationObserver 机制自动隐藏 child webview 让位，关闭后自动恢复
 * - 点击外部 / Esc / 选中菜单项后关闭（keepOpen 项除外）
 * - 菜单项模型与 ToolbarMenu 一致（同一视觉语言与暗色主题跟随）
 */

import { useEffect, useLayoutEffect, useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import './ContextMenu.css';

/** 菜单项：普通/勾选项 */
export interface ContextMenuItem {
  /** 显示文案 */
  label: string;

  /** 可选前导图标（ReactNode，通常传 SymbolicIcon）；带图标时不显示勾选列 */
  icon?: ReactNode;

  /** 破坏性动作项：悬停转 libadwaita destructive 红 */
  danger?: boolean;

  /** 勾选状态（显示 ✓ 与高亮），不传则为普通动作项 */
  checked?: boolean;

  /** 是否禁用 */
  disabled?: boolean;

  /** 右侧快捷键提示（例如 "Ctrl+C"），仅展示不改键位 */
  shortcut?: string;

  /** 点击后保持菜单打开（用于连续切换勾选项；默认点击后关闭） */
  keepOpen?: boolean;

  /** 点击回调 */
  onClick(): void;
}

/** 分隔线 */
export interface ContextMenuSeparator {
  separator: true;
}

export type ContextMenuEntry = ContextMenuItem | ContextMenuSeparator;

export interface ContextMenuProps {
  /** 锚定坐标（viewport 坐标，通常为鼠标 clientX/Y + 少量偏移） */
  pos: { x: number; y: number };

  /** 菜单内容（项或分隔线） */
  items: ContextMenuEntry[];

  /** 菜单关闭回调（外点 / Esc / 选中非 keepOpen 项时触发） */
  onClose(): void;
}

/** 距视口边缘最小留白（与 FileManager 钳制一致） */
const VIEWPORT_MARGIN = 8;
/** 锚点距鼠标的偏移量（两侧各 3px，翻转校正时对称回退） */
const ANCHOR_OFFSET = 3;

/**
 * 右键菜单
 *
 * @example
 * ```tsx
 * {menu && (
 *   <ContextMenu
 *     pos={{ x: menu.x, y: menu.y }}
 *     items={[
 *       { label: '打开', onClick: open },
 *       { separator: true },
 *       { label: '删除', danger: true, shortcut: 'Del', onClick: remove },
 *     ]}
 *     onClose={() => setMenu(null)}
 *   />
 * )}
 * ```
 */
export function ContextMenu({ pos, items, onClose }: ContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);

  // 挂载后按实测尺寸校正：右/下溢出视口时翻转到锚点另一侧（与 3px 锚点
  // 偏移对称），两侧都放不下时钳到视口边。useLayoutEffect 在 paint 前同步
  // 执行，校正不可见（无中间态闪烁）
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    let { x, y } = pos;
    if (x + w > window.innerWidth - VIEWPORT_MARGIN) {
      x = Math.max(VIEWPORT_MARGIN, x - w - ANCHOR_OFFSET * 2);
    }
    if (y + h > window.innerHeight - VIEWPORT_MARGIN) {
      y = Math.max(VIEWPORT_MARGIN, y - h - ANCHOR_OFFSET * 2);
    }
    el.style.left = `${x}px`;
    el.style.top = `${y}px`;
  }, [pos]);

  // 外部点击 / Esc / 失焦关闭。用 mousedown 而非 click：菜单内 mousedown 不
  // 冒泡关闭（由菜单项 onClick 处理），面板外按下即收起，交互与系统菜单一致。
  // 必须挂 capture 阶段：标题栏的 WindowShell 拖拽起始会对 mousedown 调
  // stopPropagation（冒泡到不了 window），capture 在最上游先行触发保证必达；
  // blur 关闭覆盖点击其他应用窗口的场景（对齐系统菜单行为）
  useEffect(() => {
    const onPointerDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if ((target as Element).closest?.('.ctx-menu')) return;
      onClose();
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('mousedown', onPointerDown, true);
    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('blur', onClose);
    return () => {
      window.removeEventListener('mousedown', onPointerDown, true);
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('blur', onClose);
    };
  }, [onClose]);

  return createPortal(
    <div
      ref={ref}
      className="ctx-menu"
      role="menu"
      data-browser-overlay=""
      style={{ left: pos.x, top: pos.y }}
      onClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item, i) =>
        'separator' in item ? (
          <div key={i} className="ctx-separator" />
        ) : (
          <button
            key={i}
            type="button"
            role="menuitemcheckbox"
            aria-checked={item.checked}
            className={`ctx-item${item.checked ? ' checked' : ''}${item.icon ? ' has-icon' : ''}${item.danger ? ' danger' : ''}`}
            disabled={item.disabled}
            onClick={() => {
              item.onClick();
              if (!item.keepOpen) onClose();
            }}
          >
            <span className="ctx-item-check">{item.icon ?? (item.checked ? '✓' : '')}</span>
            <span className="ctx-item-label">{item.label}</span>
            {item.shortcut && <span className="ctx-item-shortcut">{item.shortcut}</span>}
          </button>
        ),
      )}
    </div>,
    document.body,
  );
}
