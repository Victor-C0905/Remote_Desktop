/**
 * 远程目录选择器（系统通用组件）
 *
 * 通过 remote_read_dir 浏览服务器目录，支持筛选、面包屑、直接输入路径、
 * 新建文件夹与错误重试；选中当前目录后回调。
 * 用于保存文件、解压目标目录等场景（保存对话框 / 解压对话框）。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import { Dialog } from '../Dialog/Dialog';
import { AppIcon } from '../AppIcon';
import { formatUnknownError } from '../../types/errors';
import { createLogger } from '../../utils/logger';
import type { FileEntry } from '../../apps/fileTypes';
import { joinRemotePath, normalizeRemotePath, parentRemotePath } from './path';
import { ensureRemoteDirectory } from './remoteDirectory';
import './RemoteDirectoryPicker.css';

const log = createLogger('RemoteDirectoryPicker');

interface ReadDirResponse {
  path: string;
  entries: FileEntry[];
}

interface RemoteDirectoryPickerProps {
  serverId: string;
  /** 初始目录；默认根目录 */
  initialPath?: string;
  onSelect: (path: string) => void;
  onCancel: () => void;
}

/** 文件夹名合法性：单段名称、非 . / ..、不含 NUL */
function isValidFolderName(name: string): boolean {
  return !!name && name !== '.' && name !== '..' && !name.includes('/') && !name.includes('\0');
}

export function RemoteDirectoryPicker({
  serverId, initialPath = '/', onSelect, onCancel,
}: RemoteDirectoryPickerProps) {
  const { t } = useTranslation();
  const [path, setPath] = useState(() => normalizeRemotePath(initialPath));
  const [pathDraft, setPathDraft] = useState(() => normalizeRemotePath(initialPath));
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  /** 读取失败的路径（用于“创建此目录”）；null 表示当前无失败路径 */
  const [failedPath, setFailedPath] = useState<string | null>(null);
  const [filter, setFilter] = useState('');
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState('');
  const [creatingBusy, setCreatingBusy] = useState(false);
  const [creatingPathBusy, setCreatingPathBusy] = useState(false);
  /** 用户点“选择此目录”时草稿路径尚不可用；创建成功后直接选中 */
  const [pendingSelect, setPendingSelect] = useState(false);
  /** 用户正在编辑路径：异步 load 完成时不得覆盖其输入 */
  const draftDirtyRef = useRef(false);

  const load = useCallback(async (target: string): Promise<string | null> => {
    setLoading(true);
    setError(null);
    try {
      const resp = await invoke<ReadDirResponse>('remote_read_dir', { serverId, path: target });
      const dirs = (resp.entries ?? []).filter((e) => e.is_dir);
      setEntries(dirs);
      const resolved = normalizeRemotePath(resp.path || target);
      setPath(resolved);
      // 用户正在输入时保留草稿，避免异步加载把刚输入的内容覆盖掉
      if (!draftDirtyRef.current) setPathDraft(resolved);
      setFailedPath(null);
      return resolved;
    } catch (err) {
      log.error('读取目录失败:', target, err);
      setEntries([]);
      setError(formatUnknownError(err));
      setFailedPath(normalizeRemotePath(target));
      return null;
    } finally {
      setLoading(false);
    }
  }, [serverId]);

  // 仅首次加载；后续导航由 nav / 路径输入 / 新建文件夹触发
  useEffect(() => { void load(path); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const nav = useCallback((next: string) => {
    setFilter('');
    setPendingSelect(false);
    draftDirtyRef.current = false;
    void load(normalizeRemotePath(next));
  }, [load]);

  /** 手动输入路径：成功才更新当前目录，失败保留草稿供修正 */
  const goToDraft = useCallback(() => {
    const target = normalizeRemotePath(pathDraft);
    setFilter('');
    setPendingSelect(false);
    draftDirtyRef.current = false;
    void load(target);
  }, [pathDraft, load]);

  /**
   * 选择当前输入的目录：
   * - 与已加载路径一致 → 直接选中
   * - 草稿尚未跳转/不存在 → 先校验；存在则直接选中，不存在则给出“创建并选择”
   * 不允许静默忽略输入。
   */
  const confirmSelection = useCallback(async () => {
    const target = normalizeRemotePath(pathDraft);
    if (target === path) {
      onSelect(path);
      return;
    }
    setPendingSelect(true);
    draftDirtyRef.current = false;
    const resolved = await load(target);
    if (resolved) {
      setPendingSelect(false);
      onSelect(resolved);
    }
  }, [pathDraft, path, load, onSelect]);

  const filtered = useMemo(() => {
    const q = filter.trim().toLowerCase();
    const list = q ? entries.filter((e) => e.name.toLowerCase().includes(q)) : entries;
    return [...list].sort((a, b) => a.name.localeCompare(b.name));
  }, [entries, filter]);

  const crumbs = path.split('/').filter(Boolean);
  const nameError = newName.trim() && !isValidFolderName(newName.trim());

  const createFolder = useCallback(async () => {
    const name = newName.trim();
    if (!isValidFolderName(name)) return;
    setCreatingBusy(true);
    setError(null);
    try {
      const created = joinRemotePath(path, name);
      await ensureRemoteDirectory(serverId, created);
      setNewName('');
      setCreating(false);
      setFilter('');
      // 创建后直接进入，便于用户确认并选择该目录
      draftDirtyRef.current = false;
      await load(created);
    } catch (err) {
      log.error('新建文件夹失败:', path, name, err);
      setError(formatUnknownError(err));
    } finally {
      setCreatingBusy(false);
    }
  }, [newName, path, serverId, load]);

  /** 输入路径不存在时：统一创建（create_dir_all）后进入 */
  const createMissingPath = useCallback(async () => {
    if (!failedPath || failedPath === '/') return;
    setCreatingPathBusy(true);
    setError(null);
    try {
      const result = await ensureRemoteDirectory(serverId, failedPath);
      if (result === 'parent-missing') {
        // 拒绝 create_dir_all 一条链，先让用户定位到上级目录
        setError(t('directoryPicker.parentMissing'));
        return;
      }
      draftDirtyRef.current = false;
      const resolved = await load(failedPath);
      if (resolved && pendingSelect) {
        // 从“选择此目录”进入：创建成功后直接选中，避免再点一次
        setPendingSelect(false);
        onSelect(resolved);
      }
    } catch (err) {
      log.error('创建目录失败:', failedPath, err);
      setError(formatUnknownError(err));
    } finally {
      setCreatingPathBusy(false);
    }
  }, [failedPath, serverId, load, t, pendingSelect, onSelect]);

  return (
    <Dialog onClose={onCancel} width={520} draggable dismissOnOverlay={false}>
      <div className="dg-title">{t('directoryPicker.title')}</div>

      <div className="rdp-crumbs">
        <button className="rdp-crumb" onClick={() => nav('/')}>{t('directoryPicker.root')}</button>
        {crumbs.map((seg, i) => {
          const target = `/${crumbs.slice(0, i + 1).join('/')}`;
          return (
            <span key={target} className="rdp-crumb-item">
              <span className="rdp-sep">›</span>
              <button className="rdp-crumb" onClick={() => nav(target)}>{seg}</button>
            </span>
          );
        })}
      </div>

      <div className="rdp-path">
        <input
          className="dg-input"
          value={pathDraft}
          onKeyDown={(e) => { if (e.key === 'Enter') goToDraft(); }}
          onChange={(e) => {
            draftDirtyRef.current = true;
            setPathDraft(e.target.value);
            // 草稿变化后旧的失败路径/待选中状态不再有效
            setError(null);
            setFailedPath(null);
            setPendingSelect(false);
          }}
          placeholder={t('directoryPicker.pathPlaceholder')}
          spellCheck={false}
          aria-label={t('directoryPicker.pathPlaceholder')}
        />
        <button className="dg-btn" onClick={goToDraft} disabled={!pathDraft.trim()}>
          {t('directoryPicker.go')}
        </button>
      </div>

      <input
        className="dg-input rdp-filter"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        placeholder={t('directoryPicker.filterPlaceholder')}
      />

      <div className="rdp-list">
        {loading && <div className="rdp-hint">{t('directoryPicker.loading')}</div>}
        {!loading && error && (
          <div className="rdp-error">
            <span>{t('directoryPicker.error', { message: error })}</span>
            <div className="rdp-error-actions">
              <button className="dg-btn" onClick={() => void load(path)}>{t('common.retry')}</button>
              {failedPath && failedPath !== '/' && (
                <button
                  className="dg-btn"
                  onClick={() => void createMissingPath()}
                  disabled={creatingPathBusy}
                >
                  {creatingPathBusy
                    ? t('common.loading')
                    : pendingSelect
                      ? t('directoryPicker.createAndSelect')
                      : t('directoryPicker.createPath')}
                </button>
              )}
            </div>
          </div>
        )}
        {!loading && path !== '/' && (
          <button className="rdp-row" onClick={() => nav(parentRemotePath(path))}>
            <AppIcon name="folder" size={15} />
            <span>{t('directoryPicker.parent')}</span>
          </button>
        )}
        {!loading && !error && filtered.map((e) => (
          <button
            key={e.name}
            className="rdp-row"
            title={joinRemotePath(path, e.name)}
            onClick={() => nav(joinRemotePath(path, e.name))}
          >
            <AppIcon name="folder" size={15} />
            <span>{e.name}</span>
          </button>
        ))}
        {!loading && !error && filtered.length === 0 && entries.length > 0 && (
          <div className="rdp-hint">{t('directoryPicker.noMatch')}</div>
        )}
        {!loading && !error && entries.length === 0 && (
          <div className="rdp-hint">{t('directoryPicker.empty')}</div>
        )}
      </div>

      {creating ? (
        <div className="rdp-new">
          <input
            className="dg-input"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            placeholder={t('directoryPicker.folderName')}
            autoFocus
            onKeyDown={(e) => { if (e.key === 'Enter') void createFolder(); }}
          />
          <button
            className="dg-btn"
            onClick={() => void createFolder()}
            disabled={creatingBusy || !newName.trim() || !!nameError}
          >
            {t('directoryPicker.create')}
          </button>
          <button className="dg-btn" onClick={() => { setCreating(false); setNewName(''); }}>
            {t('common.cancel')}
          </button>
          {nameError && <div className="rdp-error">{t('directoryPicker.invalidName')}</div>}
        </div>
      ) : (
        <div className="rdp-new">
          <button className="dg-btn" onClick={() => setCreating(true)}>{t('directoryPicker.newFolder')}</button>
        </div>
      )}

      <div className="dg-actions">
        <button className="dg-btn" onClick={onCancel}>{t('common.cancel')}</button>
        <button
          className="dg-btn-primary"
          onClick={() => void confirmSelection()}
          disabled={loading || !!error}
        >
          {t('directoryPicker.selectCurrent')}
        </button>
      </div>
    </Dialog>
  );
}
