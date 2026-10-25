/**
 * AppIcon 语义 key 单一真相源
 * （从 AppIcon.tsx 拆出：types.ts / fileIcons.ts 等处仅 import 类型，
 *  避免引入 React 组件模块的循环依赖）
 */
export type AppIconName =
  // 应用
  | 'terminal' | 'files' | 'monitor' | 'editor' | 'settings' | 'browser'
  | 'image' | 'hex' | 'pdf' | 'archive'
  // 设置区块
  | 'connection' | 'appearance' | 'keyboard' | 'notifications' | 'stats' | 'about'
  // 侧栏书签
  | 'documents' | 'pictures' | 'music' | 'videos' | 'network' | 'home'
  | 'download' | 'disk'
  // 文件类型
  | 'folder' | 'folder-open' | 'folder-plus' | 'file' | 'file-text' | 'file-code'
  | 'file-json' | 'file-image' | 'file-audio' | 'file-video' | 'file-archive'
  | 'config' | 'database' | 'lock'
  // 动作
  | 'trash' | 'search' | 'package';
