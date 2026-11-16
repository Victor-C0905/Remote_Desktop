/**
 * 文件管理器侧边栏数据构建（FileManager 与 usePreloader 共用，避免两处分叉）
 *
 * 数据来源：
 * - 常用：本机按服务器统计的目录访问次数（filePathUsageStore），
 *   排除家目录与标准位置（侧边栏「位置」已有，重复入口属于功能冗余）
 * - 家目录 / 用户名：remote_get_current_user（Agent 按 /etc/passwd 返回）
 * - 标准位置：Agent 返回的 common_locations（XDG user-dirs 中真实存在的目录）
 */
import type { AppIconName } from '../components/iconNames';

/** 服务器标准位置（与 Agent CommonLocation 字段一致） */
export interface CommonLocation {
  /** "home" 家目录标准目录 */
  group: string;
  /** 稳定语义 key（用于选图标） */
  key: string;
  /** 展示名称：真实目录名（如「文档」） */
  label: string;
  path: string;
}

/** 常用路径（frecency 统计结果，filePathUsageStore 产出） */
export interface FrequentPath {
  path: string;
  /** 当前时间衰减后的分数（已由 selector 过滤门槛） */
  score: number;
  count: number;
  lastUsed: number;
}

/** 侧边栏项（labelKey: 预置书签词典 key，渲染处 t()；label: 动态标签如真实目录名） */
export interface SidebarItem {
  icon: AppIconName;
  labelKey?: string;
  label?: string;
  path: string;
  /** 悬浮提示（常用路径显示完整路径） */
  title?: string;
}

/** 侧边栏部分（titleKey: 词典 key，渲染处 t()） */
export interface SidebarSection {
  titleKey?: string;
  title?: string;
  items: SidebarItem[];
}

/** 语义 key → 图标（未知 key 用文件夹图标兜底） */
const LOCATION_ICONS: Partial<Record<string, AppIconName>> = {
  documents: 'documents',
  downloads: 'download',
  pictures: 'pictures',
  music: 'music',
  videos: 'videos',
};

/** 旧版 Agent（不返回 common_locations）的家目录标准目录回退布局 */
const LEGACY_HOME_DIRS: { icon: AppIconName; labelKey: string; name: string }[] = [
  { icon: 'documents', labelKey: 'fileManager.documents', name: 'Documents' },
  { icon: 'download', labelKey: 'fileManager.downloads', name: 'Downloads' },
  { icon: 'pictures', labelKey: 'fileManager.pictures', name: 'Pictures' },
  { icon: 'music', labelKey: 'fileManager.music', name: 'Music' },
  { icon: 'videos', labelKey: 'fileManager.videos', name: 'Videos' },
];

/** 拼接远程 Linux 路径（家目录可能为 "/" 或以 "/" 结尾） */
function joinRemotePath(base: string, name: string): string {
  return base.endsWith('/') ? `${base}${name}` : `${base}/${name}`;
}

/** 路径的展示名：取最后一段（根目录显示 "/"） */
function pathLabel(path: string): string {
  if (path === '/') return '/';
  const parts = path.split('/').filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/**
 * 构建侧边栏分组
 * - 远程模式：常用（按访问统计，排除家目录与标准位置） + 主目录/标准位置
 * - 离线模式：保留默认 Linux 结构占位（不可导航，仅维持 UI 一致）
 */
export function buildSidebarSections(params: {
  username: string | null;
  homeDir: string | null;
  activeServerId: string | null;
  commonLocations?: CommonLocation[];
  /** 按访问分数排序的常用目录（无统计时不渲染该分组） */
  frequentPaths?: FrequentPath[];
}): SidebarSection[] {
  const { username, homeDir, activeServerId, commonLocations, frequentPaths } = params;

  if (!activeServerId || !username || !homeDir) {
    // 离线模式：使用默认 Linux 侧边栏（保持 UI 结构一致）
    return [
      {
        titleKey: 'fileManager.location',
        items: [
          { icon: 'home', labelKey: 'fileManager.home', path: '/home' },
          ...LEGACY_HOME_DIRS.map((dir) => ({
            icon: dir.icon,
            labelKey: dir.labelKey,
            path: joinRemotePath('/home', dir.name),
          })),
        ],
      },
    ];
  }

  const homeItems: SidebarItem[] = [
    { icon: 'home', labelKey: 'fileManager.home', path: homeDir },
  ];
  const seenPaths = new Set([homeDir]);
  // Agent 只返回服务器上真实存在的标准目录；空数组 = 家目录下没有 XDG 目录
  // （如容器内的 root）。此时仅展示主目录——硬编码回退会指向不存在的目录
  //（R6-P2-02；旧版 Agent 将失去标准位置快捷方式，属设计取舍 2026-11-12）
  for (const loc of (commonLocations ?? []).filter((loc) => loc.group === 'home')) {
    if (!loc.path || seenPaths.has(loc.path)) continue;
    seenPaths.add(loc.path);
    homeItems.push({
      icon: LOCATION_ICONS[loc.key] ?? 'folder',
      label: loc.label || loc.path,
      path: loc.path,
    });
  }

  // 常用：完全由本机访问统计生成（无统计/首次使用时整组隐藏）。
  // 排除家目录与标准位置：侧边栏「位置」已有入口，重复展示属于功能冗余
  const excluded = new Set(homeItems.map((item) => item.path));
  const frequentItems = (frequentPaths ?? [])
    .filter((entry) => !excluded.has(entry.path))
    .map((entry) => ({
      icon: 'folder' as const,
      label: pathLabel(entry.path),
      title: entry.path,
      path: entry.path,
    }));

  const sections: SidebarSection[] = [];
  if (frequentItems.length > 0) {
    sections.push({ titleKey: 'fileManager.frequent', items: frequentItems });
  }

  sections.push({ titleKey: 'fileManager.location', items: homeItems });

  return sections;
}
