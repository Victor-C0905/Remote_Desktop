/**
 * 远程路径小工具（无平台依赖，供目录选择器与保存对话框复用）
 */

/** 规范化绝对路径：补前导斜杠、去掉空段与尾部斜杠 */
export function normalizeRemotePath(path: string): string {
  const abs = path.startsWith('/') ? path : `/${path}`;
  const parts = abs.split('/').filter(Boolean);
  return `/${parts.join('/')}`;
}

/** 拼接目录与名称 */
export function joinRemotePath(base: string, name: string): string {
  const dir = normalizeRemotePath(base);
  return dir === '/' ? `/${name}` : `${dir}/${name}`;
}

/** 取父目录（根目录返回 '/'） */
export function parentRemotePath(path: string): string {
  const dir = normalizeRemotePath(path);
  if (dir === '/') return '/';
  const idx = dir.lastIndexOf('/');
  return idx <= 0 ? '/' : dir.slice(0, idx);
}
