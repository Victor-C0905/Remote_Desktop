// src/apps/useFileTransfer.ts
// 文件管理器拖拽移动/复制与应用内剪贴板：clip / 放置高亮 / 重名与防呆确认 /
// 批量执行器（move/copy 共用，协议单文件语义 → 批量顺序执行）。
//
// ⚠️ 内部拖拽为指针式实现（mousedown → document mousemove/mouseup），不用
// HTML5 DnD：Windows 上 Tauri 替换了 WebView2 的放置目标（OS 文件拖入上传
// 的 onDragDropEvent 依赖它），HTML5 dragstart/drop 在真实环境中不会触发
// （tauri WebviewWindowBuilder::disable_drag_drop_handler 文档原文：required
// to use HTML5 drag and drop APIs ... on Windows）。与 WindowShell 窗口拖拽、
// 框选 marquee 同一模式；放置目标以 data-drop-dir 属性标记。

import { useCallback, useRef, useState, type Dispatch, type SetStateAction } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import { buildMoveTargets, entryName } from './fileSelection';
import { assessTransferRisk, type GuardFinding } from './fileGuard';
import type { FileEntry } from './fileTypes';
import { useNotificationStore } from '../stores/notificationStore';
import { createLogger } from '../utils/logger';

const log = createLogger('FileManager');

/** 应用内剪贴板（会话内有效，跨窗口不共享） */
export interface FileClip {
  mode: 'copy' | 'cut';
  paths: string[];
}

/** 单个移动/复制目标（协议单文件语义） */
export interface TransferTarget {
  src: string;
  dst: string;
}

/** 重名确认弹窗状态：待执行目标 + 模式 + 冲突清单 */
export interface TransferConfirmState {
  mode: 'move' | 'copy';
  targets: TransferTarget[];
  conflicts: string[];
}

/** 面包屑弹簧加载驻留时长（ms）：Explorer/Chrome 同量级，短了误触、长了迟钝 */
const SPRING_LOAD_DELAY_MS = 700;
/** 点击/拖动判别阈值平方（6px，Explorer 惯例） */
const DRAG_THRESHOLD_SQ = 36;

export function useFileTransfer(params: {
  activeServerId: string | null;
  currentPath: string;
  /** 起拖时的选区：源行在选区内则携带整个选区 */
  selected: string[];
  refreshAfterMutation: () => void;
  /** 服务器确认后的逐项结果补丁：移动移除源、目标新增/覆盖（entry 由服务器回传） */
  applyTransferResult: (mode: 'move' | 'copy', src: string, dst: string, entry: FileEntry) => void;
  entryFullPath: (entry: FileEntry) => string;
  /** 弹簧加载切目录用 */
  navigateTo: (path: string) => Promise<boolean>;
  /** 正在内联重命名的条目（重命名期间不发起拖拽） */
  editingEntry: FileEntry | null;
  setSelected: Dispatch<SetStateAction<string[]>>;
  setAnchorPath: Dispatch<SetStateAction<string | null>>;
}) {
  const { activeServerId, currentPath, selected, refreshAfterMutation, applyTransferResult, entryFullPath, navigateTo, editingEntry, setSelected, setAnchorPath } = params;
  const { t } = useTranslation();
  const pushNotification = useNotificationStore((s) => s.pushNotification);

  // clip：应用内剪贴板（会话内有效，跨窗口不共享）；cut 项在列表中淡化显示
  const [clip, setClip] = useState<FileClip | null>(null);
  // 当前悬停高亮的放置目标（文件夹行 / 面包屑分段 / 侧边栏项路径）
  const [dropTargetPath, setDropTargetPath] = useState<string | null>(null);
  // 移动/粘贴重名确认：待执行目标 + 模式 + 冲突清单
  const [transferConfirm, setTransferConfirm] = useState<TransferConfirmState | null>(null);
  // 防呆确认：触及系统/敏感目录时先于重名确认弹出
  const [riskConfirm, setRiskConfirm] = useState<{ mode: 'move' | 'copy'; targets: TransferTarget[]; findings: GuardFinding[] } | null>(null);

  // 拖拽载荷（拖拽发起时确定，落放时消费）
  const dragPathsRef = useRef<string[]>([]);
  // 拖拽幽灵：携带原始路径（FM 侧按路径解析出真实图标与名称渲染"拿起"效果）
  const [dragGhost, setDragGhost] = useState<{ count: number; paths: string[] } | null>(null);
  const [ghostPos, setGhostPos] = useState({ x: 0, y: 0 });
  // 指针拖拽状态机
  const candidateRef = useRef<{ x0: number; y0: number; paths: string[]; rowPath: string; inSelection: boolean } | null>(null);
  const draggingRef = useRef(false);
  const pointRef = useRef({ x: 0, y: 0 });
  const rafRef = useRef<number | null>(null);
  // 面包屑弹簧加载计时
  const springRef = useRef<{ timer: number | null; path: string | null }>({ timer: null, path: null });

  const clearSpring = useCallback(() => {
    if (springRef.current.timer !== null) clearTimeout(springRef.current.timer);
    springRef.current = { timer: null, path: null };
  }, []);

  /** 批量执行 move/copy：顺序执行（协议单文件语义），逐项收集失败后统一刷新通知。
   *  成功通知静默入中心（不弹预览条），body 含源/目的目录，details 逐条
   *  「src → dst」供点击展开；move 成功项附「撤销」动作（原路移回），
   *  copy 不附（撤销=删除副本，反而危险） */
  const execTransfer = useCallback(async (mode: 'move' | 'copy', targets: TransferTarget[]) => {
    if (!activeServerId || targets.length === 0) return;
    const failed: string[] = [];
    const donePairs: Array<{ src: string; dst: string; entry: FileEntry | null }> = [];
    for (const target of targets) {
      try {
        const entry = await invoke<FileEntry | null>(mode === 'move' ? 'remote_move' : 'remote_copy', {
          serverId: activeServerId, src: target.src, dst: target.dst,
        });
        donePairs.push({ src: target.src, dst: target.dst, entry });
      } catch (err) {
        log.error(`${mode} 失败:`, err);
        failed.push(entryName(target.src));
      }
    }
    // 全部成功项都带回权威条目 → 逐项打补丁；任一缺失（旧版 Agent）→ 回退一次全量刷新
    if (donePairs.length > 0) {
      if (donePairs.every((p) => p.entry)) {
        for (const p of donePairs) {
          if (p.entry) applyTransferResult(mode, p.src, p.dst, p.entry);
        }
      } else {
        refreshAfterMutation();
      }
    }
    const done = targets.length - failed.length;
    if (failed.length > 0) {
      pushNotification({
        title: t('common.error'),
        body: t('fileManager.transferFailedBatch', { n: failed.length, op: mode === 'move' ? t('fileManager.opMove') : t('fileManager.opCopy'), names: failed.slice(0, 3).join('、') }),
        source: t('fileManager.source'),
        urgency: 'normal',
      });
      return;
    }
    const srcDir = targets[0].src.slice(0, targets[0].src.lastIndexOf('/')) || '/';
    const dstDir = targets[0].dst.slice(0, targets[0].dst.lastIndexOf('/')) || '/';
    pushNotification({
      title: t('fileManager.source'),
      body: mode === 'move'
        ? t('fileManager.movedDoneFromTo', { n: done, src: srcDir, dst: dstDir })
        : t('fileManager.copiedDoneFromTo', { n: done, src: srcDir, dst: dstDir }),
      source: t('fileManager.source'),
      urgency: 'normal',
      toast: false,
      details: donePairs.map((p) => `${p.src} → ${p.dst}`),
      ...(mode === 'move' && donePairs.length > 0
        ? {
            onAction: {
              label: t('fileManager.undoMove'),
              run: () => {
                void (async () => {
                  const undoFailed: string[] = [];
                  const undoPairs: Array<{ src: string; dst: string; entry: FileEntry | null }> = [];
                  for (const p of [...donePairs].reverse()) {
                    try {
                      const entry = await invoke<FileEntry | null>('remote_move', { serverId: activeServerId, src: p.dst, dst: p.src });
                      undoPairs.push({ src: p.dst, dst: p.src, entry });
                    } catch (err) {
                      log.error('撤销移动失败:', err);
                      undoFailed.push(entryName(p.dst));
                    }
                  }
                  // 与正向移动同口径：条目齐全则增量回滚，否则回退全量刷新
                  if (undoPairs.length > 0) {
                    if (undoPairs.every((p) => p.entry)) {
                      for (const p of undoPairs) {
                        if (p.entry) applyTransferResult('move', p.src, p.dst, p.entry);
                      }
                    } else {
                      refreshAfterMutation();
                    }
                  }
                  if (undoFailed.length > 0) {
                    pushNotification({
                      title: t('common.error'),
                      body: t('fileManager.undoFailed', { n: undoFailed.length, names: undoFailed.slice(0, 3).join('、') }),
                      source: t('fileManager.source'),
                      urgency: 'normal',
                    });
                  }
                })();
              },
            },
          }
        : {}),
    });
  }, [activeServerId, refreshAfterMutation, applyTransferResult, t]);

  /** 重名预检 + 执行（防呆确认通过后走这里） */
  const checkConflictsAndExec = useCallback(async (mode: 'move' | 'copy', targets: TransferTarget[]) => {
    if (!activeServerId || targets.length === 0) return;
    const conflicts: string[] = [];
    for (const target of targets) {
      try {
        const info = await invoke<{ exists: boolean }>('check_file_exists', { serverId: activeServerId, path: target.dst });
        if (info.exists) conflicts.push(target.dst);
      } catch {
        // 探测失败不阻断，交给执行层报错
      }
    }
    if (conflicts.length > 0) setTransferConfirm({ mode, targets, conflicts });
    else void execTransfer(mode, targets);
  }, [activeServerId, execTransfer]);

  /** 防呆预检 → 重名预检 → 执行：dstDir 供风险评估（src/dst 触及系统或敏感目录即拦截确认） */
  const transferWithConfirm = useCallback(async (mode: 'move' | 'copy', targets: TransferTarget[], dstDir: string) => {
    if (!activeServerId || targets.length === 0) return;
    const findings = assessTransferRisk(targets.map((t) => t.src), dstDir);
    if (findings.length > 0) {
      setRiskConfirm({ mode, targets, findings });
      return;
    }
    void checkConflictsAndExec(mode, targets);
  }, [activeServerId, checkConflictsAndExec]);

  /** 粘贴：copy → remote_copy 批量；cut → remote_move（即移动）批量并消费剪贴板 */
  const handlePaste = useCallback(() => {
    if (!clip) return;
    const { mode, paths } = clip;
    if (mode === 'cut') setClip(null);
    void transferWithConfirm(mode === 'cut' ? 'move' : 'copy', buildMoveTargets(paths, currentPath), currentPath);
  }, [clip, currentPath, transferWithConfirm]);

  /** 从载荷放入 dirPath（清空载荷、过滤原地自放）；返回是否发起 */
  const dropPathsTo = useCallback((dirPath: string, mode: 'move' | 'copy'): boolean => {
    const paths = dragPathsRef.current;
    dragPathsRef.current = [];
    setDropTargetPath(null);
    const targets = buildMoveTargets(paths, dirPath).filter((t) => t.src !== t.dst);
    if (targets.length > 0) {
      void transferWithConfirm(mode, targets, dirPath);
      return true;
    }
    return false;
  }, [transferWithConfirm]);

  // ── 指针式内部拖拽（mousedown → document mousemove/mouseup）─────────

  /** 光标处的放置目标解析：命中 data-drop-dir 元素返回其路径；
   *  未命中但在内容区空白（不在任何行上）→ inBlank（放入当前目录） */
  const resolveDrop = (x: number, y: number): { dir: string | null; inBlank: boolean } => {
    const el = document.elementFromPoint(x, y) as HTMLElement | null;
    const dir = el?.closest<HTMLElement>('[data-drop-dir]')?.dataset.dropDir ?? null;
    if (dir) return { dir, inBlank: false };
    const inContent = !!el?.closest('.fm-list-content, .fm-grid, .fm-empty');
    const onRow = !!el?.closest('.fm-list-row, .fm-grid-item, .fm-sidebar-item');
    return { dir: null, inBlank: inContent && !onRow };
  };

  /** 悬停路由：更新高亮 + 面包屑弹簧加载计时（非法目标不弹簧） */
  const applyHover = useCallback((x: number, y: number) => {
    const el = document.elementFromPoint(x, y) as HTMLElement | null;
    const dir = el?.closest<HTMLElement>('[data-drop-dir]')?.dataset.dropDir ?? null;
    setDropTargetPath((cur) => (cur === dir ? cur : dir));

    const springEl = el?.closest<HTMLElement>('[data-drop-dir][data-spring]');
    const sp = springEl?.dataset.dropDir ?? null;
    if (sp && sp !== currentPath) {
      if (springRef.current.path !== sp) {
        clearSpring();
        springRef.current = {
          path: sp,
          timer: window.setTimeout(() => {
            springRef.current.timer = null;
            void navigateTo(sp).then(() => setDropTargetPath(null));
          }, SPRING_LOAD_DELAY_MS),
        };
      }
    } else if (springRef.current.path !== null) {
      clearSpring();
    }
  }, [currentPath, clearSpring, navigateTo]);

  /** 行 mousedown：记录拖拽候选（源行在选区内携带整个选区）；
   *  超过 6px 阈值才真正进入拖拽（点击判别），松开时按落点执行移动/复制 */
  const handleRowMouseDown = useCallback((e: React.MouseEvent, entry: FileEntry) => {
    if (e.button !== 0 || editingEntry) return;
    const target = e.target as HTMLElement;
    if (target.closest('input, button')) return;

    const rowPath = entryFullPath(entry);
    const inSelection = selected.includes(rowPath);
    const paths = inSelection ? [...selected] : [rowPath];
    dragPathsRef.current = paths;
    candidateRef.current = { x0: e.clientX, y0: e.clientY, paths, rowPath, inSelection };
    pointRef.current = { x: e.clientX, y: e.clientY };

    const onMove = (ev: MouseEvent) => {
      pointRef.current = { x: ev.clientX, y: ev.clientY };
      if (rafRef.current !== null) return;
      rafRef.current = requestAnimationFrame(() => {
        rafRef.current = null;
        const cand = candidateRef.current;
        if (!cand) return;
        if (!draggingRef.current) {
          const dx = pointRef.current.x - cand.x0;
          const dy = pointRef.current.y - cand.y0;
          if (dx * dx + dy * dy <= DRAG_THRESHOLD_SQ) return;
          draggingRef.current = true;
          // Explorer 行为：拖动未选中行 = 单选该行并拖它
          if (!cand.inSelection) {
            setSelected([cand.rowPath]);
            setAnchorPath(cand.rowPath);
          }
          setGhostPos({ x: pointRef.current.x, y: pointRef.current.y });
          setDragGhost({ count: cand.paths.length, paths: cand.paths });
          // 全窗 grabbing 光标（落放/取消时移除）
          document.body.dataset.dragCursor = '1';
          return;
        }
        setGhostPos({ x: pointRef.current.x, y: pointRef.current.y });
        applyHover(pointRef.current.x, pointRef.current.y);
      });
    };

    const finish = (perform: boolean, ev: MouseEvent | null) => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      window.removeEventListener('keydown', onKey);
      if (rafRef.current !== null) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
      clearSpring();
      const wasDragging = draggingRef.current;
      draggingRef.current = false;
      candidateRef.current = null;
      setDragGhost(null);
      delete document.body.dataset.dragCursor;
      setDropTargetPath(null);
      if (!perform || !wasDragging || !ev) {
        dragPathsRef.current = [];
        return;
      }
      // 吞掉落点元素随后收到的 click（拖到文件夹上松开不应触发导航/选中）
      const swallow = (ce: MouseEvent) => {
        ce.stopPropagation();
        ce.preventDefault();
        window.removeEventListener('click', swallow, true);
      };
      window.addEventListener('click', swallow, true);
      window.setTimeout(() => window.removeEventListener('click', swallow, true), 0);

      const { dir, inBlank } = resolveDrop(ev.clientX, ev.clientY);
      // Ctrl/⌘ 落放 = 复制（Explorer 惯例），否则移动
      const mode: 'move' | 'copy' = ev.ctrlKey || ev.metaKey ? 'copy' : 'move';
      const targetDir = dir ?? (inBlank ? currentPath : null);
      if (!targetDir || !dropPathsTo(targetDir, mode)) {
        dragPathsRef.current = [];
      }
    };
    const onUp = (ev: MouseEvent) => finish(true, ev);
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === 'Escape') finish(false, null);
    };

    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
    window.addEventListener('keydown', onKey);
  }, [editingEntry, selected, entryFullPath, setSelected, setAnchorPath, currentPath, dropPathsTo, applyHover, clearSpring]);

  return {
    clip, setClip,
    dropTargetPath,
    transferConfirm, setTransferConfirm,
    riskConfirm, setRiskConfirm,
    checkConflictsAndExec,
    execTransfer,
    handlePaste,
    dragGhost, ghostPos,
    handleRowMouseDown,
  };
}
