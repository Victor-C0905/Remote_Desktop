/**
 * 文件图标映射（FileManager 与 TextEditor 共用）
 *
 * 从 FileManager 提取，统一两个应用的文件图标语言：
 * 扩展名精确匹配 → 隐藏文件（无匹配扩展名时）→ 默认 📄
 */

export const FOLDER_ICON = "📁";

export const FILE_ICONS: Record<string, string> = {
  ".txt": "📄", ".md": "📝", ".json": "📋", ".toml": "⚙️",
  ".yaml": "⚙️", ".yml": "⚙️", ".rs": "🦀", ".ts": "📘",
  ".tsx": "📘", ".js": "📙", ".py": "🐍", ".css": "🎨",
  ".html": "🌐", ".sh": "🔧", ".conf": "⚙️", ".log": "📜",
  ".png": "🖼️", ".jpg": "🖼️", ".jpeg": "🖼️", ".svg": "🖼️",
  ".mp4": "🎬", ".mp3": "🎵", ".zip": "📦", ".tar": "📦",
  ".gz": "📦", ".db": "🗃️", ".sql": "🗃️",
};

/**
 * 按文件名取图标（不带路径；目录请使用 FOLDER_ICON）
 */
export function getFileIconForName(name: string): string {
  const ext = name.lastIndexOf(".");
  if (ext >= 0) {
    const icon = FILE_ICONS[name.slice(ext).toLowerCase()];
    if (icon) return icon;
  }
  if (name.startsWith(".")) return "🔒";
  return "📄";
}
