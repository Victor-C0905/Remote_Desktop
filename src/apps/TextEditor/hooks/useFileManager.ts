/**
 * 文件管理 Hook
 *
 * 管理文件状态的核心 Hook，负责：
 * - 文件读取（通过 Tauri API）
 * - 文件保存（通过 Tauri API）
 * - 文件关闭
 * - 校验和计算（SHA-256）
 * - 错误处理和加载状态管理
 */

import { useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { FileState, FileChange, ApplyDiffResponse } from '../types/editor';
import { calculateDiff } from '../utils/diff';

/**
 * Tauri API 返回的文件读取结果
 *
 * 注意：当前 Agent 协议不返回 mtime，后续 Task 2.2 会扩展
 */
interface ReadFileResult {
  path: string;
  content: string;
  size: number;
}

/**
 * Tauri API 返回的文件写入结果
 *
 * 注意：当前 Agent 协议不返回 mtime，后续 Task 2.2 会扩展
 */
interface WriteFileResult {
  path: string;
  size: number;
}

/**
 * 文件管理器返回值
 */
interface UseFileManagerReturn {
  /** 当前打开的文件状态 */
  fileState: FileState | null;

  /** 是否正在加载文件 */
  isLoading: boolean;

  /** 是否正在保存文件 */
  isSaving: boolean;

  /** 错误信息（如果有） */
  error: string | null;

  /** 本地缓存的内容（用于新文件） */
  localContent: string;

  /** 打开文件 */
  openFile: (serverId: string, path: string) => Promise<void>;

  /** 保存文件（自动选择差异同步或全量保存） */
  saveFile: (serverId: string, useDiffSync?: boolean) => Promise<void>;

  /** 使用差异同步保存（强制使用差异同步） */
  saveWithDiff: (serverId: string) => Promise<void>;

  /** 保存到指定路径（用于新文件） */
  saveAs: (serverId: string, path: string) => Promise<void>;

  /** 关闭文件 */
  closeFile: () => void;

  /** 更新文件内容（本地编辑时调用） */
  updateContent: (newContent: string) => void;

  /** 检查文件是否有未保存的变更 */
  hasUnsavedChanges: () => boolean;

  /** 清除错误 */
  clearError: () => void;
}

/**
 * 计算内容的 SHA-256 校验和
 *
 * @param content - 要计算校验和的内容
 * @returns SHA-256 哈希值（十六进制字符串）
 */
async function calculateChecksum(content: string): Promise<string> {
  // 使用 Web Crypto API 计算 SHA-256
  const encoder = new TextEncoder();
  const data = encoder.encode(content);
  const hashBuffer = await crypto.subtle.digest('SHA-256', data);

  // 将 ArrayBuffer 转换为十六进制字符串
  const hashArray = Array.from(new Uint8Array(hashBuffer));
  const hashHex = hashArray.map(b => b.toString(16).padStart(2, '0')).join('');

  return hashHex;
}

/**
 * 文件管理 Hook
 *
 * 管理文件的读取、保存、关闭等操作，维护文件状态和加载状态
 *
 * @returns 文件管理器返回值
 *
 * @example
 * ```tsx
 * const FileManager = () => {
 *   const {
 *     fileState,
 *     isLoading,
 *     isSaving,
 *     error,
 *     openFile,
 *     saveFile,
 *     closeFile,
 *     updateContent,
 *     hasUnsavedChanges,
 *   } = useFileManager();
 *
 *   const handleOpen = async () => {
 *     await openFile('server-1', '/path/to/file.txt');
 *   };
 *
 *   const handleSave = async () => {
 *     await saveFile('server-1');
 *   };
 *
 *   return (
 *     <div>
 *       {isLoading && <p>加载中...</p>}
 *       {error && <p>错误: {error}</p>}
 *       {fileState && (
 *         <textarea
 *           value={fileState.fileType === 'text' ? fileState.content : ''}
 *           onChange={(e) => updateContent(e.target.value)}
 *         />
 *       )}
 *       <button onClick={handleSave} disabled={!hasUnsavedChanges()}>
 *         保存
 *       </button>
 *     </div>
 *   );
 * };
 * ```
 */
export function useFileManager(): UseFileManagerReturn {
  // 当前文件状态
  const [fileState, setFileState] = useState<FileState | null>(null);

  // 加载状态
  const [isLoading, setIsLoading] = useState(false);

  // 保存状态
  const [isSaving, setIsSaving] = useState(false);

  // 错误状态
  const [error, setError] = useState<string | null>(null);

  // 原始校验和（用于检测是否有未保存的变更）
  const [originalChecksum, setOriginalChecksum] = useState<string>('');

  // 本地缓存的内容（用于新文件）
  const [localContent, setLocalContent] = useState<string>('');

  /**
   * 打开文件
   *
   * 通过 Tauri API 从远程服务器读取文件
   *
   * @param serverId - 服务器 ID
   * @param path - 文件的绝对路径
   */
  const openFile = useCallback(async (serverId: string, path: string): Promise<void> => {
    // 检查 serverId 是否为空
    if (!serverId) {
      setError('未连接到服务器');
      return;
    }

    // 清除之前的错误
    setError(null);
    setIsLoading(true);

    try {
      // 设置为加载状态
      setFileState({
        path,
        mtime: 0, // 当前 API 不返回 mtime，后续 Task 2.2 会扩展
        checksum: '',
        isLocked: false,
        fileType: 'loading',
      });

      // 调用 Tauri API 读取远程文件
      const result = await invoke<ReadFileResult>('remote_read_file', {
        serverId,
        path,
      });

      // 计算校验和
      const checksum = await calculateChecksum(result.content);

      // 创建文件状态
      const newFileState: FileState = {
        path: result.path,
        mtime: 0, // 当前 API 不返回 mtime，后续 Task 2.2 会扩展
        checksum,
        isLocked: false,
        fileType: 'text', // 当前只支持文本文件，后续会添加二进制文件支持
        content: result.content,
      };

      setFileState(newFileState);
      setOriginalChecksum(checksum);
    } catch (err) {
      // 处理错误
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(`无法打开文件 "${path}": ${errorMessage}`);
      setFileState(null);
      setOriginalChecksum('');
    } finally {
      setIsLoading(false);
    }
  }, []);

  /**
   * 保存文件（自动选择差异同步或全量保存）
   *
   * 自动选择策略：
   * - 新文件：全量保存
   * - 小文件（<1KB）：全量保存（差异同步优势不明显）
   * - 大文件（>=1KB）：差异同步（节省 90%+ 流量）
   *
   * @param serverId - 服务器 ID
   * @param useDiffSync - 是否强制使用差异同步（默认自动选择）
   */
  const saveFile = useCallback(async (serverId: string, useDiffSync?: boolean): Promise<void> => {
    // 检查 serverId 是否为空
    if (!serverId) {
      setError('未连接到服务器');
      return;
    }

    // 检查是否有打开的文件
    if (!fileState) {
      setError('没有打开的文件');
      return;
    }

    // 检查文件类型
    if (fileState.fileType !== 'text') {
      setError('只支持保存文本文件');
      return;
    }

    // 自动选择保存策略
    const content = fileState.content || '';
    const shouldUseDiffSync = useDiffSync ?? (content.length >= 1024);

    if (shouldUseDiffSync && fileState.path) {
      // 差异同步保存
      await saveWithDiffInternal(serverId);
    } else {
      // 全量保存
      await saveFileFull(serverId);
    }
  }, [fileState]);

  /**
   * 使用差异同步保存文件（内部函数）
   *
   * 流程：
   * 1. 客户端计算差异（本地，不占用网络）
   * 2. 发送差异到 Agent（只传输几十字节）
   * 3. Agent 应用差异 + mtime 验证
   * 4. 返回新的 mtime
   */
  const saveWithDiffInternal = useCallback(async (serverId: string): Promise<void> => {
    if (!fileState || fileState.fileType !== 'text') {
      setError('无法保存：文件未打开或不是文本文件');
      return;
    }

    setError(null);
    setIsSaving(true);

    try {
      console.log('[saveWithDiff] 开始差异同步保存:', fileState.path);

      // 步骤 1：客户端计算差异
      const oldContent = fileState.content;
      const newContent = fileState.content; // 当前内容

      const diffs = calculateDiff(oldContent, newContent);

      console.log('[saveWithDiff] 计算差异完成:', {
        diffCount: diffs.length,
        oldLines: oldContent.split('\n').length,
        newLines: newContent.split('\n').length,
      });

      // 步骤 2：发送差异到 Agent
      const response = await invoke<ApplyDiffResponse>('remote_apply_diff', {
        serverId,
        path: fileState.path,
        baseMtime: fileState.mtime,
        diffs,
      });

      // 步骤 3：处理响应
      if (response.success) {
        console.log('[saveWithDiff] 保存成功:', response);

        // 重新计算校验和
        const newChecksum = await calculateChecksum(newContent);

        // 更新文件状态
        setFileState(prev => {
          if (!prev || prev.fileType !== 'text') return prev;

          return {
            ...prev,
            content: newContent,
            mtime: response.new_mtime,
            checksum: newChecksum,
          };
        });

        setOriginalChecksum(newChecksum);
      } else {
        // 版本冲突或其他错误
        const errorMsg = response.error || '未知错误';
        console.error('[saveWithDiff] 保存失败:', errorMsg);

        if (errorMsg.includes('版本冲突')) {
          setError('文件已被其他程序修改，请重新打开文件后再编辑。');
        } else {
          setError(`保存失败：${errorMsg}`);
        }
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      console.error('[saveWithDiff] 保存失败:', err);
      setError(`无法保存文件 "${fileState.path}": ${errorMessage}`);
    } finally {
      setIsSaving(false);
    }
  }, [fileState]);

  /**
   * 全量保存文件（传统方式）
   *
   * 适用于：新文件、小文件
   */
  const saveFileFull = useCallback(async (serverId: string): Promise<void> => {
    if (!fileState || fileState.fileType !== 'text') {
      setError('无法保存：文件未打开或不是文本文件');
      return;
    }

    setError(null);
    setIsSaving(true);

    try {
      console.log('[saveFileFull] 开始全量保存:', fileState.path);

      const result = await invoke<WriteFileResult>('remote_write_file', {
        serverId,
        path: fileState.path,
        content: fileState.content,
      });

      console.log('[saveFileFull] 保存成功:', result);

      // 重新计算校验和
      const newChecksum = await calculateChecksum(fileState.content);

      // 更新文件状态
      setFileState(prev => {
        if (!prev || prev.fileType !== 'text') return prev;

        return {
          ...prev,
          path: result.path,
          checksum: newChecksum,
          mtime: 0, // 当前 API 不返回 mtime
        };
      });

      setOriginalChecksum(newChecksum);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      console.error('[saveFileFull] 保存失败:', err);
      setError(`无法保存文件 "${fileState.path}": ${errorMessage}`);
    } finally {
      setIsSaving(false);
    }
  }, [fileState]);

  /**
   * 使用差异同步保存（公开接口，供外部调用）
   *
   * @param serverId - 服务器 ID
   */
  const saveWithDiff = useCallback(async (serverId: string): Promise<void> => {
    await saveWithDiffInternal(serverId);
  }, [saveWithDiffInternal]);

  /**
   * 保存到指定路径（用于新文件）
   *
   * 通过 Tauri API 将内容保存到指定的远程路径
   *
   * @param serverId - 服务器 ID
   * @param path - 目标文件路径
   */
  const saveAs = useCallback(async (serverId: string, path: string): Promise<void> => {
    // 检查 serverId 是否为空
    if (!serverId) {
      setError('未连接到服务器');
      return;
    }

    // 检查路径是否为空
    if (!path) {
      setError('请输入有效的文件路径');
      return;
    }

    // 清除之前的错误
    setError(null);
    setIsSaving(true);

    try {
      // 调用 Tauri API 保存远程文件
      const result = await invoke<WriteFileResult>('remote_write_file', {
        serverId,
        path,
        content: localContent,
      });

      // 计算校验和
      const checksum = await calculateChecksum(localContent);

      // 创建文件状态（保存成功后，新文件变为已打开文件）
      const newFileState: FileState = {
        path: result.path,
        mtime: 0, // 当前 API 不返回 mtime，后续 Task 2.2 会扩展
        checksum,
        isLocked: false,
        fileType: 'text',
        content: localContent,
      };

      setFileState(newFileState);
      setOriginalChecksum(checksum);
      setLocalContent(''); // 清空本地缓存（因为现在是已打开文件）
    } catch (err) {
      // 处理错误
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(`无法保存文件 "${path}": ${errorMessage}`);
    } finally {
      setIsSaving(false);
    }
  }, [localContent]);

  /**
   * 关闭文件
   *
   * 清除当前文件状态，恢复到新文件状态
   */
  const closeFile = useCallback((): void => {
    setFileState(null);
    setOriginalChecksum('');
    setError(null);
    setIsLoading(false);
    setIsSaving(false);
    setLocalContent(''); // 清空本地缓存
  }, []);

  /**
   * 更新文件内容
   *
   * 在本地编辑时调用：
   * - 如果是已打开文件，更新 fileState.content
   * - 如果是新文件，更新 localContent
   *
   * @param newContent - 新的文件内容
   */
  const updateContent = useCallback((newContent: string): void => {
    // 如果是新文件（未打开文件），只更新本地缓存
    if (!fileState) {
      setLocalContent(newContent);
      return;
    }

    // 如果是已打开文件，更新 fileState
    setFileState(prev => {
      if (!prev || prev.fileType !== 'text') return prev;

      // 生成版本号，用于防止竞态条件
      const updateVersion = Date.now();

      // 先更新内容（同步）+ 版本号
      const newState = {
        ...prev,
        content: newContent,
        _updateVersion: updateVersion, // 内部追踪字段
      };

      // 异步计算并更新校验和（不阻塞用户输入）
      calculateChecksum(newContent).then(newChecksum => {
        setFileState(current => {
          if (!current || current.fileType !== 'text') return current;

          // 只更新匹配版本号的校验和，防止竞态条件
          if (current._updateVersion !== updateVersion) return current;

          return {
            ...current,
            checksum: newChecksum,
          };
        });
      });

      return newState;
    });
  }, [fileState]);

  /**
   * 检查文件是否有未保存的变更
   *
   * 通过比较当前校验和与原始校验和来判断：
   * - 如果是新文件（fileState == null），只要有内容就认为有未保存变更
   * - 如果是已打开文件，通过校验和比较来判断
   *
   * @returns 是否有未保存的变更
   */
  const hasUnsavedChanges = useCallback((): boolean => {
    // 如果是新文件，只要有内容就认为有未保存变更
    if (!fileState) {
      return localContent.length > 0;
    }

    // 如果不是文本文件，返回 false
    if (fileState.fileType !== 'text') return false;

    // 如果文件正在加载或保存，认为没有未保存的变更
    if (isLoading || isSaving) return false;

    // 比较当前校验和与原始校验和
    return fileState.checksum !== originalChecksum;
  }, [fileState, isLoading, isSaving, originalChecksum, localContent]);

  /**
   * 清除错误
   */
  const clearError = useCallback((): void => {
    setError(null);
  }, []);

  return {
    fileState,
    isLoading,
    isSaving,
    error,
    localContent,
    openFile,
    saveFile,
    saveWithDiff,
    saveAs,
    closeFile,
    updateContent,
    hasUnsavedChanges,
    clearError,
  };
}