# 文件管理器「书签」——手动添加常用文件夹 实现方案

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在文件管理器侧边栏新增「书签」分组，用户可手动把任意文件夹加入/移除书签（按服务器隔离、持久化），对齐资源管理器"固定到快速访问"的体验。

**Architecture:** 新增 zustand persist store（按 serverId 隔离，复用 fileManagerStorage 存储域），纯函数 `buildSidebarSections` 增加书签分组产出，FM 工具栏加星标 toggle（添加当前目录）+ 侧栏书签项悬浮 ×（移除）。删除服务器时经 `utils/serverCleanup.ts` 统一清理。

**Tech Stack:** React 18 + zustand/persist（现有模式）、vitest + @testing-library（jsdom）、现有 i18n 词典（zh-CN.ts / en-US.ts）。

---

## 背景与设计取舍（已定，不再开放讨论）

| 决策点 | 取舍 | 理由 |
|---|---|---|
| 分组归属 | **独立「书签」分组**，排在「常用」（frecency 统计）之前 | 手动固定与自动统计语义不同、生命周期不同；移除入口只作用于书签项。不与「常用」混合（Explorer 快速访问是合并展示，但本应用的移除交互需要区分来源） |
| 与「常用」去重 | **书签优先**——常用组过滤已书签路径（2026-11-23 用户确认） | 避免同一路径两行展示与双高亮；书签是显式固定，优先于自动统计 |
| 与「位置」组去重 | **不去重**——用户显式添加的路径即使已在「位置」组也照常展示 | 用户显式意图优先于去重美学；与 bookmarksStore（浏览器书签）行为一致 |
| 排序 | 追加序（先加在前），**不做拖拽排序** | YAGNI；数据结构按追加序存储，未来可加 |
| 数据模型 | `{ path, addedAt }`，同服务器同路径去重，**无 id** | 移除入口只拿得到 path；path 在服务器内唯一，按 path 移除无歧义 |
| 离线模式 | 不渲染书签分组 | 无 activeServerId 无归属（buildSidebarSections 离线分支自然跳过） |
| 添加入口 | 工具栏星标 toggle（当前目录）；**不做**侧栏拖拽添加 | 最小可用入口；拖拽添加（把目录拖到分组标题）留作后续 |
| 移除入口 | 书签项悬浮 × 按钮 | 与搜索框 ✕、Win11 Explorer 快速访问「取消固定」同语义 |
| 存储 | `fileManagerStorage`（与常用路径统计同存储域），zustand persist name `fm-folder-bookmarks` | 与 filePathUsageStore 同域；不进 servers.bin（业务仓），不进 localStorage（跨会话用户数据） |

**涉及红线：无**（纯前端 src/ 改动，不触碰浏览器会话/窗口/协议/安全红线）。

**工作分支**：从 dev 拉出 `feature/fm-folder-bookmarks`（执行前先同步本地 dev；当前会话若在 `feature/ui-audit-fixes` 有未提交改动，须先经用户处置，不得夹带）。

## File Structure

| 文件 | 动作 | 职责 |
|---|---|---|
| `src/stores/folderBookmarksStore.ts` | 新建 | 书签状态：按服务器隔离、去重、持久化 |
| `src/stores/folderBookmarksStore.test.ts` | 新建 | store 行为回归 |
| `src/apps/fileSidebar.ts` | 修改 | `FolderBookmark` 类型、`SidebarItem.removable`、书签分组构建 |
| `src/apps/__tests__/fileSidebar.test.ts` | 新建 | 分组构建回归（纯函数） |
| `src/apps/FileManager.tsx` | 修改 | store 接线、分组数据下发、侧栏 × 移除、工具栏星标 toggle |
| `src/utils/serverCleanup.ts` | 修改 | 服务器删除时清理书签 |
| `src/i18n/locales/zh-CN.ts` / `en-US.ts` | 修改 | 3 个新词典 key |
| `src/apps/FileManager.css` | 修改 | 书签项 × 按钮样式 |

---

### Task 1: folderBookmarksStore（按服务器隔离的书签 store）

**Files:**
- Create: `src/stores/folderBookmarksStore.ts`
- Test: `src/stores/folderBookmarksStore.test.ts`

- [x] **Step 1: 写失败测试**（镜像 bookmarksStore.test.ts 的模式：zustand 直连 getState，beforeEach 重置）

```ts
// src/stores/folderBookmarksStore.test.ts
import { describe, it, expect, beforeEach } from "vitest";
import {
  selectFolderBookmarks,
  isFolderBookmarked,
  useFolderBookmarksStore,
} from "./folderBookmarksStore";

const state = () => useFolderBookmarksStore.getState();
const pathsOf = (serverId: string | null) =>
  selectFolderBookmarks(state().folderBookmarksByServer, serverId).map((b) => b.path);

beforeEach(() => {
  useFolderBookmarksStore.setState({ folderBookmarksByServer: {} });
});

describe("folderBookmarksStore 按服务器隔离", () => {
  it("新增/删除只影响对应服务器", () => {
    state().addFolderBookmark("s1", "/var/log");
    state().addFolderBookmark("s2", "/etc");

    expect(pathsOf("s1")).toEqual(["/var/log"]);
    expect(pathsOf("s2")).toEqual(["/etc"]);

    state().removeFolderBookmark("s1", "/var/log");
    expect(pathsOf("s1")).toEqual([]);
    expect(pathsOf("s2")).toEqual(["/etc"]);
  });

  it("serverId 为 null 时不写入，选择器返回稳定的空列表", () => {
    state().addFolderBookmark(null, "/var/log");

    expect(state().folderBookmarksByServer).toEqual({});
    // 缺省时复用同一空数组引用，避免 useSyncExternalStore 快照抖动
    expect(selectFolderBookmarks({}, null)).toBe(selectFolderBookmarks({}, "s1"));
  });

  it("同服务器同路径去重", () => {
    state().addFolderBookmark("s1", "/var/log");
    state().addFolderBookmark("s1", "/var/log");

    const list = selectFolderBookmarks(state().folderBookmarksByServer, "s1");
    expect(list).toHaveLength(1);
    expect(list[0].addedAt).toBeGreaterThan(0);
  });

  it("移除不存在的路径不产生状态变更", () => {
    state().addFolderBookmark("s1", "/var/log");
    const before = state().folderBookmarksByServer;

    state().removeFolderBookmark("s1", "/not/exist");

    expect(state().folderBookmarksByServer).toBe(before);
  });

  it("clearServer 只清理指定服务器", () => {
    state().addFolderBookmark("s1", "/var/log");
    state().addFolderBookmark("s2", "/etc");

    state().clearServer("s1");

    expect(pathsOf("s1")).toEqual([]);
    expect(pathsOf("s2")).toEqual(["/etc"]);
    expect("s1" in state().folderBookmarksByServer).toBe(false);
  });

  it("isFolderBookmarked 判定星标 toggle 状态", () => {
    state().addFolderBookmark("s1", "/var/log");
    const list = selectFolderBookmarks(state().folderBookmarksByServer, "s1");

    expect(isFolderBookmarked(list, "/var/log")).toBe(true);
    expect(isFolderBookmarked(list, "/etc")).toBe(false);
    expect(isFolderBookmarked(list, undefined)).toBe(false);
  });
});
```

- [x] **Step 2: 跑测试确认失败**

Run: `npx vitest run src/stores/folderBookmarksStore.test.ts`
Expected: FAIL（模块不存在）

- [x] **Step 3: 最小实现**

```ts
// src/stores/folderBookmarksStore.ts
// 文件管理器「书签」：手动添加的常用文件夹，按服务器隔离，持久化于
// fileManagerStorage（与常用路径统计同存储域）。同路径去重（星标
// toggle 由调用方先查 isFolderBookmarked 后调）。

import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { fileManagerStorage } from "../utils/storage";

export interface FolderBookmark {
  path: string;
  addedAt: number;
}

/** 空列表常量：selector 缺省时复用同一引用，避免 useSyncExternalStore 快照抖动 */
const EMPTY_BOOKMARKS: FolderBookmark[] = [];

interface FolderBookmarksState {
  /** serverId → 书签列表（追加序，侧栏按此顺序展示） */
  folderBookmarksByServer: Record<string, FolderBookmark[]>;
  addFolderBookmark: (serverId: string | null, path: string) => void;
  removeFolderBookmark: (serverId: string | null, path: string) => void;
  /** 删除服务器时清理其书签（utils/serverCleanup 统一清理入口调用） */
  clearServer: (serverId: string) => void;
}

/** 读取某服务器的书签（null 或无数据时返回稳定的空列表） */
export function selectFolderBookmarks(
  byServer: Record<string, FolderBookmark[]>,
  serverId: string | null,
): FolderBookmark[] {
  if (!serverId) return EMPTY_BOOKMARKS;
  return byServer[serverId] ?? EMPTY_BOOKMARKS;
}

/** 某路径是否已收藏（星标按钮的 toggle 判定） */
export const isFolderBookmarked = (
  bookmarks: FolderBookmark[],
  path: string | undefined | null,
): boolean => !!path && bookmarks.some((b) => b.path === path);

export const useFolderBookmarksStore = create<FolderBookmarksState>()(
  persist(
    (set) => ({
      folderBookmarksByServer: {},

      addFolderBookmark: (serverId, path) =>
        set((s) => {
          if (!serverId || !path) return s;
          const list = s.folderBookmarksByServer[serverId] ?? [];
          if (list.some((b) => b.path === path)) return s;
          return {
            folderBookmarksByServer: {
              ...s.folderBookmarksByServer,
              [serverId]: [...list, { path, addedAt: Date.now() }],
            },
          };
        }),

      removeFolderBookmark: (serverId, path) =>
        set((s) => {
          if (!serverId || !path) return s;
          const list = s.folderBookmarksByServer[serverId];
          if (!list) return s;
          const next = list.filter((b) => b.path !== path);
          if (next.length === list.length) return s;
          return {
            folderBookmarksByServer: { ...s.folderBookmarksByServer, [serverId]: next },
          };
        }),

      clearServer: (serverId) =>
        set((s) => {
          if (!(serverId in s.folderBookmarksByServer)) return s;
          const next = { ...s.folderBookmarksByServer };
          delete next[serverId];
          return { folderBookmarksByServer: next };
        }),
    }),
    {
      name: "fm-folder-bookmarks",
      version: 1,
      storage: createJSONStorage(() => fileManagerStorage),
      partialize: (s) => ({ folderBookmarksByServer: s.folderBookmarksByServer }),
    },
  ),
);
```

- [x] **Step 4: 跑测试确认通过**

Run: `npx vitest run src/stores/folderBookmarksStore.test.ts`
Expected: PASS 6/6

- [x] **Step 5: 提交**

```bash
git add src/stores/folderBookmarksStore.ts src/stores/folderBookmarksStore.test.ts
git commit -m "feat(ui): 文件夹书签 store——按服务器隔离/去重/持久化"
```

---

### Task 2: buildSidebarSections 产出「书签」分组

**Files:**
- Modify: `src/apps/fileSidebar.ts`
- Test: `src/apps/__tests__/fileSidebar.test.ts`（新建）

- [x] **Step 1: 写失败测试**

```ts
// src/apps/__tests__/fileSidebar.test.ts
import { describe, it, expect } from "vitest";
import { buildSidebarSections } from "../fileSidebar";

const base = {
  username: "root",
  homeDir: "/root",
  activeServerId: "s1",
  commonLocations: [],
};

describe("buildSidebarSections 书签分组", () => {
  it("书签分组排在最前，项带 removable 标记与完整路径悬浮提示", () => {
    const sections = buildSidebarSections({
      ...base,
      folderBookmarks: [
        { path: "/var/log", addedAt: 1 },
        { path: "/etc/nginx", addedAt: 2 },
      ],
    });

    expect(sections[0].titleKey).toBe("fileManager.bookmarks");
    expect(sections[0].items).toEqual([
      { icon: "folder", label: "log", title: "/var/log", path: "/var/log", removable: true },
      { icon: "folder", label: "nginx", title: "/etc/nginx", path: "/etc/nginx", removable: true },
    ]);
    // 位置组仍在其后
    expect(sections[1].titleKey).toBe("fileManager.location");
  });

  it("无书签时不渲染书签分组", () => {
    const sections = buildSidebarSections(base);
    expect(sections.map((s) => s.titleKey)).not.toContain("fileManager.bookmarks");
  });

  it("离线模式不渲染书签（无服务器归属）", () => {
    const sections = buildSidebarSections({
      username: null,
      homeDir: null,
      activeServerId: null,
      folderBookmarks: [{ path: "/var/log", addedAt: 1 }],
    });
    expect(sections.map((s) => s.titleKey)).not.toContain("fileManager.bookmarks");
  });
});
```

- [x] **Step 2: 跑测试确认失败**

Run: `npx vitest run src/apps/__tests__/fileSidebar.test.ts`
Expected: FAIL（folderBookmarks 参数未生效 / bookmarks 分组不存在）

- [x] **Step 3: 实现**（`src/apps/fileSidebar.ts` 三处改动）

① `SidebarItem` 接口增加字段（`title?: string;` 之后）：

```ts
  /** 手动书签项：渲染悬浮 ×（移除入口，仅书签组使用） */
  removable?: boolean;
```

② 新增类型（`FrequentPath` 接口之后）：

```ts
/** 手动添加的文件夹书签（folderBookmarksStore 产出） */
export interface FolderBookmark {
  path: string;
  addedAt: number;
}
```

③ `buildSidebarSections`：params 增加 `folderBookmarks?: FolderBookmark[];`（`frequentPaths` 参数之后）；section 构建处（`const sections: SidebarSection[] = [];` 与 `if (frequentItems.length > 0)` 之间）插入：

```ts
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
```

（离线分支不动：`!activeServerId` 早返回，自然不含书签。）

- [x] **Step 4: 跑测试确认通过**

Run: `npx vitest run src/apps/__tests__/fileSidebar.test.ts`
Expected: PASS 3/3

- [x] **Step 5: 提交**

```bash
git add src/apps/fileSidebar.ts src/apps/__tests__/fileSidebar.test.ts
git commit -m "feat(ui): buildSidebarSections 产出书签分组（removable 项）"
```

---

### Task 3: i18n 词典 key

**Files:**
- Modify: `src/i18n/locales/zh-CN.ts`（`fileManager` 块内 `location`/`frequent` 键附近，约 442-444 行）
- Modify: `src/i18n/locales/en-US.ts`（同位置，约 433-435 行）

- [x] **Step 1: zh-CN.ts 添加**（`frequent: '常用',` 旁边）

```ts
    bookmarks: '书签',
    addBookmark: '把当前文件夹加入书签',
    removeBookmark: '从书签移除',
```

- [x] **Step 2: en-US.ts 添加**（`frequent: 'Frequent',` 旁边）

```ts
    bookmarks: 'Bookmarks',
    addBookmark: 'Add current folder to bookmarks',
    removeBookmark: 'Remove from bookmarks',
```

- [x] **Step 3: 类型检查确认 key 拼写与词典类型一致**

Run: `npx tsc --noEmit`
Expected: 无新增错误

- [x] **Step 4: 提交**

```bash
git add src/i18n/locales/zh-CN.ts src/i18n/locales/en-US.ts
git commit -m "feat(i18n): 书签分组/添加/移除词典 key（zh-CN + en-US）"
```

---

### Task 4: FM 接线——侧栏渲染书签分组 + × 移除 + 工具栏星标 toggle

**Files:**
- Modify: `src/apps/FileManager.tsx`
- Modify: `src/apps/FileManager.css`

- [x] **Step 1: store 接线**（import 区，`filePathUsageStore` import 旁）

```tsx
import { selectFolderBookmarks, isFolderBookmarked, useFolderBookmarksStore } from '../stores/folderBookmarksStore';
```

`frequentPaths` 的 useMemo（约 394 行）旁添加订阅：

```tsx
  const folderBookmarksByServer = useFolderBookmarksStore((s) => s.folderBookmarksByServer);
  const folderBookmarks = useMemo(
    () => selectFolderBookmarks(folderBookmarksByServer, activeServerId),
    [folderBookmarksByServer, activeServerId]
  );
```

`buildSidebarSections` 调用处（约 905 行）追加参数，并在依赖数组加入 `folderBookmarks`：

```tsx
    const sections = buildSidebarSections({
      username,
      homeDir,
      activeServerId,
      commonLocations,
      frequentPaths,
      folderBookmarks,
    });
```

（保持该 useEffect 原有依赖数组的其他项不变，只增 `folderBookmarks`。）

移除回调（放在 `handleSidebarReset` 附近）：

```tsx
  const removeFolderBookmark = useCallback((path: string) => {
    if (!activeServerId) return;
    useFolderBookmarksStore.getState().removeFolderBookmark(activeServerId, path);
  }, [activeServerId]);

  const currentFolderBookmarked = useMemo(
    () => isFolderBookmarked(folderBookmarks, currentPath),
    [folderBookmarks, currentPath]
  );

  const toggleFolderBookmark = useCallback(() => {
    if (!activeServerId || !currentPath) return;
    const { addFolderBookmark, removeFolderBookmark: rm } = useFolderBookmarksStore.getState();
    if (currentFolderBookmarked) rm(activeServerId, currentPath);
    else addFolderBookmark(activeServerId, currentPath);
  }, [activeServerId, currentPath, currentFolderBookmarked]);
```

- [x] **Step 2: 侧栏项渲染 × 移除按钮**（`sidebar` prop 内，约 2565-2580 行的 `section.items.map` 返回块）

```tsx
                  return (
                  <div
                    key={`${sectionIdx}-${itemIdx}`}
                    className={`fm-sidebar-item${currentPath === item.path ? " fm-sidebar-item-active" : ""}${isDropTarget ? " drop-target" : ""}`}
                    data-drop-dir={item.path}
                    title={item.title}
                    onClick={() => navigateTo(item.path, "sidebar")}
                  >
                    <span className="fm-si-icon"><AppIcon name={item.icon} size={16} /></span>
                    <span className="fm-si-label">{item.labelKey ? t(item.labelKey) : item.label}</span>
                    {item.removable && (
                      <button
                        className="fm-si-remove"
                        title={t("fileManager.removeBookmark")}
                        onClick={(e) => { e.stopPropagation(); removeFolderBookmark(item.path); }}
                      >×</button>
                    )}
                  </div>
                  );
```

（相对现状仅新增 `{item.removable && (...)}` 一块；`fm-si-meta` 挂载项右侧容量徽标不受影响。）

- [x] **Step 3: 工具栏星标 toggle**（`fm-toolbar-view` 簇内，「新建文件夹」按钮**之前**插入）

```tsx
              <button
                className={`fm-toolbar-btn ${currentFolderBookmarked ? "active" : ""}`}
                onClick={toggleFolderBookmark}
                title={currentFolderBookmarked ? t("fileManager.removeBookmark") : t("fileManager.addBookmark")}
                disabled={!activeServerId}
              ><SymbolicIcon name={currentFolderBookmarked ? "star" : "star-outline"} size={15} /></button>
```

- [x] **Step 4: CSS——书签项 × 悬浮按钮**（`FileManager.css` 的 `.fm-sidebar .fm-sidebar-item .fm-si-meta` 块之后，约 415 行）

```css
/* 书签项移除按钮：悬浮浮现（右侧，语义同搜索框 ✕ / Explorer 取消固定） */
.fm-sidebar .fm-sidebar-item .fm-si-remove {
  flex-shrink: 0;
  display: none;
  border: none;
  background: none;
  color: var(--quirel-text-secondary);
  cursor: pointer;
  padding: 0 2px;
  font-size: 12px;
  line-height: 1;
}

.fm-sidebar .fm-sidebar-item:hover .fm-si-remove {
  display: inline-flex;
}

.fm-sidebar .fm-sidebar-item .fm-si-remove:hover {
  color: var(--quirel-text-primary);
}
```

- [x] **Step 5: 全量前端单测确认无回归**

Run: `npm run verify:fe`
Expected: PASS 2/2

- [x] **Step 6: 提交**

```bash
git add src/apps/FileManager.tsx src/apps/FileManager.css
git commit -m "feat(ui): 文件管理器书签——工具栏星标 toggle/侧栏书签组/悬浮移除"
```

---

### Task 5: 服务器删除时清理书签

**Files:**
- Modify: `src/utils/serverCleanup.ts`（「书签」步骤之后，约 33-35 行）

- [x] **Step 1: 登记清理步骤**（steps 数组内，浏览器书签项之后）

```ts
    {
      label: "文件夹书签",
      run: async () =>
        (await import("../stores/folderBookmarksStore")).useFolderBookmarksStore.getState().clearServer(serverId),
    },
```

（沿用该文件「各步骤幂等、单步失败不阻断」的既有约定；此文件无既有测试覆盖，行为经 Task 1 的 clearServer 单测保证，接线属既有集成缝隙，靠实机冒烟验证。）

- [x] **Step 2: 提交**

```bash
git add src/utils/serverCleanup.ts
git commit -m "feat(ui): 删除服务器时统一清理文件夹书签"
```

---

### Task 6: 完成验证

- [x] **Step 1: 全量回归**（2026-11-23 verify:full 12/12 PASS）

Run: `npm run verify:full`
Expected: PASS 12/12

- [x] **Step 2: 实机冒烟**（2026-11-23；6/7 通过，拖拽项受前台占用阻断）

1. 连接服务器 → 进入任意目录 → 点工具栏 ☆：按钮变 ★（active），侧栏出现「书签」组且当前目录在最前
2. 再点 ★：目录从书签组移除（组空则整组隐藏）
3. 书签项点击跳转、悬浮出现 ×、点 × 移除且不触发跳转
4. 拖文件到书签项 = 移动/复制进该目录（沿既有 drop-target 行为）
5. 断开重连、重启客户端：书签仍在（fileManagerStorage 持久化）
6. 切换另一台服务器：书签按服务器隔离
7. 删除服务器：`fm-folder-bookmarks` 中该 serverId 数据被清理

**冒烟结果（2026-11-23，dev 客户端 + WSL quireld:8443）**

- ✅ 添加：/var/log 点 ☆ → ★；侧栏「书签」组置顶，原「常用」组按书签优先过滤消失
- ✅ 持久化：结束客户端进程并重启 → 重连后书签仍在，常用组仍被过滤
- ✅ 导航：点击书签项跳转 /var/log；书签行高亮、★ 保持
- ✅ 移除：悬浮 × 出现；UIA Invoke 触发移除 → 书签组消失、常用组恢复、★ 复位、`filemanager.bin` 中列表清空
  （坐标合成点击在 hover 生效前落到行上，属自动化时序问题；真实鼠标点击建议手工复核一次）
- ⏸ 拖拽到书签项：Cua `TARGET_OCCLUDED`（前台被其它窗口占用，AppActivate 未取得前台），未注入；
  书签项与其它侧栏项共用 `data-drop-dir` / `resolveDrop`，结构上已是放置目标，建议手工补测
- ⏸ 跨服务器隔离 / 删除服务器清理：仅单实例环境，未实机覆盖（store 单测已覆盖按 serverId 隔离与 clearServer）

---

## Self-Review 记录

- 覆盖检查：添加（工具栏 toggle）✓ 移除（悬浮 ×）✓ 分组展示与排序 ✓ 按服务器隔离 ✓ 持久化 ✓ 删除服务器清理 ✓ i18n ✓ 离线兜底 ✓
- 占位符扫描：无 TBD/TODO；所有代码步骤含完整代码
- 类型一致性：`FolderBookmark { path, addedAt }`（无 id，与「按 path 移除」一致）；store 方法名 `addFolderBookmark/removeFolderBookmark/clearServer` 在 Task 1/4/5 一致；i18n key `fileManager.bookmarks/addBookmark/removeBookmark` 在 Task 2/3/4 一致
