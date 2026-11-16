# 通知功能优化实施计划（2026-11-16）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 通知同源聚合降噪 + 通知中心未读/搜索/聚合徽标 + 清空跟随筛选视图与二次确认。

**Architecture:** 纯数据层改动集中在 `notificationStore`（新增 `kind`/`count` 与纯函数 `mergeIncoming`，60s 窗口同键合并）；展示层新增 `notificationFilter.ts` 纯函数承载筛选叠加与清空作用域；`NotificationCenter`/`ArrivalToast` 消费聚合字段。磁盘 `notifications.bin` 版本保持 1（新增字段全部可选）。

**Tech Stack:** React + TypeScript + zustand + vitest + @testing-library/react。

**Spec:** `docs/superpowers/specs/2026-11-16-notification-optimization-design.md`
**Branch:** `feature/notifications`（已含设计文档提交 9e921db）

**测试环境注意：** 本机 vitest 需 `NODE_OPTIONS=--experimental-require-module`（既有环境问题）。

---

### Task 1: notificationStore 聚合（kind/count + mergeIncoming）

**Files:**
- Modify: `src/stores/notificationStore.ts`
- Test: `src/stores/notificationStore.test.ts`

- [ ] **Step 1: 写失败测试**

在 `src/stores/notificationStore.test.ts` 末尾追加（文件已有 `n()` 构造器与 `AppNotification` 导入，复用；若缺 `mergeIncoming`/`MERGE_WINDOW_MS` 导入则补入 import）：

```ts
describe("mergeIncoming（同源聚合）", () => {
  const existing = n({ id: "n-old", count: 2, timestamp: 1000, body: "旧正文" });

  it("窗口内同 serverId/kind/source/title 且未读 → 合并：count+1、时间正文刷新、保留原 id", () => {
    const merged = mergeIncoming(existing, n({ timestamp: 1000 + MERGE_WINDOW_MS - 1 }));
    expect(merged).not.toBeNull();
    expect(merged!.id).toBe("n-old");
    expect(merged!.count).toBe(3);
    expect(merged!.timestamp).toBe(1000 + MERGE_WINDOW_MS - 1);
    expect(merged!.body).toBe("b");
    expect(merged!.live).toBe(true);
  });

  it("超窗 / 已读 / onAction / kind 不同 / 跨服务器 → 不合并", () => {
    expect(mergeIncoming(existing, n({ timestamp: 1000 + MERGE_WINDOW_MS }))).toBeNull();
    expect(mergeIncoming(n({ ...existing, read: true }), n({ timestamp: 1500 }))).toBeNull();
    expect(
      mergeIncoming(n({ ...existing, onAction: { label: "撤销", run: () => {} } }), n({ timestamp: 1500 })),
    ).toBeNull();
    // existing 无 kind（other）与 kind: archive 不同键
    expect(mergeIncoming(existing, n({ timestamp: 1500, kind: "archive" }))).toBeNull();
    expect(mergeIncoming(existing, n({ timestamp: 1500, serverId: "srv-b" }))).toBeNull();
  });

  it("details 追加去重，保留最近 30 行（溢出丢最旧）", () => {
    const merged = mergeIncoming(
      n({ ...existing, details: ["a"] }),
      n({ timestamp: 1500, details: ["a", "b"] }),
    );
    expect(merged!.details).toEqual(["a", "b"]);

    const full = n({ ...existing, details: Array.from({ length: 30 }, (_, i) => `d${i}`) });
    const overflow = mergeIncoming(full, n({ timestamp: 1500, details: ["d0", "new"] }));
    expect(overflow!.details).toHaveLength(30);
    expect(overflow!.details).toContain("new");
    expect(overflow!.details).not.toContain("d0");
  });

  it("pushNotification：60s 内同键未读通知合并为一条，原 id 保留", () => {
    useNotificationStore.setState({ notifications: [] });
    const { pushNotification } = useNotificationStore.getState();
    pushNotification({
      title: "连接中断", body: "第一次", urgency: "normal",
      source: "prod-1", kind: "connection", serverId: "srv-a",
    });
    const firstId = useNotificationStore.getState().notifications[0].id;
    pushNotification({
      title: "连接中断", body: "第二次", urgency: "normal",
      source: "prod-1", kind: "connection", serverId: "srv-a",
    });
    const list = useNotificationStore.getState().notifications;
    expect(list).toHaveLength(1);
    expect(list[0].id).toBe(firstId);
    expect(list[0].count).toBe(2);
    expect(list[0].body).toBe("第二次");
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

```bash
NODE_OPTIONS=--experimental-require-module npx vitest run src/stores/notificationStore.test.ts
```
预期：FAIL（`mergeIncoming` 未导出 / `kind` 属性不存在类型报错）。

- [ ] **Step 3: 实现 notificationStore**

`src/stores/notificationStore.ts` 修改点：

1. 类型区新增：

```ts
/** 通知类别（聚合键组成之一；旧记录缺省 other） */
export type NotificationKind = "connection" | "transfer" | "file" | "archive" | "system" | "other";
```

2. `AppNotification` 增加两个可选字段（放在 `serverName` 之后）：

```ts
  /** 通知类别：聚合键组成之一，删除服务器/按类别组织时使用；旧记录缺省 other */
  kind?: NotificationKind;
  /** 同源聚合条数（同键 60s 窗口内合并；缺省 1） */
  count?: number;
```

3. 常量与纯函数（放在 `migrateNotificationState` 之后、自增 ID 区之前）：

```ts
/** 同源合并窗口：窗口内同键未读通知合并计数，防止重复事件刷屏 */
export const MERGE_WINDOW_MS = 60_000;
/** 合并后明细行上限（与 NotificationCenter 展示上限一致） */
const MERGE_DETAILS_CAP = 30;

/**
 * 同源合并判定：键相同（serverId+kind+source+title）、existing 未读且在窗口内
 * → 返回合并后的 existing 副本（保留原 id，count+1，时间/正文/级别取最新）；
 * 否则返回 null（调用方作为新条目插入）。带 onAction 的通知永不合并——
 * 合并会用新闭包覆盖撤销动作。
 */
export function mergeIncoming(
  existing: AppNotification,
  incoming: AppNotification,
): AppNotification | null {
  if (existing.onAction) return null;
  if (existing.read) return null;
  if (existing.serverId !== incoming.serverId) return null;
  if ((existing.kind ?? "other") !== (incoming.kind ?? "other")) return null;
  if (existing.source !== incoming.source) return null;
  if (existing.title !== incoming.title) return null;
  if (incoming.timestamp - existing.timestamp >= MERGE_WINDOW_MS) return null;

  const hasDetails = existing.details != null || incoming.details != null;
  const details = hasDetails
    ? [...new Set([...(existing.details ?? []), ...(incoming.details ?? [])])].slice(-MERGE_DETAILS_CAP)
    : undefined;
  return {
    ...existing,
    timestamp: incoming.timestamp,
    body: incoming.body,
    urgency: incoming.urgency,
    count: (existing.count ?? 1) + 1,
    live: true,
    ...(incoming.action ? { action: incoming.action } : {}),
    ...(details ? { details } : {}),
  };
}
```

4. `pushNotification` 替换为：

```ts
      pushNotification: (n) =>
        set((s) => {
          const incoming: AppNotification = {
            ...n,
            id: `notif-${Date.now()}-${nextId++}`,
            timestamp: Date.now(),
            read: false,
            live: true,
            count: 1,
          };
          // 同源降噪：窗口内同键未读通知合并进已有条目（保留原 id），并置顶刷新
          const idx = s.notifications.findIndex((e) => mergeIncoming(e, incoming) !== null);
          if (idx >= 0) {
            const merged = mergeIncoming(s.notifications[idx], incoming)!;
            const rest = s.notifications.filter((_, i) => i !== idx);
            return { notifications: [merged, ...rest].slice(0, NOTIFICATION_CAP) };
          }
          return { notifications: [incoming, ...s.notifications].slice(0, NOTIFICATION_CAP) };
        }),
```

5. `partialize` 中剥离 `live` 的解构已覆盖新增字段（`{ live, ...n }` 展开保留 kind/count），无需改动。

- [ ] **Step 4: 运行测试确认通过**

```bash
NODE_OPTIONS=--experimental-require-module npx vitest run src/stores/notificationStore.test.ts
```
预期：全部 PASS（含文件原有用例）。

- [ ] **Step 5: 提交**

```bash
git add src/stores/notificationStore.ts src/stores/notificationStore.test.ts
git commit -m "feat(notify): 通知同源聚合——60s 窗口内同键未读通知合并计数"
```

---

### Task 2: 调用点 kind 标注

**Files:**
- Modify: `src/context/ServerManager.tsx`（4 处 `pushServerNotification` → `kind: "connection"`，约行 153/255/430/452）
- Modify: `src/apps/useFileTransfer.ts`（3 处 → `kind: "file"`，约行 124/134/171）
- Modify: `src/apps/FileManager.tsx`（`notifyActionError` relay，约行 384）
- Modify: `src/components/TransferStatusBar/TaskCard.tsx`（1 处 → `kind: "transfer"`，约行 155）
- Modify: `src/apps/ArchiveViewer/notifications.ts`（1 处 → `kind: "archive"`）
- Modify: `src/utils/credentials.ts`（2 处 → `kind: "system"`，约行 69/106）
- Modify: `src/context/WallpaperContext.tsx`（1 处 → `kind: "system"`，约行 96）

- [ ] **Step 1: 逐文件补 kind**

`ServerManager.tsx` 每个 `pushServerNotification(...)` 对象字面量内加一行：

```ts
          kind: "connection",
```

`useFileTransfer.ts` 三处同理加 `kind: "file",`；`TaskCard.tsx` 加 `kind: "transfer",`；
`ArchiveViewer/notifications.ts` 的 `pushServerNotification(serverId, {` 字面量内加 `kind: "archive",`；
`credentials.ts`、`WallpaperContext.tsx` 的 `pushNotification({...})` 字面量内加 `kind: "system",`。

`FileManager.tsx` 的 relay 改为（其调用方均为文件操作错误，缺省 file）：

```ts
  const notifyActionError = useCallback((n: Parameters<typeof pushServerNotification>[1]) => {
    pushServerNotification(activeServerIdRef.current, { ...n, kind: n.kind ?? "file" });
    setActionError(n.body ?? null);
  }, []);
```

`notify.ts` 无需改动：`ServerNotificationInput = Omit<AppNotification, "id"|"timestamp"|"read"|"serverId"|"serverName">`，`kind` 不在被剔除之列，自动透传。

- [ ] **Step 2: 类型检查 + 全量单测**

```bash
npx tsc --noEmit
NODE_OPTIONS=--experimental-require-module npm run test:run
```
预期：均通过（纯标注，无行为变化）。

- [ ] **Step 3: 提交**

```bash
git add src/context/ServerManager.tsx src/apps/useFileTransfer.ts src/apps/FileManager.tsx src/components/TransferStatusBar/TaskCard.tsx src/apps/ArchiveViewer/notifications.ts src/utils/credentials.ts src/context/WallpaperContext.tsx
git commit -m "feat(notify): 各调用点补通知类别标注（connection/file/transfer/archive/system）"
```

---

### Task 3: notificationFilter 纯函数

**Files:**
- Create: `src/shell/notificationFilter.ts`
- Test: `src/shell/notificationFilter.test.ts`

- [ ] **Step 1: 写失败测试**

```ts
// src/shell/notificationFilter.test.ts
import { describe, it, expect } from "vitest";
import {
  matchesSeverity,
  matchesQuery,
  applyNotificationFilters,
  buildClearFilter,
  type NotificationFilter,
} from "./notificationFilter";
import type { AppNotification } from "../stores/notificationStore";

const n = (partial: Partial<AppNotification> = {}): AppNotification => ({
  id: "n1",
  title: "连接中断",
  body: "网络断开",
  timestamp: 0,
  urgency: "normal",
  source: "prod-1",
  read: false,
  ...partial,
});

describe("matchesSeverity", () => {
  it("all 恒真；other 仅收纳未知紧急度；具体值精确匹配", () => {
    expect(matchesSeverity(n(), "all")).toBe(true);
    expect(matchesSeverity(n({ urgency: "critical" as never }), "other")).toBe(false);
    expect(matchesSeverity(n({ urgency: "weird" as NotificationFilter }), "other")).toBe(true);
    expect(matchesSeverity(n({ urgency: "critical" }), "critical")).toBe(true);
    expect(matchesSeverity(n({ urgency: "critical" }), "normal")).toBe(false);
  });
});

describe("matchesQuery", () => {
  it("title/body/source 子串匹配，不区分大小写；空/空白串恒真", () => {
    expect(matchesQuery(n(), "", "prod-1")).toBe(true);
    expect(matchesQuery(n(), "   ", "prod-1")).toBe(true);
    expect(matchesQuery(n(), "中断", "prod-1")).toBe(true);
    expect(matchesQuery(n(), "PROD", "prod-1")).toBe(true);
    expect(matchesQuery(n(), "不存在", "prod-1")).toBe(false);
  });
});

describe("applyNotificationFilters", () => {
  const pool = [
    n({ id: "1", urgency: "critical", read: false, title: "A 失败" }),
    n({ id: "2", urgency: "normal", read: true, title: "B 成功" }),
    n({ id: "3", urgency: "normal", read: false, title: "C 失败" }),
  ];
  const originOf = (x: AppNotification) => x.source;

  it("severity ∧ unread ∧ query 三者 AND 叠加", () => {
    expect(applyNotificationFilters(pool, { filter: "all", unreadOnly: false, query: "", originOf }).map((x) => x.id))
      .toEqual(["1", "2", "3"]);
    expect(applyNotificationFilters(pool, { filter: "all", unreadOnly: true, query: "", originOf }).map((x) => x.id))
      .toEqual(["1", "3"]);
    expect(applyNotificationFilters(pool, { filter: "normal", unreadOnly: true, query: "", originOf }).map((x) => x.id))
      .toEqual(["3"]);
    expect(applyNotificationFilters(pool, { filter: "all", unreadOnly: false, query: "失败", originOf }).map((x) => x.id))
      .toEqual(["1", "3"]);
    expect(applyNotificationFilters(pool, { filter: "critical", unreadOnly: true, query: "成功", originOf }))
      .toEqual([]);
  });
});

describe("buildClearFilter（清空作用域跟随筛选视图，搜索不参与）", () => {
  const pool = [
    n({ id: "1", urgency: "critical", read: false }),
    n({ id: "2", urgency: "normal", read: true }),
    n({ id: "3", urgency: "normal", read: false }),
  ];

  it("all + 未读关 → 全部命中；all + 未读开 → 仅未读；具体 chip → 仅该严重度", () => {
    const f1 = buildClearFilter({ filter: "all", unreadOnly: false });
    expect(pool.filter(f1).map((x) => x.id)).toEqual(["1", "2", "3"]);
    const f2 = buildClearFilter({ filter: "all", unreadOnly: true });
    expect(pool.filter(f2).map((x) => x.id)).toEqual(["1", "3"]);
    const f3 = buildClearFilter({ filter: "critical", unreadOnly: false });
    expect(pool.filter(f3).map((x) => x.id)).toEqual(["1"]);
  });
});
```

- [ ] **Step 2: 运行确认失败**

```bash
NODE_OPTIONS=--experimental-require-module npx vitest run src/shell/notificationFilter.test.ts
```
预期：FAIL（模块不存在）。

- [ ] **Step 3: 实现**

```ts
// src/shell/notificationFilter.ts
// 通知中心筛选纯函数：严重度 chip ∧ 未读开关 ∧ 搜索子串的叠加与清空作用域。
// 与组件解耦，便于单测；origin 由调用方注入（NotificationCenter.originOf）。

import type { AppNotification, NotificationUrgency } from "../stores/notificationStore";

/** 通知中心筛选视图（与严重度 chips 对齐） */
export type NotificationFilter = "all" | NotificationUrgency | "other";

/** 严重度 chip 匹配：all 恒真；other 兜底收纳三类之外的未知紧急度 */
export function matchesSeverity(n: AppNotification, filter: NotificationFilter): boolean {
  if (filter === "all") return true;
  if (filter === "other") {
    return n.urgency !== "critical" && n.urgency !== "normal" && n.urgency !== "low";
  }
  return n.urgency === filter;
}

/** 搜索子串匹配：title/body/来源（origin 由调用方算好传入），不区分大小写；空白串恒真 */
export function matchesQuery(n: AppNotification, query: string, origin: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return (
    n.title.toLowerCase().includes(q) ||
    n.body.toLowerCase().includes(q) ||
    origin.toLowerCase().includes(q)
  );
}

export interface NotificationFilterOptions {
  filter: NotificationFilter;
  unreadOnly: boolean;
  query: string;
  originOf: (n: AppNotification) => string;
}

/** 组合过滤：severity ∧ unread ∧ query（列表展示用，保持输入顺序） */
export function applyNotificationFilters(
  notifications: AppNotification[],
  opts: NotificationFilterOptions,
): AppNotification[] {
  return notifications.filter((n) => {
    if (!matchesSeverity(n, opts.filter)) return false;
    if (opts.unreadOnly && n.read) return false;
    if (!matchesQuery(n, opts.query, opts.originOf(n))) return false;
    return true;
  });
}

/**
 * 清空按钮的作用域（用户拍板：跟随当前筛选视图）：severity ∧ unread。
 * 搜索词不参与清空范围——清空只受 chips 与未读开关约束；「全部已读」同理不在此列。
 */
export function buildClearFilter(
  opts: { filter: NotificationFilter; unreadOnly: boolean },
): (n: AppNotification) => boolean {
  return (n) => matchesSeverity(n, opts.filter) && (!opts.unreadOnly || !n.read);
}
```

- [ ] **Step 4: 运行确认通过**

```bash
NODE_OPTIONS=--experimental-require-module npx vitest run src/shell/notificationFilter.test.ts
```
预期：PASS。

- [ ] **Step 5: 提交**

```bash
git add src/shell/notificationFilter.ts src/shell/notificationFilter.test.ts
git commit -m "feat(notify): 通知筛选纯函数——severity/unread/query 叠加与清空作用域"
```

---

### Task 4: NotificationCenter 接入 + i18n

**Files:**
- Modify: `src/shell/NotificationCenter.tsx`
- Modify: `src/shell/NotificationCenter.css`
- Modify: `src/i18n/locales/zh-CN.ts`（`notifications` 节，约行 809-835）
- Modify: `src/i18n/locales/en-US.ts`（对应 `notifications` 节）

- [ ] **Step 1: i18n 词条**

zh-CN `notifications` 节 `clearAllTitle` 之后插入：

```ts
    unreadOnly: '未读 ({{count}})',
    searchPlaceholder: '搜索通知',
    clearConfirmTitle: '清除通知',
    clearConfirmMessage: '将清除当前视图内的 {{count}} 条通知，是否继续？',
    clearConfirmDetail: '清除后无法恢复。',
```

en-US 对应位置插入：

```ts
    unreadOnly: 'Unread ({{count}})',
    searchPlaceholder: 'Search notifications',
    clearConfirmTitle: 'Clear notifications',
    clearConfirmMessage: 'Clear {{count}} notification(s) in the current view?',
    clearConfirmDetail: 'This cannot be undone.',
```

- [ ] **Step 2: 组件接入**

`NotificationCenter.tsx`：

1. 新增导入：

```ts
import { ConfirmDialog } from "../components/Dialog/ConfirmDialog";
import {
  applyNotificationFilters,
  buildClearFilter,
  type NotificationFilter,
} from "./notificationFilter";
```

2. `filter` state 类型改为导入的 `NotificationFilter`：

```ts
  const [filter, setFilter] = useState<NotificationFilter>("all");
  // 未读开关与搜索词：与严重度 chip AND 叠加；搜索不参与清空范围
  const [unreadOnly, setUnreadOnly] = useState(false);
  const [query, setQuery] = useState("");
  // 清空二次确认（ConfirmDialog；确认文案动态说明作用域与条数）
  const [confirmClear, setConfirmClear] = useState(false);
```

3. 替换 `filteredNotifications` 计算（删除原 `visibleNotifications.filter(...)` 链）：

```ts
  const filteredNotifications = applyNotificationFilters(visibleNotifications, {
    filter,
    unreadOnly,
    query,
    originOf: (x) => originOf(x, servers),
  }).sort((a, b) => b.timestamp - a.timestamp);

  // 清空作用域条数（severity ∧ unread，不含搜索词）：确认文案展示
  const clearScopeCount = visibleNotifications.filter(buildClearFilter({ filter, unreadOnly })).length;
```

4. 筛选行 chips 之后（`otherCount` 按钮的条件块后）加未读 toggle：

```tsx
          <button
            className={`nc-filter-btn ${unreadOnly ? "active" : ""}`}
            onClick={() => setUnreadOnly((v) => !v)}
          >
            {t('notifications.unreadOnly', { count: visibleNotifications.filter((n) => !n.read).length })}
          </button>
```

5. 筛选行 `</div>` 之后、列表之前加搜索框：

```tsx
        {/* 搜索：title/body/来源子串，与 chips、未读开关叠加 */}
        <div className="nc-search">
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t('notifications.searchPlaceholder')}
          />
        </div>
```

6. 卡片时间行加聚合徽标（`count > 1` 时显示，复用 nc-badge 样式）：

```tsx
                <div className="nc-notif-top">
                  <span className="nc-notif-time">
                    {formatTime(t, notif.timestamp)}
                    {(notif.count ?? 1) > 1 && (
                      <span className="nc-badge nc-notif-count">×{notif.count}</span>
                    )}
                  </span>
```

7. 清空按钮改为打开确认弹窗；确认后按作用域清除：

```tsx
            <button className="nc-action-btn nc-action-danger" onClick={() => setConfirmClear(true)} title={t('notifications.clearAllTitle')}>
```

组件 return 的最外层 div 内、`</div>`（nc-panel 结束）之后追加：

```tsx
      {confirmClear && (
        <ConfirmDialog
          danger
          title={t("notifications.clearConfirmTitle")}
          message={t("notifications.clearConfirmMessage", { count: clearScopeCount })}
          detail={t("notifications.clearConfirmDetail")}
          onConfirm={() => {
            const inScope = buildClearFilter({ filter, unreadOnly });
            clearAll((n) => isNotificationVisible(n, activeServerId) && inScope(n));
            setConfirmClear(false);
          }}
          onCancel={() => setConfirmClear(false)}
        />
      )}
```

（`clearAll` 已在现有 `useNotificationStore()` 解构中。）

- [ ] **Step 3: CSS 追加**

`NotificationCenter.css` 末尾追加（令牌与文件既有用法一致）：

```css
/* 搜索框：筛选行下方单行输入 */
.nc-search {
  padding: 0 12px 8px;
}
.nc-search input {
  width: 100%;
  height: 28px;
  padding: 0 10px;
  border: 1px solid var(--quirel-border-color);
  border-radius: var(--radius-xs);
  background: var(--quirel-view-bg);
  color: var(--quirel-text-primary);
  font-size: var(--font-small);
  outline: none;
}
.nc-search input:focus {
  border-color: var(--quirel-accent-bg);
}

/* 聚合徽标在时间行内与文本的间距（视觉复用 nc-badge 胶囊） */
.nc-notif-count {
  margin-left: 6px;
}
```

- [ ] **Step 4: 验证**

```bash
npx tsc --noEmit
NODE_OPTIONS=--experimental-require-module npm run test:run
```
预期：均通过。

- [ ] **Step 5: 提交**

```bash
git add src/shell/NotificationCenter.tsx src/shell/NotificationCenter.css src/i18n/locales/zh-CN.ts src/i18n/locales/en-US.ts
git commit -m "feat(notify): 通知中心未读过滤/搜索/聚合徽标，清空跟随筛选视图并二次确认"
```

---

### Task 5: ArrivalToast 合并更新 ×N

**Files:**
- Modify: `src/shell/ArrivalToast.tsx`
- Test: `src/shell/ArrivalToast.test.tsx`

- [ ] **Step 1: 写失败测试**

`ArrivalToast.test.tsx` 追加：

```tsx
  it("窗口内同源通知合并为一条，预览条显示 ×N 并重置计时", () => {
    const { container } = render(<ArrivalToast onOpen={() => {}} />);
    act(() => push("通知 A"));
    act(() => push("通知 A")); // 同 source+title → 合并，同 id 计数刷新
    expect(screen.getByText("通知 A")).toBeInTheDocument();
    expect(screen.getByText("×2")).toBeInTheDocument();
    expect(container.querySelector(".at-toast")).not.toBeNull();
    // 合并更新重置停留计时：再等 3.4s 仍在显示，累计到 3.5s 才进入退出动画
    act(() => vi.advanceTimersByTime(3400));
    expect(container.querySelector(".at-toast")).not.toBeNull();
    act(() => vi.advanceTimersByTime(100));
    expect(container.querySelector(".at-toast-leaving")).not.toBeNull();
  });
```

- [ ] **Step 2: 运行确认失败**

```bash
NODE_OPTIONS=--experimental-require-module npx vitest run src/shell/ArrivalToast.test.tsx
```
预期：新用例 FAIL（无 ×2；计时未重置导致提前进入 leaving）。

- [ ] **Step 3: 实现**

`ArrivalToast.tsx`：

1. refs 区新增：

```ts
  const shownStampRef = useRef<number>(0);
```

2. 监听 effect 中 `shownIdRef` 分支替换为（保持其余拦截逻辑不变）：

```ts
    if (shownIdRef.current === latest.id) {
      // 同 id 合并更新（同源降噪）：时间戳变化 → 刷新内容与 ×N、重置停留计时
      if (shownStampRef.current !== latest.timestamp) {
        shownStampRef.current = latest.timestamp;
        setCurrent(latest);
        setLeaving(false);
        setVisible(true);
        scheduleHide();
      }
      return;
    }
    shownIdRef.current = latest.id;
    shownStampRef.current = latest.timestamp;
```

3. 渲染区 `+N` 旁追加 ×N 徽标（复用 at-missed 样式）：

```tsx
      {(current.count ?? 1) > 1 && <span className="at-missed">×{current.count}</span>}
      {missed > 0 && <span className="at-missed">+{missed}</span>}
```

- [ ] **Step 4: 运行确认通过**

```bash
NODE_OPTIONS=--experimental-require-module npx vitest run src/shell/ArrivalToast.test.tsx
```
预期：全部 PASS（原「连发替换内容并累计 +N」用例不受影响——不同标题不合并）。

- [ ] **Step 5: 提交**

```bash
git add src/shell/ArrivalToast.tsx src/shell/ArrivalToast.test.tsx
git commit -m "feat(notify): 到达预览条支持同源合并更新（×N 徽标 + 计时重置）"
```

---

### Task 6: 全量验证与收尾

- [ ] **Step 1: 前端门禁**

```bash
npx tsc --noEmit
npm run verify:fe
```
预期：tsc exit 0；verify:fe 2/2 PASS（敏感日志门禁 + vitest 全量）。

- [ ] **Step 2: 全量门禁（完成声明前必跑）**

```bash
NODE_OPTIONS=--experimental-require-module npm run verify:full
```
预期：12/12 PASS。

- [ ] **Step 3: 输出完成摘要四件套并停手**

变更摘要 / 影响面 / 测试证据（含命令与结果）/ 建议 squash 提交信息
（`feat(notify): 通知同源聚合降噪与中心组织优化`）。合并由用户决定，AI 不执行。
