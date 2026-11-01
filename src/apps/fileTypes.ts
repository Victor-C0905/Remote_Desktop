// src/apps/fileTypes.ts
// 文件条目类型（单一来源）：FileManager / fileTable / 各 hooks 共享，
// 避免组件文件与子模块间的类型循环引用。

export interface FileEntry {
  name: string;
  is_dir: boolean;
  size: number;
  mtime: string;
  permissions: string;
  owner?: string;  // 所有者（旧版 Agent 不下发时为 undefined，显示回退 "—"）
  group?: string;  // 所属组（同上）
  path?: string;   // 搜索命中自带完整路径（嵌套目录定位依据）；普通浏览为 undefined
}
