# 传输任务跨服务器边界 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现"连接是短暂的，任务是持久的"设计——新增 `interrupted` 传输状态、传输面板按服务器分组折叠、切换服务器时的中断确认、中断任务切回原服务器重试、启动僵尸记录降级。

**Architecture:** Rust 侧 `TransferStatus` 新增 `Interrupted` 终态；断开连接的统一清理块把非终态任务标记为 interrupted 并保留（不再删除），供切回后重试。前端 `TransferPanel` 按 `session_id` 分组：当前服务器组展开、其他组折叠为摘要行。`connectServer` 在切换且有活跃传输时用 Tauri `ask()` 确认（与现有文件覆盖确认模式一致）。

**Tech Stack:** Rust (Tauri 2, QUIC) / React + TypeScript + Zustand / Vitest + Testing Library

**设计决策（已与用户确认）：**
1. 列表组织：全局 + 按服务器分组折叠（当前服务器展开，其他折叠为摘要）
2. 有活跃传输时切换服务器：弹确认
3. 中断任务：归属即导航，点击可切回原服务器重试

**边界原则：** 服务器之间的边界画在"执行权"上，不画在"可见性"上。

---

## 背景知识（执行者必读）

- 单连接模型：同一时刻只有一个 `activeServerId`，切换 = `remote_disconnect` 旧连接（[ServerManager.tsx:320-328](../../../src/context/ServerManager.tsx)）
- Rust 统一清理块（connection.rs:571-641）：断开时唯一清理入口，已调用 `tm.cleanup_by_connection()`——本计划只改该方法行为
- `TransferTask.session_id` 在创建时被设置为 `server_id`（transfer.rs:318），前端分组直接用 `session_id`
- Rust `TransferManager.tasks` 是内存 map，重启即空 → 前端 Store 恢复的非终态任务是僵尸，需降级
- 前端测试用 Vitest（`npm run test:run`），mock 模式参照 `src/stores/serversStore.test.ts`
- **Git 规则（用户硬性约束）**：允许 `git add` 暂存，**禁止执行 `git commit` / `git push` / `git reset` 等历史操作**。每个任务收尾只暂存并提示用户提交，commit 文案在计划中给出，由用户执行。
- 命令：前端测试 `npm run test:run`（在 `e:\MyWork\gnome-remote`）；Rust 测试 `cargo test`（在 `e:\MyWork\gnome-remote\src-tauri`）；类型检查 `npx tsc --noEmit`

---

### Task 1: Rust — TransferStatus 新增 Interrupted 状态

**Files:**
- Modify: `src-tauri/src/transfer.rs:90-124`（TransferStatus 枚举及 impl）

- [ ] **Step 1: 写失败测试（文件末尾追加测试模块）**

在 `src-tauri/src/transfer.rs` 文件末尾追加：

```rust
// ── 单元测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_is_terminal() {
        // interrupted 是终态：可被 5 分钟清理任务回收，不再变更
        assert!(TransferStatus::Interrupted.is_terminal());
    }

    #[test]
    fn interrupted_is_retryable() {
        // interrupted 可重试：切回原服务器后重试
        assert!(TransferStatus::Interrupted.is_retryable());
    }

    #[test]
    fn interrupted_display_lowercase() {
        assert_eq!(TransferStatus::Interrupted.to_string(), "interrupted");
    }

    #[test]
    fn interrupted_serializes_lowercase() {
        // serde 序列化必须与前端 TS 类型字面量一致
        let json = serde_json::to_string(&TransferStatus::Interrupted).unwrap();
        assert_eq!(json, "\"interrupted\"");
    }

    #[test]
    fn active_is_not_terminal() {
        // 回归保护：非终态判断不受影响
        assert!(!TransferStatus::Active.is_terminal());
        assert!(!TransferStatus::Pending.is_terminal());
        assert!(!TransferStatus::Paused.is_terminal());
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test interrupted`（在 `src-tauri` 目录）
Expected: 编译失败，`No variant named Interrupted found for type TransferStatus`

- [ ] **Step 3: 实现枚举变体**

修改 `src-tauri/src/transfer.rs` 的 `TransferStatus` 枚举（92-99 行区域）：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferStatus {
    Pending,
    Active,
    Paused,
    Completed,
    Error,
    Cancelled,
    /// 连接断开导致的中断（区别于文件/协议错误）
    Interrupted,
}
```

修改 `is_terminal` / `is_retryable`（102-111 行区域）：

```rust
impl TransferStatus {
    /// 是否为终态（完成后不可变更）
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Error | Self::Cancelled | Self::Interrupted
        )
    }

    /// 是否可重试
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Error | Self::Cancelled | Self::Interrupted
        )
    }
}
```

修改 `Display`（113-124 行区域），在 `Cancelled` 分支后追加：

```rust
            Self::Interrupted => write!(f, "interrupted"),
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test`（在 `src-tauri` 目录）
Expected: 全部 PASS，无编译警告

- [ ] **Step 5: 暂存（不提交）**

Run: `git add src-tauri/src/transfer.rs`

提示用户提交，建议文案：`feat(transfer): TransferStatus 新增 Interrupted 终态（可重试）`

---

### Task 2: Rust — cleanup_by_connection 标记 Interrupted 并保留任务

**Files:**
- Modify: `src-tauri/src/transfer.rs:627-661`（cleanup_by_connection）

**行为变更说明：** 现状是把该连接**所有**任务（含已完成）置为 `Cancelled` 并从 map 删除。改为：只处理**非终态**任务，置为 `Interrupted` 并**保留**在 map 中（供切回服务器后 `retry_task` 使用）；终态任务不动（交给既有 5 分钟清理任务按"保留最近 10 个"回收）。

- [ ] **Step 1: 修改 cleanup_by_connection**

将 `src-tauri/src/transfer.rs` 中的 `cleanup_by_connection`（627 行起）整体替换为：

```rust
    /// 清理指定连接的所有任务
    ///
    /// 连接断开（用户切换/网络中断）时的统一清理入口：
    /// - 非终态任务 → 标记 Interrupted 并**保留**（供切回服务器后重试）
    /// - 终态任务不动（由 start_cleanup_task 的 5 分钟周期按保留策略回收）
    pub async fn cleanup_by_connection(&self, connection_id: &str) {
        // 1. 锁内标记非终态任务为 Interrupted（保留记录）
        let interrupted_tasks = {
            let mut tasks = self.tasks.lock().await;
            let mut marked = Vec::new();
            for (_id, task) in tasks.iter_mut() {
                if task.server_id == connection_id && !task.status.is_terminal() {
                    task.status = TransferStatus::Interrupted;
                    task.error = Some("连接已断开，传输已中断".to_string());
                    task.speed_bps = 0;
                    task.eta_secs = 0;
                    marked.push(task.clone());
                }
            }
            marked
        };
        // ← 锁已释放

        let count = interrupted_tasks.len();

        // 2. 发送事件（前端收到 interrupted 状态更新）
        for task in interrupted_tasks {
            let _ = self.emit_progress(&task);
        }

        tracing::info!(connection_id, count, "已中断连接的传输任务（保留记录供重试）");
    }
```

- [ ] **Step 2: 编译验证**

Run: `cargo build`（在 `src-tauri` 目录）
Expected: 编译成功。注意 `count` 变量名若触发未使用警告，确认 `tracing::info!` 已使用它。

- [ ] **Step 3: 回归测试**

Run: `cargo test`（在 `src-tauri` 目录）
Expected: 全部 PASS

说明：`cleanup_by_connection` 依赖 `AppHandle` emit，无法脱离 Tauri 运行时做纯单测，行为由 Task 9 手动冒烟验证（连接 A 传输中切换 B → A 任务显示"已中断"且保留）。

- [ ] **Step 4: 暂存（不提交）**

Run: `git add src-tauri/src/transfer.rs`

提示用户提交，建议文案：`feat(transfer): 断开连接时任务标记 Interrupted 并保留（原为 Cancelled+删除）`

---

### Task 3: Rust — get_active_transfer_count 命令

**Files:**
- Modify: `src-tauri/src/transfer.rs`（TransferManager impl 增加方法；命令区增加 command）
- Modify: `src-tauri/src/lib.rs:622-628`（invoke_handler 注册）

- [ ] **Step 1: TransferManager 增加 count_active 方法**

在 `src-tauri/src/transfer.rs` 的 `cleanup_by_connection` 方法后追加：

```rust
    /// 统计未完成传输任务数（pending/active/paused）
    ///
    /// 供前端切换服务器前的中断确认使用。
    /// paused 也计入：连接断开时暂停中的任务同样会被置为 interrupted。
    pub async fn count_active(&self) -> usize {
        let tasks = self.tasks.lock().await;
        tasks
            .values()
            .filter(|t| {
                matches!(
                    t.status,
                    TransferStatus::Pending | TransferStatus::Active | TransferStatus::Paused
                )
            })
            .count()
    }
```

- [ ] **Step 2: 增加 Tauri command**

在 `src-tauri/src/transfer.rs` 的 `cancel_transfer` 命令（2086-2098 行区域）后追加：

```rust
/// 查询未完成传输任务数（切换服务器前的中断确认）
#[command]
pub async fn get_active_transfer_count(app_handle: AppHandle) -> Result<u32, String> {
    let manager = app_handle.state::<Arc<TransferManager>>();
    Ok(manager.count_active().await as u32)
}
```

- [ ] **Step 3: lib.rs 注册命令**

在 `src-tauri/src/lib.rs` 的 invoke_handler 中，`transfer::cancel_transfer,` 之后追加一行：

```rust
            transfer::get_active_transfer_count,
```

- [ ] **Step 4: 编译 + 回归**

Run: `cargo build && cargo test`（在 `src-tauri` 目录）
Expected: 编译成功，测试全 PASS

- [ ] **Step 5: 暂存（不提交）**

Run: `git add src-tauri/src/transfer.rs src-tauri/src/lib.rs`

提示用户提交，建议文案：`feat(transfer): 新增 get_active_transfer_count 命令（切换确认依赖）`

---

### Task 4: 前端 — useTransferProgress：interrupted 类型 + 僵尸降级 + save 竞争修复

**Files:**
- Modify: `src/hooks/useTransferProgress.ts`
- Create: `src/hooks/useTransferProgress.test.ts`

- [ ] **Step 1: 写失败测试**

创建 `src/hooks/useTransferProgress.test.ts`：

```typescript
// src/hooks/useTransferProgress.test.ts
import { describe, it, expect, vi } from "vitest";

// mock Tauri 事件与存储（hook 模块顶层依赖，测试环境不可用）
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
vi.mock("../utils/transferStorage", () => ({
  transferStorage: {
    save: vi.fn(async () => {}),
    load: vi.fn(async () => []),
    clear: vi.fn(async () => {}),
  },
}));

import {
  degradeUnfinishedTasks,
  isTerminalStatus,
  TransferTask,
} from "./useTransferProgress";

const task = (overrides: Partial<TransferTask>): TransferTask => ({
  id: "t1",
  session_id: "s1",
  direction: "download",
  file_name: "a.bin",
  remote_path: "/tmp/a.bin",
  file_size: 100,
  transferred: 0,
  speed: 0,
  eta: 0,
  status: "active",
  progress: 0,
  start_time: Date.now(),
  ...overrides,
});

describe("isTerminalStatus", () => {
  it("interrupted 是终态", () => {
    expect(isTerminalStatus("interrupted")).toBe(true);
  });

  it("active/pending/paused 非终态", () => {
    expect(isTerminalStatus("active")).toBe(false);
    expect(isTerminalStatus("pending")).toBe(false);
    expect(isTerminalStatus("paused")).toBe(false);
  });
});

describe("degradeUnfinishedTasks（僵尸记录降级）", () => {
  it("非终态任务降级为 interrupted，带中断文案", () => {
    const result = degradeUnfinishedTasks([
      task({ id: "t1", status: "active" }),
      task({ id: "t2", status: "paused" }),
    ]);
    expect(result.every((t) => t.status === "interrupted")).toBe(true);
    expect(result.every((t) => t.error === "连接已断开，传输已中断")).toBe(true);
  });

  it("终态任务原样保留", () => {
    const input = [
      task({ id: "t1", status: "completed" }),
      task({ id: "t2", status: "error", error: "原错误" }),
      task({ id: "t3", status: "interrupted" }),
    ];
    expect(degradeUnfinishedTasks(input)).toEqual(input);
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `npm run test:run -- src/hooks/useTransferProgress.test.ts`
Expected: FAIL — `degradeUnfinishedTasks` / `isTerminalStatus` 未导出

- [ ] **Step 3: 实现**

修改 `src/hooks/useTransferProgress.ts`：

3a. `TransferTask` 接口的 `status` 字段（57 行区域）：

```typescript
  status: 'pending' | 'active' | 'paused' | 'completed' | 'error' | 'cancelled' | 'interrupted';
```

3b. 在 `TransferTask` 接口定义之后追加两个导出纯函数：

```typescript
/**
 * 终态判定（与 Rust 侧 TransferStatus::is_terminal 对齐）
 */
export function isTerminalStatus(status: TransferTask['status']): boolean {
  return status === 'completed' || status === 'error' || status === 'cancelled' || status === 'interrupted';
}

/**
 * 启动恢复降级：Tauri Store 里的非终态任务在 Rust 侧已不存在
 * （TransferManager 的 tasks map 是内存态，重启即空），
 * 统一降级为 interrupted，避免显示无法操作的"僵尸"活动任务。
 */
export function degradeUnfinishedTasks(tasks: TransferTask[]): TransferTask[] {
  return tasks.map(t =>
    isTerminalStatus(t.status)
      ? t
      : { ...t, status: 'interrupted' as const, error: '连接已断开，传输已中断' }
  );
}
```

3c. hook 顶部 `useState` 前增加 loaded 标志 ref（修复启动时 save([]) 覆盖 Store 的竞争）：

```typescript
export function useTransferProgress(connectionId?: string) {
  // 初始为空数组，通过 useEffect 异步从 Tauri Store 加载
  const [transfers, setTransfers] = useState<TransferTask[]>([]);
  // Store 加载完成标志：加载完成前不 save，避免空数组覆盖持久化数据
  const loadedRef = useRef(false);
```

（同时在文件顶部 import 中把 `import { useState, useEffect } from 'react';` 改为 `import { useState, useEffect, useRef } from 'react';`）

3d. 加载 effect（94-108 行区域）改为：

```typescript
  // 异步从 Tauri Store 加载历史传输任务，只保留最近24小时的
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const saved = await transferStorage.load();
      if (cancelled) return;
      const now = Date.now();
      const oneDayMs = 24 * 60 * 60 * 1000;
      const filtered = saved.filter(task => {
        const age = now - task.start_time;
        return age < oneDayMs;
      });
      // 僵尸降级：非终态任务在后端已不存在，统一降级为 interrupted
      setTransfers(degradeUnfinishedTasks(filtered));
      loadedRef.current = true;
    })();
    return () => { cancelled = true; };
  }, []);
```

3e. 保存 effect（111-113 行区域）改为：

```typescript
  // 监听变化并保存到 Tauri Store（加载完成后才启用，防止空数组覆盖）
  useEffect(() => {
    if (!loadedRef.current) return;
    transferStorage.save(transfers);
  }, [transfers]);
```

3f. `showCompletionNotification` 无需改动（`interrupted` 不会触发完成通知）。

- [ ] **Step 4: 运行测试确认通过**

Run: `npm run test:run -- src/hooks/useTransferProgress.test.ts`
Expected: PASS

- [ ] **Step 5: 类型检查**

Run: `npx tsc --noEmit`
Expected: 无错误

- [ ] **Step 6: 暂存（不提交）**

Run: `git add src/hooks/useTransferProgress.ts src/hooks/useTransferProgress.test.ts`

提示用户提交，建议文案：`feat(transfer): 前端支持 interrupted 状态，启动降级僵尸记录，修复持久化覆盖竞争`

---

### Task 5: 前端 — transferGrouping 分组纯函数

**Files:**
- Create: `src/components/TransferStatusBar/transferGrouping.ts`
- Create: `src/components/TransferStatusBar/transferGrouping.test.ts`

- [ ] **Step 1: 写失败测试**

创建 `src/components/TransferStatusBar/transferGrouping.test.ts`：

```typescript
// src/components/TransferStatusBar/transferGrouping.test.ts
import { describe, it, expect } from "vitest";
import { groupTransfersByServer, buildGroupSummary } from "./transferGrouping";
import { TransferTask } from "../../hooks/useTransferProgress";

const task = (overrides: Partial<TransferTask>): TransferTask => ({
  id: "t1",
  session_id: "s1",
  direction: "download",
  file_name: "a.bin",
  remote_path: "/tmp/a.bin",
  file_size: 100,
  transferred: 0,
  speed: 0,
  eta: 0,
  status: "active",
  progress: 0,
  start_time: 1000,
  ...overrides,
});

const servers = [
  { id: "s1", name: "web-01", host: "1.1.1.1" },
  { id: "s2", name: "db-01", host: "2.2.2.2" },
];

describe("groupTransfersByServer", () => {
  it("按 session_id 分组并解析服务器名", () => {
    const groups = groupTransfersByServer(
      [
        task({ id: "t1", session_id: "s1" }),
        task({ id: "t2", session_id: "s2" }),
        task({ id: "t3", session_id: "s1" }),
      ],
      servers,
      "s1"
    );
    expect(groups).toHaveLength(2);
    const g1 = groups.find((g) => g.serverId === "s1")!;
    expect(g1.serverName).toBe("web-01");
    expect(g1.tasks).toHaveLength(2);
    expect(g1.isCurrent).toBe(true);
  });

  it("当前服务器组排在最前", () => {
    const groups = groupTransfersByServer(
      [
        task({ id: "t1", session_id: "s2", start_time: 2000 }),
        task({ id: "t2", session_id: "s1", start_time: 1000 }),
      ],
      servers,
      "s1"
    );
    expect(groups[0].serverId).toBe("s1");
  });

  it("服务器已删除 → 兜底「未知服务器」", () => {
    const groups = groupTransfersByServer(
      [task({ session_id: "gone" })],
      servers,
      null
    );
    expect(groups[0].serverName).toBe("未知服务器");
  });

  it("服务器无 name 时回退 host", () => {
    const groups = groupTransfersByServer(
      [task({ session_id: "s3" })],
      [...servers, { id: "s3", name: "", host: "3.3.3.3" }],
      null
    );
    expect(groups[0].serverName).toBe("3.3.3.3");
  });

  it("非当前组按最近任务时间倒序", () => {
    const groups = groupTransfersByServer(
      [
        task({ id: "old", session_id: "s2", start_time: 1000 }),
        task({ id: "new", session_id: "s3", start_time: 9000 }),
      ],
      servers,
      null
    );
    // 无当前服务器时，全部按最近任务排序
    expect(groups[0].serverId).toBe("s3");
    expect(groups[1].serverId).toBe("s2");
  });
});

describe("buildGroupSummary", () => {
  it("聚合各状态计数", () => {
    const summary = buildGroupSummary([
      task({ status: "interrupted" }),
      task({ status: "interrupted" }),
      task({ status: "completed" }),
    ]);
    expect(summary).toBe("2 个已中断 · 1 个已完成");
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `npm run test:run -- src/components/TransferStatusBar/transferGrouping.test.ts`
Expected: FAIL — 模块不存在

- [ ] **Step 3: 实现**

创建 `src/components/TransferStatusBar/transferGrouping.ts`：

```typescript
/**
 * 传输任务按服务器分组的纯函数
 *
 * 设计：传输列表全局唯一（跨服务器可见），按 session_id（即 server_id）分组；
 * 当前活跃服务器的组在前且默认展开，其他服务器折叠为摘要行。
 * 边界原则：服务器之间的边界画在"执行权"上，不画在"可见性"上。
 */

import { TransferTask } from '../../hooks/useTransferProgress';
import { ServerConfig } from '../../stores/serversStore';

/** 分组结果 */
export interface TransferGroup {
  serverId: string;
  serverName: string;
  /** 是否为当前活跃服务器的组 */
  isCurrent: boolean;
  tasks: TransferTask[];
  /** 折态摘要，如 "2 个已中断 · 1 个已完成" */
  summary: string;
}

/** 状态中文标签（与 TaskCard 显示一致） */
const STATUS_LABELS: Record<TransferTask['status'], string> = {
  active: '进行中',
  pending: '排队中',
  paused: '已暂停',
  completed: '已完成',
  error: '失败',
  cancelled: '已取消',
  interrupted: '已中断',
};

/**
 * 按 session_id（= server_id）分组
 * - 当前活跃服务器的组排在最前
 * - 其余组按组内最近任务的 start_time 倒序
 * - 服务器已被删除时兜底显示「未知服务器」
 */
export function groupTransfersByServer(
  transfers: TransferTask[],
  servers: Pick<ServerConfig, 'id' | 'name' | 'host'>[],
  activeServerId: string | null
): TransferGroup[] {
  // 按 session_id 分组（保持插入顺序）
  const byServer = new Map<string, TransferTask[]>();
  for (const t of transfers) {
    const list = byServer.get(t.session_id) ?? [];
    list.push(t);
    byServer.set(t.session_id, list);
  }

  const groups: TransferGroup[] = [];
  for (const [serverId, tasks] of byServer) {
    const server = servers.find((s) => s.id === serverId);
    groups.push({
      serverId,
      serverName: server?.name || server?.host || '未知服务器',
      isCurrent: serverId === activeServerId,
      tasks,
      summary: buildGroupSummary(tasks),
    });
  }

  groups.sort((a, b) => {
    // 当前服务器组置顶
    if (a.isCurrent !== b.isCurrent) return a.isCurrent ? -1 : 1;
    // 其余按组内最近任务时间倒序（新的在前）
    const aLatest = Math.max(...a.tasks.map((t) => t.start_time));
    const bLatest = Math.max(...b.tasks.map((t) => t.start_time));
    return bLatest - aLatest;
  });

  return groups;
}

/** 聚合组内各状态计数，生成摘要文案 */
export function buildGroupSummary(tasks: TransferTask[]): string {
  const counts = new Map<TransferTask['status'], number>();
  for (const t of tasks) {
    counts.set(t.status, (counts.get(t.status) ?? 0) + 1);
  }
  return [...counts.entries()]
    .map(([status, n]) => `${n} 个${STATUS_LABELS[status]}`)
    .join(' · ');
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `npm run test:run -- src/components/TransferStatusBar/transferGrouping.test.ts`
Expected: PASS

- [ ] **Step 5: 类型检查 + 暂存（不提交）**

Run: `npx tsc --noEmit`，Expected: 无错误

Run: `git add src/components/TransferStatusBar/transferGrouping.ts src/components/TransferStatusBar/transferGrouping.test.ts`

提示用户提交，建议文案：`feat(transfer): 传输任务按服务器分组的纯函数（含测试）`

---

### Task 6: 前端 — TransferPanel 分组渲染 + 折叠

**Files:**
- Modify: `src/components/TransferStatusBar/TransferPanel.tsx`
- Modify: `src/components/TransferStatusBar/TransferPanel.css`

- [ ] **Step 1: 修改 TransferPanel.tsx**

1. import 区追加：

```typescript
import { useState, useEffect, useRef } from 'react';
import { useServersStore } from '../../stores/serversStore';
import { groupTransfersByServer } from './transferGrouping';
```

2. 组件内（`const panelRef = ...` 之后）增加分组状态：

```typescript
  // 服务器分组：当前服务器的组默认展开，其他默认折叠
  const servers = useServersStore((s) => s.servers);
  const activeServerId = useServersStore((s) => s.activeServerId);
  const groups = groupTransfersByServer(visibleTransfers, servers, activeServerId);

  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(new Set());

  // 活跃服务器变化时重置默认展开（只展开当前服务器）
  useEffect(() => {
    setExpandedGroups(activeServerId ? new Set([activeServerId]) : new Set());
  }, [activeServerId]);

  const toggleGroup = (serverId: string) => {
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(serverId)) {
        next.delete(serverId);
      } else {
        next.add(serverId);
      }
      return next;
    });
  };
```

3. 把现有 `sortedTransfers` 的排序 map 改造为可复用的组内排序函数（`statusOrder` 加 `interrupted: 6`）：

```typescript
  /**
   * 组内排序
   * 优先级：活动 → 排队 → 已暂停 → 已完成 → 失败 → 已取消 → 已中断
   */
  const sortTasks = (tasks: TransferTask[]) => {
    const statusOrder = {
      active: 0,
      pending: 1,
      paused: 2,
      completed: 3,
      error: 4,
      cancelled: 5,
      interrupted: 6,
    };
    return [...tasks].sort((a, b) => statusOrder[a.status] - statusOrder[b.status]);
  };
```

（删除原 `sortedTransfers` 变量，后续渲染不再直接使用它。）

4. 替换任务列表渲染区（`.tp-list` 内部的 `sortedTransfers.length === 0 ? ... : sortedTransfers.map(...)`）：

```tsx
      {/* 任务列表（按服务器分组，当前服务器组展开，其他折叠） */}
      <div className="tp-list">
        {groups.length === 0 ? (
          <div className="tp-empty" role="status" aria-live="polite">
            <div className="tp-empty-icon">
              <DownloadIcon />
            </div>
            <p className="tp-empty-text">暂无传输任务</p>
          </div>
        ) : (
          groups.map((group) => {
            const expanded = expandedGroups.has(group.serverId);
            return (
              <div
                key={group.serverId}
                className={`tp-group${group.isCurrent ? ' current' : ''}`}
              >
                <button
                  className="tp-group-header"
                  onClick={() => toggleGroup(group.serverId)}
                  aria-expanded={expanded}
                  aria-label={`${group.serverName} 的传输任务（${group.summary}）`}
                >
                  <span className={`tp-group-dot${group.isCurrent ? ' active' : ''}`} />
                  <span className="tp-group-name">{group.serverName}</span>
                  {!expanded && <span className="tp-group-summary">{group.summary}</span>}
                  <span className="tp-group-toggle">
                    {expanded ? '▾' : '▸'}
                  </span>
                </button>
                {expanded && (
                  <div className="tp-group-tasks">
                    {sortTasks(group.tasks).map((task) => (
                      <TaskCard
                        key={task.id}
                        task={task}
                        onRemove={onRemoveTask}
                      />
                    ))}
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>
```

说明：折叠指示符用文本箭头 `▾`/`▸`（与项目面包屑导航的 `▸` 分隔符风格一致），无需新增 SVG。

- [ ] **Step 2: 追加 CSS**

在 `src/components/TransferStatusBar/TransferPanel.css` 末尾追加（沿用 `tp-` 前缀与现有变量）：

```css
/* ── 服务器分组 ─────────────────────────────────────────────── */

.tp-group {
  margin-bottom: 4px;
}

.tp-group-header {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 6px 10px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: inherit;
  font-size: 12px;
  cursor: pointer;
  text-align: left;
}

.tp-group-header:hover {
  background: var(--quirel-hover-bg, rgba(128, 128, 128, 0.12));
}

/* 服务器颜色点：当前服务器高亮 accent，其余灰色 */
.tp-group-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
  background: var(--quirel-dimmed-color, #9a9996);
}

.tp-group-dot.active {
  background: var(--quirel-accent-bg, #3584e4);
}

.tp-group-name {
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tp-group-summary {
  color: var(--quirel-dimmed-color, #9a9996);
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tp-group-toggle {
  margin-left: auto;
  flex-shrink: 0;
  color: var(--quirel-dimmed-color, #9a9996);
  font-size: 11px;
}

.tp-group-tasks {
  padding-left: 4px;
}
```

- [ ] **Step 3: 类型检查 + 回归**

Run: `npx tsc --noEmit && npm run test:run`
Expected: 类型无错误；测试无新增失败（既有 6 个 Terminal/WindowManagerContext mock 失败属已知基线）

- [ ] **Step 4: 暂存（不提交）**

Run: `git add src/components/TransferStatusBar/TransferPanel.tsx src/components/TransferStatusBar/TransferPanel.css`

提示用户提交，建议文案：`feat(transfer): 传输面板按服务器分组折叠（当前服务器展开）`

---

### Task 7: 前端 — TaskCard interrupted 状态 + 切回重试

**Files:**
- Modify: `src/components/TransferStatusBar/TaskCard.tsx`
- Modify: `src/components/TransferStatusBar/TaskCard.css`
- Create: `src/components/TransferStatusBar/TaskCard.test.tsx`

- [ ] **Step 1: 写失败测试**

创建 `src/components/TransferStatusBar/TaskCard.test.tsx`：

```typescript
// src/components/TransferStatusBar/TaskCard.test.tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

// mock Tauri invoke
const invokeMock = vi.fn(async () => {});
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// mock ServerManager context：返回固定 activeServerId 与可 spy 的 connectServer
const connectServerMock = vi.fn(async () => {});
vi.mock("../../context/ServerManager", () => ({
  useServerManager: () => ({
    activeServerId: "s-current",
    connectServer: connectServerMock,
  }),
}));

// mock useTransferProgress（TaskCard 只用其类型）
vi.mock("../../hooks/useTransferProgress", () => ({}));

import { TaskCard } from "./TaskCard";
import { TransferTask } from "../../hooks/useTransferProgress";

const interruptedTask: TransferTask = {
  id: "t1",
  session_id: "s-other",
  direction: "download",
  file_name: "big.tar.gz",
  remote_path: "/tmp/big.tar.gz",
  file_size: 100,
  transferred: 40,
  speed: 0,
  eta: 0,
  status: "interrupted",
  error: "连接已断开，传输已中断",
  progress: 40,
  start_time: Date.now(),
};

describe("TaskCard · interrupted 状态", () => {
  beforeEach(() => {
    invokeMock.mockClear();
    connectServerMock.mockClear();
  });

  it("渲染「已中断」状态文案", () => {
    render(<TaskCard task={interruptedTask} onRemove={() => {}} />);
    expect(screen.getByText("已中断")).toBeTruthy();
  });

  it("渲染「切回服务器并重试」按钮", () => {
    render(<TaskCard task={interruptedTask} onRemove={() => {}} />);
    expect(screen.getByRole("button", { name: "切回服务器并重试" })).toBeTruthy();
  });

  it("点击重试：先切回原服务器，再调 retry_transfer", async () => {
    render(<TaskCard task={interruptedTask} onRemove={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "切回服务器并重试" }));
    // 等待异步链完成
    await vi.waitFor(() => {
      expect(connectServerMock).toHaveBeenCalledWith("s-other");
      expect(invokeMock).toHaveBeenCalledWith("retry_transfer", { taskId: "t1" });
    });
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `npm run test:run -- src/components/TransferStatusBar/TaskCard.test.tsx`
Expected: FAIL — 找不到「已中断」/「切回服务器并重试」

- [ ] **Step 3: 修改 TaskCard.tsx**

3a. import 区追加：

```typescript
import { useServerManager } from '../../context/ServerManager';
import { useServersStore } from '../../stores/serversStore';
```

3b. 组件内（`const startTimeText = ...` 之前）获取 context：

```typescript
export function TaskCard({ task, onRemove }: TaskCardProps) {
  const { activeServerId, connectServer } = useServerManager();
  const startTimeText = formatTime(task.start_time);
```

3c. 在 `handleRetry` 之后追加两个函数：

```typescript
  /**
   * 切回原服务器并重试中断的任务（归属即导航）
   * - 任务属于当前服务器：直接重试
   * - 任务属于其他服务器：先切换连接（若当前有活跃传输会触发中断确认），
   *   连接成功后重试；连接失败则终止（错误通知已由 connectServer 发出）
   * - 后端任务记录已被清理（重启/超出保留数量）：回退为重新发起传输
   */
  const handleSwitchAndRetry = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      if (task.session_id !== activeServerId) {
        await connectServer(task.session_id);
        // 非响应式读取最新连接状态，连接失败则不重试
        const server = useServersStore
          .getState()
          .servers.find((s) => s.id === task.session_id);
        if (server?.status !== 'connected') {
          log.warn('切回服务器失败，取消重试:', task.session_id);
          return;
        }
      }
      await invoke('retry_transfer', { taskId: task.id });
      log.info('已重试中断的传输:', task.id);
    } catch (error) {
      // retry_transfer 报"任务不存在"：后端记录已丢失，用原始参数重新发起
      log.warn('重试失败，尝试重新发起传输:', error);
      await restartTransfer();
    }
  };

  /**
   * 用任务保存的原始参数重新发起传输（后端记录丢失时的兜底）
   */
  const restartTransfer = async () => {
    try {
      await invoke('transfer_file', {
        serverId: task.session_id,
        direction: task.direction,
        remotePath: task.remote_path,
        localPath: task.local_path,
      });
      // 新任务已由 transfer-progress 事件进入列表，移除旧的中断记录
      onRemove(task.id);
    } catch (error) {
      log.error('重新发起传输失败:', error);
    }
  };
```

3d. `getProgressClass` 追加 interrupted 分支（在 cancelled 之后）：

```typescript
    if (task.status === 'cancelled') return 'tc-progress-fill cancelled';
    if (task.status === 'interrupted') return 'tc-progress-fill interrupted';
```

3e. 进度信息区（`tc-middle` 内，error 显示之后）追加 interrupted 状态文案：

```tsx
        {task.status === 'interrupted' && (
          <span className="tc-error-msg" title="连接已断开，传输已中断">已中断</span>
        )}
```

3f. 操作按钮区（`cancelled` 块之后、`pending` 块之前）追加：

```tsx
        {/* 已中断任务：切回服务器重试 + 关闭 */}
        {task.status === 'interrupted' && (
          <>
            <button
              className="tc-action-btn"
              onClick={handleSwitchAndRetry}
              aria-label="切回服务器并重试"
              title="切回服务器并重试"
            >
              <RefreshIcon />
            </button>
            <button
              className="tc-action-btn"
              onClick={handleClose}
              aria-label="关闭任务"
              title="关闭"
            >
              <CloseIcon />
            </button>
          </>
        )}
```

注意：`handleClose` 中"取消传输"的判断条件保持现状（active/paused 才 cancel），interrupted 任务直接移除记录即可——Rust 侧无对应任务时 `cancel_transfer` 不会被执行。

- [ ] **Step 4: 追加 TaskCard.css**

在 `src/components/TransferStatusBar/TaskCard.css` 中 `.tc-progress-fill.cancelled` 样式之后追加（若无对应规则则在进度条样式区末尾追加）：

```css
/* 已中断：警示色（区别于 error 的红色） */
.tc-progress-fill.interrupted {
  background: var(--quirel-warning-color, #f5c211);
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `npm run test:run -- src/components/TransferStatusBar/TaskCard.test.tsx`
Expected: PASS

- [ ] **Step 6: 类型检查 + 暂存（不提交）**

Run: `npx tsc --noEmit`，Expected: 无错误

Run: `git add src/components/TransferStatusBar/TaskCard.tsx src/components/TransferStatusBar/TaskCard.css src/components/TransferStatusBar/TaskCard.test.tsx`

提示用户提交，建议文案：`feat(transfer): 中断任务显示与「切回服务器重试」（含记录丢失兜底）`

---

### Task 8: 前端 — ServerManager 切换服务器中断确认

**Files:**
- Modify: `src/context/ServerManager.tsx:311-345`（connectServer）

说明：确认对话框使用 Tauri `ask()` 系统对话框——与 FileManager 现有"确认覆盖"（FileManager.tsx:1118）模式一致，不新建自定义模态组件。

- [ ] **Step 1: 修改 connectServer**

1. import 区追加：

```typescript
import { ask } from "@tauri-apps/plugin-dialog";
```

2. `connectServer` 回调体开头（`const server = servers.find(...)` 的空值检查之后、"断开当前连接"注释之前）插入确认逻辑：

```typescript
  const connectServer = useCallback(async (id: string) => {
    log.info("开始连接服务器:", id);
    const server = servers.find((s) => s.id === id);
    if (!server) {
      log.error("未找到服务器:", id);
      return;
    }

    // 切换目标 ≠ 当前连接 且有未完成传输 → 确认中断（显式契约：
    // 用户知道切换有代价；任务随后会被统一清理块标记为 interrupted）
    if (activeServerId && activeServerId !== id) {
      try {
        const activeCount = await invoke<number>("get_active_transfer_count");
        if (activeCount > 0) {
          const target = servers.find((s) => s.id === id);
          const confirmed = await ask(
            `当前有 ${activeCount} 个传输任务正在进行，切换到「${target?.name || target?.host}」将中断它们。`,
            {
              title: "切换服务器",
              kind: "warning",
              okLabel: "切换",
              cancelLabel: "取消",
            }
          );
          if (!confirmed) {
            log.info("用户取消切换（有传输进行中）");
            return;
          }
        }
      } catch (e) {
        // 查询失败不阻塞切换（确认是增强，不是门禁）
        log.warn("查询活跃传输数失败，跳过确认:", e);
      }
    }

    // ……（以下保持现有逻辑不变：断开旧连接 → setServerStatus → performConnect）
```

deps 数组无需新增依赖（`servers`、`activeServerId` 已在 deps 中）。

- [ ] **Step 2: 类型检查 + 回归**

Run: `npx tsc --noEmit && npm run test:run`
Expected: 类型无错误；测试无新增失败

说明：确认分支依赖 Tauri invoke/ask 与连接时序，不做组件级单测，由 Task 9 手动冒烟覆盖（用户取消 → 不切换；确认 → 切换且任务显示已中断）。

- [ ] **Step 3: 暂存（不提交）**

Run: `git add src/context/ServerManager.tsx`

提示用户提交，建议文案：`feat(connection): 有活跃传输时切换服务器需确认中断`

---

### Task 9: 全量验证 + 手动冒烟

**Files:** 无代码改动（验证任务）

- [ ] **Step 1: Rust 全量测试**

Run: `cargo test`（在 `src-tauri` 目录）
Expected: 全部 PASS（含 Task 1 新增的 5 个测试）

- [ ] **Step 2: 前端全量测试**

Run: `npm run test:run`
Expected: 失败集合与既有基线一致（此前已知 6 个 Terminal/WindowManagerContext mock 失败），无新增失败

- [ ] **Step 3: 类型检查**

Run: `npx tsc --noEmit`
Expected: 无错误

- [ ] **Step 4: 手动冒烟清单**

需要两台可连接的服务器（A、B）和一个大文件（如 100MB+）：

1. **传输中切换（确认弹窗）**：连接 A，从文件管理器下载大文件 → 传输中切换到 B → 应弹确认「当前有 1 个传输任务正在进行，切换到「B」将中断它们」→ 点「取消」→ 不切换，下载继续
2. **确认切换**：再次切换 B → 确认 → A 的任务变为「已中断」（黄色进度条 + "已中断"文案），不再显示"读取数据帧超时"之类实现细节
3. **分组折叠**：展开传输面板 → A 的组折叠为摘要行（"1 个已中断"）；当前服务器 B 的组（若有任务）展开。点击 A 组头可展开
4. **切回重试**：点 A 组任务的重试按钮 → 切回 A（此时 B 无活跃传输，不再弹确认）→ 任务重试，进度继续（transferred > 0 时走断点续传）
5. **切回时反向确认**：B 上先发起一个下载，再从 A 组点重试 → 应弹「切换将中断」确认（中断当前 B 的传输）
6. **僵尸降级**：A 有 active 任务时重启应用 → 面板中该任务显示为「已中断」而非卡在 active
7. **被动断开**：A 下载中直接断网 → 任务同样显示「已中断」（统一清理块路径）
8. **记录丢失兜底**：重启应用后（Rust map 已空）点中断任务的重试 → 走 transfer_file 重新发起，旧记录被移除

- [ ] **Step 5: 暂存遗留改动（如有）并汇报**

Run: `git status`
Expected: 工作区干净（全部改动已在前续任务暂存）；如有遗漏文件按任务归属补 `git add`

---

## Self-Review 记录

- **Spec 覆盖**：①interrupted 状态（Task 1/2/4/7）✓ ②分组折叠（Task 5/6）✓ ③切换确认（Task 3/8）✓ ④切回重试（Task 7）✓ ⑤僵尸降级（Task 4）✓ ⑥save 覆盖竞争（Task 4，讨论中发现的顺带修复）✓
- **类型一致性**：Rust `serde lowercase "interrupted"` ↔ TS `'interrupted'` 字面量 ↔ `statusOrder.interrupted: 6` ↔ `STATUS_LABELS.interrupted`，三处已对齐；`get_active_transfer_count` 返回 `u32`，前端 `invoke<number>` 对应
- **已知边界**：`cleanup_by_connection` 与确认弹窗无单测（依赖 AppHandle/系统对话框），由 Task 9 冒烟覆盖；多 FileManager 窗口共用同一 Store 的多实例竞争是既有问题，不在本计划范围
