/**
 * 文件管理器侧边栏数据构建（FileManager 与 usePreloader 共用，避免两处分叉）
 *
 * 数据来源：
 * - 常用：本机按服务器统计的目录访问次数（filePathUsageStore），
 *   排除家目录、标准位置与已书签路径（书签组已有入口）
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

/** 手动添加的文件夹书签（folderBookmarksStore 产出） */
export interface FolderBookmark {
  path: string;
  addedAt: number;
}

/** 侧边栏项（labelKey: 预置书签词典 key，渲染处 t()；label: 动态标签如真实目录名） */
export interface SidebarItem {
  icon: AppIconName;
  labelKey?: string;
  label?: string;
  path: string;
  /** 悬浮提示（常用路径显示完整路径） */
  title?: string;
  /** 手动书签项：渲染悬浮 ×（移除入口，仅书签组使用） */
  removable?: boolean;
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
  /** 手动添加的文件夹书签（无书签时不渲染该分组） */
  folderBookmarks?: FolderBookmark[];
}): SidebarSection[] {
  const { username, homeDir, activeServerId, commonLocations, frequentPaths, folderBookmarks } = params;

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
  // 书签优先：已手动固定的路径不再进入常用组，避免同一路径两行展示、双高亮
  const bookmarkedPaths = new Set((folderBookmarks ?? []).map((bm) => bm.path));
  const frequentItems = (frequentPaths ?? [])
    .filter((entry) => !excluded.has(entry.path))
    .filter((entry) => !bookmarkedPaths.has(entry.path))
    .map((entry) => ({
      icon: 'folder' as const,
      label: pathLabel(entry.path),
      title: entry.path,
      path: entry.path,
    }));

  const sections: SidebarSection[] = [];

  // 手动书签：用户显式添加的常用文件夹，排最前（显式意图优先于自动统计）。
  // 不与「位置」组去重——用户显式意图优先于去重美学
  const bookmarkItems = (folderBookmarks ?? []).map((bm) => ({
    icon: 'folder' as const,
    label: pathLabel(bm.path),
    title: bm.path,
    path: bm.path,
    removable: true,
  }));
  if (bookmarkItems.length > 0) {
    sections.push({ titleKey: 'fileManager.bookmarks', items: bookmarkItems });
  }

  if (frequentItems.length > 0) {
    sections.push({ titleKey: 'fileManager.frequent', items: frequentItems });
  }

  sections.push({ titleKey: 'fileManager.location', items: homeItems });

  return sections;
}

/* ── 目录树：展开后平铺为可见行 ─────────────────────────── */

/** 每层最多展示的子目录数（超出以「还有 N 项」提示，避免超大目录撑爆侧栏） */
export const SIDEBAR_CHILDREN_LIMIT = 200;

/** 单个目录的子目录装载结果 */
export interface SidebarChildren {
  /** 子目录项（仅目录、按名称排序，已按上限截断） */
  items: SidebarItem[];
  /** 超出展示上限被裁掉的条目数 */
  hidden: number;
}

/** 子目录加载状态（FileManager 维护，flatten 只消费） */
export type SidebarChildrenState =
  | { status: 'loading' }
  | { status: 'loaded'; items: SidebarItem[]; hidden: number }
  | { status: 'error'; message: string };

/** 目录路径 → 加载状态 */
export type SidebarChildrenStates = Record<string, SidebarChildrenState | undefined>;

/** 侧栏可见行：分组顶层项 + 展开后插入的子目录/状态行 */
export type SidebarRow =
  | {
      kind: 'item';
      item: SidebarItem;
      /** 缩进层级：0 = 分组顶层项，1 = 一级子目录…… */
      depth: number;
      /** 是否带展开箭头（离线占位项不提供展开） */
      expandable: boolean;
      expanded: boolean;
    }
  | {
      kind: 'status';
      /** loading 加载中 / error 加载失败可重试 / more 超出展示上限 */
      state: 'loading' | 'error' | 'more';
      depth: number;
      /** 状态所属目录（error 重试时作为请求路径） */
      path: string;
      /** error：错误信息 */
      message?: string;
      /** more：被裁掉的条目数 */
      count?: number;
    };

/** 分组 + 可见行（渲染层按组输出标题） */
export interface SidebarSectionRows {
  titleKey?: string;
  title?: string;
  rows: SidebarRow[];
}

/**
 * 从 read_dir 结果构建子目录项：只保留目录、按名称排序，超出上限截断。
 * 只显示子目录——侧栏是导航树，文件条目属于内容区。
 */
export function buildSidebarChildren(
  parentPath: string,
  entries: readonly { name: string; is_dir: boolean }[],
  limit = SIDEBAR_CHILDREN_LIMIT,
): SidebarChildren {
  const names = entries
    .filter((entry) => entry.is_dir)
    .map((entry) => entry.name)
    .sort((a, b) => a.localeCompare(b));
  const shown = names.slice(0, Math.max(0, limit));
  return {
    items: shown.map((name) => ({
      icon: 'folder',
      label: name,
      title: joinRemotePath(parentPath, name),
      path: joinRemotePath(parentPath, name),
    })),
    hidden: Math.max(0, names.length - shown.length),
  };
}

/**
 * 把分组 + 展开态平铺为可见行（纯函数，便于单测）。
 * 展开的目录行后紧跟其子目录（递归），加载中/无子目录/失败/超限以状态行占位。
 * `allowExpand=false`（离线占位项）时不输出箭头与子行。
 */
export function flattenSidebarSections(
  sections: SidebarSection[],
  params: {
    expandedPaths: ReadonlySet<string>;
    children: SidebarChildrenStates;
    allowExpand?: boolean;
  },
): SidebarSectionRows[] {
  const { expandedPaths, children, allowExpand = true } = params;

  const appendRows = (rows: SidebarRow[], items: SidebarItem[], depth: number): void => {
    for (const item of items) {
      const state = children[item.path];
      // 已加载且没有子目录 = 叶子：不显示展开箭头，也不保留展开态
      //（点击叶子只会得到空结果，不应提供无效的展开入口）
      const knownLeaf = state?.status === 'loaded' && state.items.length === 0;
      const hasChildren = state?.status === 'loaded' && state.items.length > 0;
      const expanded = allowExpand && !knownLeaf && expandedPaths.has(item.path);
      // 箭头只在「已知有子目录 / 正在展开 / 加载失败可重试」时显示。
      // 未知目录先留空槽，等预取结果再补箭头——避免箭头先渲染再消失
      const expandable =
        allowExpand && (hasChildren || expanded || state?.status === 'error');
      rows.push({
        kind: 'item',
        item,
        depth,
        expandable,
        expanded,
      });
      if (!expanded) continue;

      const childDepth = depth + 1;
      if (!state || state.status === 'loading') {
        rows.push({ kind: 'status', state: 'loading', depth: childDepth, path: item.path });
      } else if (state.status === 'error') {
        rows.push({
          kind: 'status',
          state: 'error',
          depth: childDepth,
          path: item.path,
          message: state.message,
        });
      } else {
        appendRows(rows, state.items, childDepth);
        if (state.hidden > 0) {
          rows.push({
            kind: 'status',
            state: 'more',
            depth: childDepth,
            path: item.path,
            count: state.hidden,
          });
        }
      }
    }
  };

  return sections.map((section) => {
    const rows: SidebarRow[] = [];
    appendRows(rows, section.items, 0);
    return { titleKey: section.titleKey, title: section.title, rows };
  });
}
