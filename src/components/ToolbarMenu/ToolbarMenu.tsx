/**
 * ToolbarMenu — 工具栏下拉菜单（共享组件）
 *
 * 现代编辑器布局的低频操作收纳点（对齐 VS Code "More Actions" /
 * Windows Terminal "⌄" 模式）：触发按钮 + portal 渲染的菜单面板。
 *
 * 特性：
 * - 菜单通过 createPortal 渲染到 body，fixed 定位锚定触发按钮，
 *   不受工具栏 overflow 裁剪（复用 FileManager 列头菜单的验证模式）
 * - 支持勾选项（checked）与分隔线（separator）
 * - 点击外部 / Esc / 点击菜单项后关闭
 */

import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import './ToolbarMenu.css';

/** 菜单项：普通/勾选项 */
export interface ToolbarMenuItem {
  /** 显示文案 */
  label: string;

  /** 勾选状态（显示 ✓ 与高亮），不传则为普通动作项 */
  checked?: boolean;

  /** 是否禁用 */
  disabled?: boolean;

  /** 点击后保持菜单打开（用于连续切换勾选项；默认点击后关闭） */
  keepOpen?: boolean;

  /** 点击回调 */
  onClick(): void;
}

/** 分隔线 */
export interface ToolbarMenuSeparator {
  separator: true;
}

export interface ToolbarMenuProps {
  /** 触发按钮内容（图标字符，如 ⋮ / ⌄） */
  icon?: string;

  /** 触发按钮的悬浮提示与 aria-label */
  title: string;

  /** 菜单内容（项或分隔线） */
  items: Array<ToolbarMenuItem | ToolbarMenuSeparator>;
}

/**
 * 工具栏下拉菜单
 *
 * @example
 * ```tsx
 * <ToolbarMenu
 *   title="更多操作"
 *   items={[
 *     { label: '另存为…', onClick: openSaveAs },
 *     { separator: true },
 *     { label: '自动换行', checked: wrap, keepOpen: true, onClick: toggleWrap },
 *   ]}
 * />
 * ```
 */
export function ToolbarMenu({ icon = '⋮', title, items }: ToolbarMenuProps) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ top: 0, left: 0 });
  const triggerRef = useRef<HTMLButtonElement>(null);

  // 打开时按触发按钮计算 fixed 坐标（面板出现在按钮正下方，右对齐）
  useLayoutEffect(() => {
    if (!open) return;
    const rect = triggerRef.current?.getBoundingClientRect();
    if (rect) {
      setPosition({ top: rect.bottom + 4, left: rect.right });
    }
  }, [open]);

  // 外部点击 / Esc 关闭
  useEffect(() => {
    if (!open) return;

    const onPointerDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (triggerRef.current?.contains(target)) return;
      // 菜单面板渲染在 body 下，也在"外部"判定内自行处理（面板点击由 onClick 处理）
      if ((target as Element).closest?.('.tbm-menu')) return;
      setOpen(false);
    };

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };

    window.addEventListener('mousedown', onPointerDown);
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('mousedown', onPointerDown);
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [open]);

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className={`tbm-trigger${open ? ' open' : ''}`}
        aria-label={title}
        aria-haspopup="menu"
        aria-expanded={open}
        title={title}
        onClick={() => setOpen((v) => !v)}
      >
        {icon}
      </button>

      {open &&
        createPortal(
          <div
            className="tbm-menu"
            role="menu"
            style={{ top: position.top, left: position.left }}
            onClick={(e) => e.stopPropagation()}
          >
            {items.map((item, i) =>
              'separator' in item ? (
                <div key={i} className="tbm-separator" />
              ) : (
                <button
                  key={i}
                  type="button"
                  role="menuitemcheckbox"
                  aria-checked={item.checked}
                  className={`tbm-item${item.checked ? ' checked' : ''}`}
                  disabled={item.disabled}
                  onClick={() => {
                    item.onClick();
                    if (!item.keepOpen) setOpen(false);
                  }}
                >
                  <span className="tbm-item-check">{item.checked ? '✓' : ''}</span>
                  <span className="tbm-item-label">{item.label}</span>
                </button>
              ),
            )}
          </div>,
          document.body,
        )}
    </>
  );
}
