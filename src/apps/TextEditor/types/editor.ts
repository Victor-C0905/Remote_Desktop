/**
 * 文件编辑器类型定义
 * 
 * 定义了文件编辑器所需的所有 TypeScript 类型
 */

/**
 * 文件状态
 *
 * 表示一个打开的文件的完整状态，包括文件路径、内容、元数据和锁定信息
 *
 * 使用联合类型区分三种状态：
 * - text: 文本文件，必须有 content
 * - binary: 二进制文件，必须有 binaryData
 * - loading: 加载中，内容还未就绪
 */
export type FileState = {
  /** 文件的绝对路径 */
  path: string;

  /** 文件的修改时间（Unix 时间戳，毫秒） */
  mtime: number;

  /** 文件内容的校验和（用于检测远程变更） */
  checksum: string;

  /** 文件是否被锁定（防止多用户同时编辑） */
  isLocked: boolean;

  /** 锁定文件的用户标识（如果文件被锁定） */
  lockedBy?: string;

  /** 内部版本号（用于防止竞态条件，仅在文本文件编辑时使用） */
  _updateVersion?: number;
} & (
  | {
      /** 文件类型：文本文件 */
      fileType: 'text';
      /** 文件的文本内容 */
      content: string;
      /** 文本文件不应该有二进制数据 */
      binaryData?: never;
    }
  | {
      /** 文件类型：二进制文件 */
      fileType: 'binary';
      /** 文件的二进制数据 */
      binaryData: Uint8Array;
      /** 二进制文件不应该有文本内容 */
      content?: never;
    }
  | {
      /** 文件类型：加载中 */
      fileType: 'loading';
      /** 加载中的文件不应该有内容 */
      content?: never;
      binaryData?: never;
    }
);

/**
 * 文件变更
 *
 * 表示文件中的一个具体变更操作
 *
 * 使用可辨识联合类型（discriminated union）确保：
 * - insert 操作必须有 new 属性，不能有 old 属性
 * - delete 操作必须有 old 属性，不能有 new 属性
 * - replace 操作必须同时有 old 和 new 属性
 */
export type FileChange =
  | {
      /** 变更类型：插入新行 */
      type: 'insert';
      /** 变更发生的行号（从 0 开始） */
      line: number;
      /** 新插入的内容 */
      new: string;
    }
  | {
      /** 变更类型：删除行 */
      type: 'delete';
      /** 变更发生的行号（从 0 开始） */
      line: number;
      /** 被删除的原始内容 */
      old: string;
    }
  | {
      /** 变更类型：替换行 */
      type: 'replace';
      /** 变更发生的行号（从 0 开始） */
      line: number;
      /** 被替换的原始内容 */
      old: string;
      /** 新替换的内容 */
      new: string;
    };

/**
 * 编辑器模式
 * 
 * 定义编辑器的显示模式
 */
export type EditorMode = 'text' | 'hex';

/**
 * 编辑器状态
 * 
 * 表示编辑器的完整状态，包括当前文件、编辑模式和加载状态
 */
export interface EditorState {
  /** 当前打开的文件状态 */
  fileState: FileState | null;
  
  /** 编辑器显示模式 */
  editorMode: EditorMode;
  
  /** 是否正在加载文件 */
  isLoading: boolean;
  
  /** 是否正在保存文件 */
  isSaving: boolean;
  
  /** 是否有未保存的变更 */
  hasUnsavedChanges: boolean;
}

/**
 * 差异响应（Agent 返回）
 */
export interface DiffResponse {
  path: string;
  diffs: FileChange[];
  mtime: number;
}

/**
 * 应用差异响应（Agent 返回）
 */
export interface ApplyDiffResponse {
  path: string;
  success: boolean;
  new_mtime: number;
  error?: string;
}