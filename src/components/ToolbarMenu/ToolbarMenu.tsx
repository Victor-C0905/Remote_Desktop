/**
 * ToolbarMenu / MenuBar — 工具栏菜单组件（共享）
 *
 * - ToolbarMenu：单触发按钮（⋮ / ⌄），用于低频操作收纳
 *   （对齐 VS Code "More Actions" / Windows Terminal "⌄" 模式）
 * - MenuBar：传统菜单栏（文件/编辑/视图…），打开后悬停切换，
 *   用于按功能分类收纳全部操作（对齐 VS Code / GNOME 应用菜单栏）
 *
 * 共同特性：
 * - 菜单面板通过 createPortal 渲染到 body，fixed 定位锚定触发元素，
 *   不受工具栏 overflow 裁剪（复用 FileManager 列头菜单的验证模式）
 * - 支持勾选项（checked）、分隔线（separator）、快捷键提示（shortcut）
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

  /** 右侧快捷键提示（例如 "Ctrl+S"），仅展示不改键位 */
  shortcut?: string;

  /** 点击后保持菜单打开（用于连续切换勾选项；默认点击后关闭） */
  keepOpen?: boolean;

  /** 点击回调 */
  onClick(): void;
}

/** 分隔线 */
export interface ToolbarMenuSeparator {
  separator: true;
}

export type MenuEntry = ToolbarMenuItem | ToolbarMenuSeparator;

/* ── 内部：菜单面板（portal 渲染）────────────────────────── */

interface MenuPanelProps {
  /** 面板锚定坐标（fixed，px） */
  top: number;
  left: number;
  /** 右对齐触发按钮（ToolbarMenu 用）；MenuBar 左对齐时不传 */
  alignRight?: boolean;
  items: MenuEntry[];
  onPick: (item: ToolbarMenuItem) => void;
}

function MenuPanel({ top, left, alignRight, items, onPick }: MenuPanelProps) {
  return createPortal(
    <div
      className="tbm-menu"
      role="menu"
      style={{ top, left, ...(alignRight ? { transform: 'translateX(-100%)' } : {}) }}
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
            onClick={() => onPick(item)}
          >
            <span className="tbm-item-check">{item.checked ? '✓' : ''}</span>
            <span className="tbm-item-label">{item.label}</span>
            {item.shortcut && <span className="tbm-item-shortcut">{item.shortcut}</span>}
          </button>
        ),
      )}
    </div>,
    document.body,
  );
}

/* ── 外部点击 / Esc 关闭 Hook ────────────────────────────── */

function useDismiss(open: boolean, onClose: () => void) {
  useEffect(() => {
    if (!open) return;

    const onPointerDown = (e: MouseEvent) => {
      const target = e.target as Node;
      // 菜单面板渲染在 body 下，面板内点击不关闭（由菜单项 onClick 处理）
      if ((target as Element).closest?.('.tbm-menu')) return;
      onClose();
    };

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };

    window.addEventListener('mousedown', onPointerDown);
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('mousedown', onPointerDown);
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [open, onClose]);
}

/* ── ToolbarMenu：单触发按钮菜单 ─────────────────────────── */

export interface ToolbarMenuProps {
  /** 触发按钮内容（图标字符，如 ⋮ / ⌄） */
  icon?: string;

  /** 触发按钮的悬浮提示与 aria-label */
  title: string;

  /** 菜单内容（项或分隔线） */
  items: MenuEntry[];
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

  // 触发按钮自身点击不触发"外部点击"关闭（由按钮 onClick 切换）
  useDismiss(open, () => setOpen(false));

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
        onClick={(e) => {
          e.stopPropagation();
          setOpen((v) => !v);
        }}
      >
        {/* 字符图标（U+2304 ⌄）的字形墨迹位置随字体度量漂移，视觉上与旁边
            的窗口控制圆点无法稳定处于同一水平线；预定义图标改用 SVG mask
            渲染（与页签 +/× 按钮同款做法），几何绝对居中。其他字符保持
            文本渲染（向后兼容） */}
        {icon === '⌄' ? <span className="tbm-icon-chevron" aria-hidden="true" /> : icon}
      </button>

      {open && (
        <MenuPanel
          top={position.top}
          left={position.left}
          alignRight
          items={items}
          onPick={(item) => {
            item.onClick();
            if (!item.keepOpen) setOpen(false);
          }}
        />
      )}
    </>
  );
}

/* ── MenuBar：传统菜单栏（文件/编辑/视图…）────────────────── */

export interface MenuBarMenu {
  /** 菜单栏标签（如 "文件"） */
  label: string;

  /** 该菜单的内容（项或分隔线） */
  items: MenuEntry[];
}

/**
 * 菜单栏
 *
 * 经典桌面应用菜单栏交互：点击展开，展开状态下悬停其他标签直接切换；
 * 点击菜单项 / 外部 / Esc 收起。
 *
 * @example
 * ```tsx
 * <MenuBar
 *   menus={[
 *     { label: '文件', items: [{ label: '保存', shortcut: 'Ctrl+S', onClick: save }] },
 *     { label: '视图', items: [{ label: '自动换行', checked: wrap, keepOpen: true, onClick: toggle }] },
 *   ]}
 * />
 * ```
 */
export function MenuBar({ menus }: { menus: MenuBarMenu[] }) {
  const [openIndex, setOpenIndex] = useState<number | null>(null);
  const [position, setPosition] = useState({ top: 0, left: 0 });
  const rootRef = useRef<HTMLDivElement>(null);
  const labelRefs = useRef<Array<HTMLButtonElement | null>>([]);

  // 打开时按当前标签按钮计算坐标（面板左对齐标签左缘，菜单栏惯例）
  useLayoutEffect(() => {
    if (openIndex === null) return;
    const rect = labelRefs.current[openIndex]?.getBoundingClientRect();
    if (rect) {
      setPosition({ top: rect.bottom + 4, left: rect.left });
    }
  }, [openIndex]);

  // 外部点击 / Esc 收起（触发标签自身点击由 onClick 切换处理）
  useDismiss(openIndex !== null, () => setOpenIndex(null));

  return (
    <div className="tmb-root" ref={rootRef} role="menubar">
      {menus.map((menu, i) => (
        <button
          key={menu.label}
          ref={(el) => {
            labelRefs.current[i] = el;
          }}
          type="button"
          className={`tmb-label${openIndex === i ? ' open' : ''}`}
          aria-haspopup="menu"
          aria-expanded={openIndex === i}
          onClick={(e) => {
            e.stopPropagation();
            setOpenIndex((cur) => (cur === i ? null : i));
          }}
          // 经典菜单栏交互：已展开时悬停其他标签直接切换
          onMouseEnter={() => {
            if (openIndex !== null && openIndex !== i) setOpenIndex(i);
          }}
        >
          {menu.label}
        </button>
      ))}

      {openIndex !== null && (
        <MenuPanel
          top={position.top}
          left={position.left}
          items={menus[openIndex].items}
          onPick={(item) => {
            item.onClick();
            if (!item.keepOpen) setOpenIndex(null);
          }}
        />
      )}
    </div>
  );
}
