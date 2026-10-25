/**
 * 文件图标映射（FileManager 与 TextEditor 共用）
 *
 * 返回 AppIcon 语义 key（见 components/AppIcon.tsx），由渲染处
 * <AppIcon name={...}/> 落地为单色线性图标——emoji 图标跨平台渲染
 * 不一致、不跟主题，已全站弃用。
 * 匹配顺序：扩展名精确匹配 → 隐藏文件 → 默认 'file'
 */
import type { AppIconName } from '../components/AppIcon';

export const FOLDER_ICON: AppIconName = 'folder';

export const FILE_ICONS: Record<string, AppIconName> = {
  ".txt": "file-text", ".md": "file-text", ".log": "file-text",
  ".doc": "file-text", ".docx": "file-text", ".rtf": "file-text", ".odt": "file-text",
  ".csv": "file-text", ".xlsx": "file-text",
  ".pdf": "pdf",
  ".json": "file-json",
  ".toml": "config", ".yaml": "config", ".yml": "config", ".conf": "config", ".ini": "config",
  ".rs": "file-code", ".ts": "file-code", ".tsx": "file-code", ".js": "file-code",
  ".jsx": "file-code", ".py": "file-code", ".sh": "file-code", ".bash": "file-code",
  ".css": "file-code", ".html": "file-code", ".xml": "file-code",
  ".png": "file-image", ".jpg": "file-image", ".jpeg": "file-image", ".svg": "file-image",
  ".gif": "file-image", ".webp": "file-image", ".bmp": "file-image", ".ico": "file-image",
  ".mp4": "file-video", ".mkv": "file-video", ".webm": "file-video", ".avi": "file-video", ".mov": "file-video",
  ".mp3": "file-audio", ".wav": "file-audio", ".flac": "file-audio", ".ogg": "file-audio", ".m4a": "file-audio",
  ".zip": "file-archive", ".tar": "file-archive", ".gz": "file-archive",
  ".bz2": "file-archive", ".xz": "file-archive", ".7z": "file-archive", ".rar": "file-archive",
  ".iso": "disk",
  ".db": "database", ".sql": "database",
};

/**
 * 按文件名取图标语义 key（不带路径；目录请使用 FOLDER_ICON）
 */
export function getFileIconForName(name: string): AppIconName {
  const ext = name.lastIndexOf(".");
  if (ext >= 0) {
    const icon = FILE_ICONS[name.slice(ext).toLowerCase()];
    if (icon) return icon;
  }
  if (name.startsWith(".")) return "lock";
  return "file";
}
