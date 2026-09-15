# 通知 UI 与连接失败画面重设计 · 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 通知中心视觉升级（Symbolic SVG + 令牌化）+ 新通知到达预览条 + 应用内连接错误态/重连态统一组件。

**Architecture:** 纯前端呈现层改造。数据层仅一处结构化变更（`ServerConfig.error` 从 string 扩展为 `StructuredError | string`），新增 `getServerErrorInfo` 查表辅助；三个新组件（SymbolicIcon / ConnectionState / ArrivalToast）+ 通知中心呈现层重写 + 三个应用接入。

**Tech Stack:** React 18 + TypeScript + zustand + CSS（无新依赖）

**规格**：`docs/superpowers/specs/2026-09-15-notification-ui-redesign-design.md`

**与规格的两处偏差**（执行时按本计划为准）：

1. **语义令牌单套定义**（规格写"亮暗两套"）：项目主题系统通过 TS 主题对象切换 `--quirel-*`，但状态色（`--quirel-error-color` 等）本就是 variables.css 单套定义。语义令牌跟随此既有模式；`-bg` 用半透明 rgba，亮暗背景均可用。
2. **ReconnectingState 不显示"第 n/3 次"**：attempt 计数不在 store 中，为避免 `setServerStatus` 签名扩散，仅显示"正在重新连接…"。

**Git 约定（用户规则）**：执行者**不得**执行 `git commit`。每个 Task 完成后仅提示用户"建议提交检查点"，由用户自行操作。

**测试环境**：vitest + jsdom + @testing-library/react（`vitest.config.ts` 已配置）。运行单测：`npm run test:run -- <path>`。既有失败（Terminal.test.tsx ×4 缺 WindowManagerProvider、WindowManagerContext.test.tsx ×2 mock 缺 create）为历史遗留，非本计划范围——**本计划的新增测试必须全部通过**。

---

### Task 1: 语义令牌增补 + getStatusColor 令牌化

**Files:**
- Modify: `src/styles/variables.css`（Status Colors 块，约 32-35 行）
- Modify: `src/stores/serversStore.ts:157-165`

- [x] **Step 1: variables.css 增补语义令牌**

在 `:root` 的 Status Colors 块后追加（保持既有三个 `*-color` 不动，别处仍在用）：

```css
  /* Status Colors */
  --quirel-success-color:    #26a269;      /* Adwaita Green - 成功状态 */
  --quirel-error-color:      #e01b24;      /* Adwaita Red - 错误状态 */
  --quirel-warning-color:    #f5c211;      /* Adwaita Yellow - 警告状态 */

  /* ── 语义严重度令牌（通知/错误 UI 统一）────────────── */
  /* urgency 映射：critical→danger / normal→warning / low→success */
  /* 单套定义（与 Status Colors 同模式）；-bg 半透明，亮暗主题通用 */
  --quirel-danger-fg:        #c01c28;      /* 深红，亮背景可读 */
  --quirel-danger-bg:       rgba(224, 27, 36, 0.08);
  --quirel-danger-border:   rgba(224, 27, 36, 0.35);
  --quirel-warning-fg:      #9c5b00;      /* 深橙，亮背景可读 */
  --quirel-warning-bg:      rgba(245, 194, 17, 0.14);
  --quirel-warning-border:  rgba(245, 194, 17, 0.45);
  --quirel-success-fg:      #1b7f4d;      /* 深绿，亮背景可读 */
  --quirel-success-bg:      rgba(38, 162, 105, 0.10);
  --quirel-success-border:  rgba(38, 162, 105, 0.35);
```

- [x] **Step 2: getStatusColor 返回 CSS var**

替换 `src/stores/serversStore.ts` 的 `getStatusColor`（消费方 TopBar.tsx:57、Settings.tsx:629/689 均用于 inline style background，CSS var 合法）：

```ts
export function getStatusColor(status: ServerConfig["status"]): string {
  switch (status) {
    case "connected": return "var(--quirel-success-color)";
    case "connecting": return "var(--quirel-warning-color)";
    case "reconnecting": return "#ff7800";
    case "disconnected": return "#9a9996";
    case "error": return "var(--quirel-error-color)";
  }
}
```

- [x] **Step 3: 验证**

Run: `npx tsc --noEmit`
Expected: 零错误

- [x] **Step 4: 检查点**

提示用户：Task 1 完成，建议提交。

---

### Task 2: SymbolicIcon 图标组件

**Files:**
- Create: `src/components/symbolic.tsx`
- Test: `src/components/symbolic.test.tsx`

- [x] **Step 1: 写失败测试**

```tsx
// src/components/symbolic.test.tsx
import { render } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { SymbolicIcon } from "./symbolic";

const ALL_NAMES = [
  "dialog-error", "dialog-warning", "dialog-information",
  "view-refresh", "network-offline",
  "mail-unread", "mailbox",
  "emblem-ok", "user-trash", "window-close",
  "bell", "bell-outline",
] as const;

describe("SymbolicIcon", () => {
  it("按 name 渲染 svg，尺寸生效", () => {
    const { container } = render(<SymbolicIcon name="dialog-error" size={20} />);
    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    expect(svg?.getAttribute("width")).toBe("20");
    expect(svg?.getAttribute("viewBox")).toBe("0 0 16 16");
  });

  it("使用 currentColor（颜色由父级继承）", () => {
    const { container } = render(<SymbolicIcon name="bell" />);
    expect(
      container.querySelector('svg [stroke="currentColor"], svg [fill="currentColor"]')
    ).not.toBeNull();
  });

  it("全部图标可渲染", () => {
    for (const name of ALL_NAMES) {
      const { container } = render(<SymbolicIcon name={name} />);
      expect(container.querySelector("svg")).not.toBeNull();
    }
  });

  it("未知图标回退占位不崩溃", () => {
    const { container } = render(<SymbolicIcon name={"no-such" as never} />);
    expect(container.querySelector("svg")).not.toBeNull();
  });
});
```

- [x] **Step 2: 运行确认失败**

Run: `npm run test:run -- src/components/symbolic.test.tsx`
Expected: FAIL（找不到模块 ./symbolic）

- [x] **Step 3: 实现 symbolic.tsx**

```tsx
// src/components/symbolic.tsx
// Adwaita Symbolic 风格内联 SVG 图标集
// 统一 16×16 viewBox；fill/stroke = currentColor，颜色由父元素 className/style 控制

import type { CSSProperties, ReactNode } from "react";

export type SymbolicIconName =
  | "dialog-error" | "dialog-warning" | "dialog-information"
  | "view-refresh" | "network-offline"
  | "mail-unread" | "mailbox"
  | "emblem-ok" | "user-trash" | "window-close"
  | "bell" | "bell-outline";

/** 图标内容（16×16 网格，几何近似 Adwaita symbolic 造型） */
const ICONS: Record<SymbolicIconName, ReactNode> = {
  // 圆环 + 叹号
  "dialog-error": (<>
    <circle cx="8" cy="8" r="6.25" fill="none" stroke="currentColor" strokeWidth="1.5" />
    <rect x="7.25" y="4.2" width="1.5" height="4.8" rx="0.75" fill="currentColor" />
    <rect x="7.25" y="10.4" width="1.5" height="1.5" rx="0.75" fill="currentColor" />
  </>),
  // 三角 + 叹号
  "dialog-warning": (<>
    <path d="M8 2.2 L14.3 13.3 a0.8 0.8 0 0 1 -0.7 1.2 H2.4 a0.8 0.8 0 0 1 -0.7 -1.2 Z"
      fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" />
    <rect x="7.25" y="5.8" width="1.5" height="3.6" rx="0.75" fill="currentColor" />
    <rect x="7.25" y="10.4" width="1.5" height="1.5" rx="0.75" fill="currentColor" />
  </>),
  // 圆环 + i
  "dialog-information": (<>
    <circle cx="8" cy="8" r="6.25" fill="none" stroke="currentColor" strokeWidth="1.5" />
    <rect x="7.25" y="4.2" width="1.5" height="1.5" rx="0.75" fill="currentColor" />
    <rect x="7.25" y="6.8" width="1.5" height="4.8" rx="0.75" fill="currentColor" />
  </>),
  // 环形箭头（重试/刷新）
  "view-refresh": (<>
    <path d="M13.2 8 a5.2 5.2 0 1 1 -1.5 -3.7" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    <path d="M13.9 1.6 v3.6 h-3.6" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
  </>),
  // 显示器 + 斜杠（断开）
  "network-offline": (<>
    <rect x="1.8" y="2.2" width="12.4" height="8.4" rx="1.2" fill="none" stroke="currentColor" strokeWidth="1.5" />
    <path d="M5.5 13.4 h5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    <path d="M3.4 3.4 L12.6 12.6" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
  </>),
  // 信封 + 未读点
  "mail-unread": (<>
    <rect x="1.5" y="4" width="13" height="9" rx="1.2" fill="none" stroke="currentColor" strokeWidth="1.5" />
    <path d="M2.2 4.8 L8 9 L13.8 4.8" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" />
    <circle cx="13.2" cy="2.8" r="1.8" fill="currentColor" />
  </>),
  // 信封
  "mailbox": (<>
    <rect x="1.5" y="3.5" width="13" height="9.5" rx="1.2" fill="none" stroke="currentColor" strokeWidth="1.5" />
    <path d="M2.2 4.3 L8 8.7 L13.8 4.3" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" />
  </>),
  // 对勾
  "emblem-ok": (<>
    <path d="M3 8.6 L6.5 12 L13 4.6" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
  </>),
  // 垃圾桶
  "user-trash": (<>
    <path d="M2.8 4.6 h10.4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    <path d="M6.6 2.4 h2.8 v1.4 h-2.8 Z" fill="currentColor" />
    <path d="M4 4.6 v8 a1.4 1.4 0 0 0 1.4 1.4 h5.2 a1.4 1.4 0 0 0 1.4 -1.4 v-8"
      fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" />
    <path d="M6.5 7 v4.5 M9.5 7 v4.5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
  </>),
  // X
  "window-close": (<>
    <path d="M4 4 L12 12 M12 4 L4 12" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
  </>),
  // 铃铛（实底）
  "bell": (<>
    <path d="M8 1.6 c-2.4 0 -3.8 1.9 -3.8 4.3 v2.8 l-1.4 2.2 a0.6 0.6 0 0 0 0.5 0.9 h9.4 a0.6 0.6 0 0 0 0.5 -0.9 l-1.4 -2.2 v-2.8 c0 -2.4 -1.4 -4.3 -3.8 -4.3 Z" fill="currentColor" />
    <path d="M6.6 13.2 a1.4 1.4 0 0 0 2.8 0 Z" fill="currentColor" />
  </>),
  // 铃铛（描边）
  "bell-outline": (<>
    <path d="M8 2.1 c-2.1 0 -3.3 1.7 -3.3 3.9 v2.9 l-1.3 2 a0.6 0.6 0 0 0 0.5 0.9 h8.2 a0.6 0.6 0 0 0 0.5 -0.9 l-1.3 -2 v-2.9 c0 -2.2 -1.2 -3.9 -3.3 -3.9 Z"
      fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" />
    <path d="M6.7 13.2 a1.3 1.3 0 0 0 2.6 0" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
  </>),
};

export interface SymbolicIconProps {
  name: SymbolicIconName;
  size?: number;
  className?: string;
  style?: CSSProperties;
}

export function SymbolicIcon({ name, size = 16, className, style }: SymbolicIconProps) {
  return (
    <svg
      className={`symbolic-icon${className ? ` ${className}` : ""}`}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      aria-hidden="true"
      style={style}
    >
      {ICONS[name] ?? ICONS["dialog-information"]}
    </svg>
  );
}
```

- [x] **Step 4: 运行确认通过**

Run: `npm run test:run -- src/components/symbolic.test.tsx`
Expected: PASS（4 个测试）

- [x] **Step 5: 检查点**

提示用户：Task 2 完成，建议提交。

---

### Task 3: serversStore error 结构化 + getServerErrorInfo

**Files:**
- Modify: `src/types/server.ts`（StructuredError 类型 + error 字段）
- Modify: `src/stores/serversStore.ts`（签名 + 辅助函数）
- Test: `src/stores/serversStore.test.ts`（新建）

- [x] **Step 1: 写失败测试**

```ts
// src/stores/serversStore.test.ts
import { describe, it, expect, beforeEach, vi } from "vitest";

// mock 存储层：persist 中间件在测试环境不可调用 Tauri
vi.mock("../utils/storage", () => ({
  serversStorage: { getItem: vi.fn(), setItem: vi.fn(), removeItem: vi.fn() },
}));

import { useServersStore, getServerErrorInfo } from "./serversStore";
import { AuthErrorCode } from "../types/errors";

const baseServer = {
  id: "s1", name: "prod-1", host: "1.1.1.1", port: 22,
  status: "error" as const,
};

describe("getServerErrorInfo", () => {
  beforeEach(() => {
    useServersStore.setState({
      servers: [
        { ...baseServer, error: { code: AuthErrorCode.ConnectTimeout } },
        { id: "s2", name: "prod-2", host: "2.2.2.2", port: 22, status: "connected" as const, error: undefined },
      ],
      activeServerId: null,
    });
  });

  it("结构化错误 → 查表 title/message/action", () => {
    const info = getServerErrorInfo(useServersStore.getState().servers[0]);
    expect(info.title).toBe("无法连接服务器");
    expect(info.message).toBe("连接超时，未能建立连接");
    expect(info.action).toBe("请检查网络后重试");
  });

  it("字符串错误（旧格式）→ Unknown 映射 + detail", () => {
    const info = getServerErrorInfo({ ...baseServer, error: "legacy text" });
    expect(info.title).toBe("连接失败");
    expect(info.detail).toBe("legacy text");
  });

  it("其他服务器已连接 → hint 对比提示", () => {
    const info = getServerErrorInfo(useServersStore.getState().servers[0]);
    expect(info.hint).toContain("1 台服务器");
    expect(info.hint).toContain("正常");
  });

  it("无其他服务器 → 无 hint", () => {
    useServersStore.setState({ servers: [useServersStore.getState().servers[0]] });
    expect(getServerErrorInfo(useServersStore.getState().servers[0]).hint).toBeUndefined();
  });

  it("无 error 字段 → Unknown 兜底", () => {
    const info = getServerErrorInfo({ ...baseServer, error: undefined });
    expect(info.title).toBe("连接失败");
  });
});
```

- [x] **Step 2: 运行确认失败**

Run: `npm run test:run -- src/stores/serversStore.test.ts`
Expected: FAIL（getServerErrorInfo 未导出）

- [x] **Step 3: 实现类型扩展**

`src/types/server.ts` 在 `AuthCredentials` 接口后新增：

```ts
/**
 * 结构化连接错误（客户端分类，code 为 AuthErrorCode 数值）
 * 旧格式为纯字符串，过渡期两种并存
 */
export interface StructuredError {
  /** 错误码（AuthErrorCode 数值，见 src/types/errors.ts） */
  code: number;
  /** 错误细节（服务器名等上下文） */
  detail?: string;
}
```

`ServerConfig.error` 字段类型改为：

```ts
  /** 错误信息（当status为error时；结构化对象或旧格式字符串） */
  error?: StructuredError | string;
```

- [x] **Step 4: 实现 store 变更**

`src/stores/serversStore.ts`：

1. import 区新增：

```ts
import { getErrorInfo, parseConnectError } from "../types/errors";
import type { StructuredError } from "../types/server";
```

（`ServerConfig` 已从 `../types/server` 导入，合并到该行即可）

2. `ServersActions.setServerStatus` 签名（第 26 行）改为：

```ts
  setServerStatus: (id: string, status: ServerConfig["status"], error?: StructuredError | string, rttMs?: number) => void;
```

3. 文件末尾（Utility Functions 区）新增：

```ts
/** getServerErrorInfo 的返回结构（供 Settings/应用错误态渲染） */
export interface ServerErrorDisplay {
  title: string;
  message: string;
  action: string;
  detail?: string;
  /** 其他服务器连接正常时的对比提示（多行） */
  hint?: string;
}

/**
 * 组装服务器的结构化错误展示信息。
 * - 结构化 error → 查 ERROR_MAP 映射
 * - 字符串 error（旧格式）→ Unknown 映射 + detail
 * - hint 为「其他 N 台服务器连接正常」对比提示（读取当前 store，零探测）
 */
export function getServerErrorInfo(server: ServerConfig): ServerErrorDisplay {
  const parsed = server.error
    ? parseConnectError(server.error)
    : { code: 999, detail: undefined };
  const info = getErrorInfo(parsed.code);
  const others = useServersStore
    .getState()
    .servers.filter((s) => s.id !== server.id && s.status === "connected").length;
  return {
    title: info.title,
    message: info.message,
    action: info.action,
    detail: parsed.detail,
    hint: others > 0
      ? `ℹ️ 其他 ${others} 台服务器连接正常，仅此台无法连接\n可能是本机与该服务器之间的网络问题，建议更换网络环境后重试`
      : undefined,
  };
}
```

- [x] **Step 5: 运行确认通过**

Run: `npm run test:run -- src/stores/serversStore.test.ts`
Expected: PASS（5 个测试）

Run: `npx tsc --noEmit`
Expected: 零错误（`setServerStatus` 的 error 参数放宽为联合类型，既有 string 调用点不报错）

- [x] **Step 6: 检查点**

提示用户：Task 3 完成，建议提交。

---

### Task 4: ServerManager 写入结构化 + Settings 错误行升级

**Files:**
- Modify: `src/context/ServerManager.tsx:341`（connectServer catch）
- Modify: `src/context/ServerManager.tsx:390`（attemptReconnect 最终失败）
- Modify: `src/apps/Settings.tsx:647-648`
- Modify: `src/apps/Settings.css`（.st-conn-error 块，约 830-842 行）

- [x] **Step 1: ServerManager 写入结构化错误**

`parseConnectError` 已在 ServerManager.tsx import（前次任务引入）。两处写入点改为：

connectServer catch（原 `setServerStatus(id, "error", display);`）：

```ts
      // store 存结构化错误（Settings/应用错误态查表渲染）；display 完整文本仅用于日志与通知
      setServerStatus(id, "error", parseConnectError(err));
```

attemptReconnect 最终失败（原 `setServerStatus(serverId, "error", display);`）：

```ts
        setServerStatus(serverId, "error", parseConnectError(err));
```

（两处 `display` 变量保留：log 与 pushNotification 仍使用）

- [x] **Step 2: Settings 错误行结构升级**

`src/apps/Settings.tsx` import 区新增（检查是否已有，避免重复）：

```ts
import { SymbolicIcon } from "../components/symbolic";
import { getServerErrorInfo } from "../stores/serversStore";
```

替换 647-648 行的 `{cardServer?.error && (...)}` 块：

```tsx
                {(() => {
                  if (!cardServer?.error) return null;
                  const ei = getServerErrorInfo(cardServer);
                  return (
                    <div className="st-conn-error">
                      <SymbolicIcon name="dialog-error" size={14} className="st-conn-error-icon" />
                      <div className="st-conn-error-body">
                        <div className="st-conn-error-title">{ei.title}</div>
                        <div className="st-conn-error-message">
                          {ei.message}{ei.detail ? `（${ei.detail}）` : ""}
                        </div>
                        {ei.hint && <div className="st-conn-error-hint">{ei.hint}</div>}
                      </div>
                    </div>
                  );
                })()}
```

- [x] **Step 3: Settings.css 错误样式重构**

替换 `.st-conn-error` 整块为：

```css
.st-conn-error {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  font-size: var(--font-small);
  color: var(--quirel-danger-fg);
  background: var(--quirel-danger-bg);
  border: 1px solid var(--quirel-danger-border);
  padding: 8px 12px;
  border-radius: var(--radius-xs);
  margin-top: 8px;
}

.st-conn-error-icon {
  flex: none;
  margin-top: 1px;
  color: var(--quirel-danger-fg);
}

.st-conn-error-body {
  min-width: 0;
}

.st-conn-error-title {
  font-weight: 600;
}

.st-conn-error-message {
  color: var(--quirel-text-secondary);
  white-space: pre-line;
}

.st-conn-error-hint {
  color: var(--quirel-text-secondary);
  opacity: 0.85;
  white-space: pre-line;
  margin-top: 2px;
}
```

- [x] **Step 4: 验证**

Run: `npx tsc --noEmit`
Expected: 零错误

Run: `npm run test:run -- src/stores/serversStore.test.ts src/types/errors.test.ts`
Expected: PASS

- [x] **Step 5: 检查点**

提示用户：Task 4 完成，建议提交。

---

### Task 5: ConnectionState 组件（错误态 + 重连态）

**Files:**
- Create: `src/components/ConnectionState.tsx`
- Create: `src/components/ConnectionState.css`
- Test: `src/components/ConnectionState.test.tsx`

- [x] **Step 1: 写失败测试**

```tsx
// src/components/ConnectionState.test.tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ConnectionErrorState, ReconnectingState } from "./ConnectionState";

describe("ConnectionErrorState", () => {
  it("渲染标题/消息/建议/对比提示/重试按钮", () => {
    render(
      <ConnectionErrorState
        title="无法连接服务器"
        message="连接超时，未能建立连接"
        action="请检查网络后重试"
        hint="ℹ️ 其他 2 台服务器连接正常，仅此台无法连接"
        onRetry={() => {}}
      />
    );
    expect(screen.getByText("无法连接服务器")).toBeInTheDocument();
    expect(screen.getByText("连接超时，未能建立连接")).toBeInTheDocument();
    expect(screen.getByText("请检查网络后重试")).toBeInTheDocument();
    expect(screen.getByText("ℹ️ 其他 2 台服务器连接正常，仅此台无法连接")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重试" })).toBeInTheDocument();
  });

  it("无 hint 不渲染；无 onOpenSettings 不渲染次按钮", () => {
    render(<ConnectionErrorState title="t" message="m" onRetry={() => {}} />);
    expect(screen.queryByText(/台服务器/)).toBeNull();
    expect(screen.queryByRole("button", { name: "查看服务器" })).toBeNull();
  });

  it("重试按钮触发 onRetry", () => {
    const onRetry = vi.fn();
    render(<ConnectionErrorState title="t" message="m" onRetry={onRetry} />);
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("查看服务器按钮触发 onOpenSettings", () => {
    const onOpenSettings = vi.fn();
    render(
      <ConnectionErrorState title="t" message="m" onRetry={() => {}} onOpenSettings={onOpenSettings} />
    );
    fireEvent.click(screen.getByRole("button", { name: "查看服务器" }));
    expect(onOpenSettings).toHaveBeenCalledOnce();
  });
});

describe("ReconnectingState", () => {
  it("渲染重连文案与服务器名", () => {
    render(<ReconnectingState serverName="prod-1" />);
    expect(screen.getByText(/正在重新连接到 prod-1/)).toBeInTheDocument();
  });

  it("无服务器名时渲染通用文案", () => {
    render(<ReconnectingState />);
    expect(screen.getByText("正在重新连接…")).toBeInTheDocument();
  });
});
```

- [x] **Step 2: 运行确认失败**

Run: `npm run test:run -- src/components/ConnectionState.test.tsx`
Expected: FAIL（找不到模块）

- [x] **Step 3: 实现 ConnectionState.tsx**

```tsx
// src/components/ConnectionState.tsx
// 连接状态覆盖层（全应用统一）：
// - ConnectionErrorState：status=error 时的完整错误态（图标/标题/消息/建议/对比提示/重试）
// - ReconnectingState：status=reconnecting 时的轻量自动重连指示

import { SymbolicIcon } from "./symbolic";
import "./ConnectionState.css";

export interface ConnectionErrorStateProps {
  title: string;
  message: string;
  /** 行动建议（ERROR_MAP.action） */
  action?: string;
  /** 其他服务器连接正常时的对比提示（多行） */
  hint?: string;
  /** 错误细节 */
  detail?: string;
  onRetry: () => void;
  onOpenSettings?: () => void;
}

export function ConnectionErrorState({
  title, message, action, hint, detail, onRetry, onOpenSettings,
}: ConnectionErrorStateProps) {
  return (
    <div className="cs-error">
      <SymbolicIcon name="network-offline" size={48} className="cs-error-icon" />
      <div className="cs-error-title">{title}</div>
      <div className="cs-error-message">
        {message}{detail ? `（${detail}）` : ""}
      </div>
      {action && <div className="cs-error-action">{action}</div>}
      {hint && (
        <div className="cs-error-hint">
          <SymbolicIcon name="dialog-information" size={12} />
          <span>{hint}</span>
        </div>
      )}
      <div className="cs-error-buttons">
        <button className="cs-btn-primary" onClick={onRetry}>
          <SymbolicIcon name="view-refresh" size={14} />
          重试
        </button>
        {onOpenSettings && (
          <button className="cs-btn-ghost" onClick={onOpenSettings}>查看服务器</button>
        )}
      </div>
    </div>
  );
}

export interface ReconnectingStateProps {
  serverName?: string;
}

export function ReconnectingState({ serverName }: ReconnectingStateProps) {
  return (
    <div className="cs-reconnecting">
      <SymbolicIcon name="view-refresh" size={16} className="cs-reconnecting-icon" />
      <span>{serverName ? `正在重新连接到 ${serverName}…` : "正在重新连接…"}</span>
    </div>
  );
}
```

- [x] **Step 4: 实现 ConnectionState.css**

```css
/* src/components/ConnectionState.css */
/* 连接状态覆盖层（全应用统一） */

/* ── 错误态卡片 ─────────────────────────────────────── */
.cs-error {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  gap: 6px;
  max-width: 420px;
  padding: 24px 28px;
  background: var(--quirel-window-bg);
  border: 1px solid var(--quirel-border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-card);
  animation: cs-fade-in 180ms var(--ease-out);
}

.cs-error-icon {
  color: var(--quirel-danger-fg);
}

.cs-error-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--quirel-text-primary);
}

.cs-error-message {
  font-size: 14px;
  color: var(--quirel-text-secondary);
}

.cs-error-action {
  font-size: 12px;
  color: var(--quirel-text-secondary);
  background: var(--quirel-card-bg);
  border: 1px solid var(--quirel-border-color);
  border-radius: var(--radius-pill);
  padding: 2px 10px;
}

.cs-error-hint {
  display: flex;
  align-items: flex-start;
  gap: 4px;
  font-size: 12px;
  color: var(--quirel-text-secondary);
  white-space: pre-line;
  text-align: left;
}

.cs-error-buttons {
  display: flex;
  gap: 8px;
  margin-top: 10px;
}

.cs-btn-primary {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 36px;
  padding: 0 16px;
  background: var(--quirel-accent-bg);
  color: var(--quirel-accent-fg);
  border: none;
  border-radius: var(--radius-sm);
  font-size: 13px;
  cursor: pointer;
}

.cs-btn-primary:hover {
  background: var(--quirel-accent-hover);
}

.cs-btn-ghost {
  height: 36px;
  padding: 0 16px;
  background: transparent;
  color: var(--quirel-text-primary);
  border: 1px solid var(--quirel-border-color);
  border-radius: var(--radius-sm);
  font-size: 13px;
  cursor: pointer;
}

/* ── 轻量重连态 ─────────────────────────────────────── */
.cs-reconnecting {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 10px 18px;
  background: var(--quirel-window-bg);
  border: 1px solid var(--quirel-border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-card);
  font-size: 14px;
  color: var(--quirel-text-primary);
}

.cs-reconnecting-icon {
  color: var(--quirel-accent-bg);
}

/* ── 动效 ───────────────────────────────────────────── */
@media (prefers-reduced-motion: no-preference) {
  .cs-reconnecting-icon {
    animation: cs-rotate 1s linear infinite;
  }
}

@keyframes cs-rotate {
  to { transform: rotate(360deg); }
}

@keyframes cs-fade-in {
  from { opacity: 0; transform: translateY(8px); }
  to { opacity: 1; transform: translateY(0); }
}
```

- [x] **Step 5: 运行确认通过**

Run: `npm run test:run -- src/components/ConnectionState.test.tsx`
Expected: PASS（6 个测试）

- [x] **Step 6: 检查点**

提示用户：Task 5 完成，建议提交。

---

### Task 6: ArrivalToast 到达预览条

**Files:**
- Create: `src/shell/ArrivalToast.tsx`
- Create: `src/shell/ArrivalToast.css`
- Test: `src/shell/ArrivalToast.test.tsx`

- [x] **Step 1: 写失败测试**

```tsx
// src/shell/ArrivalToast.test.tsx
import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { ArrivalToast } from "./ArrivalToast";
import { useNotificationStore } from "../stores/notificationStore";

function push(title: string) {
  useNotificationStore.getState().pushNotification({
    title,
    body: "正文",
    urgency: "normal",
    source: "prod-1",
  });
}

describe("ArrivalToast", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useNotificationStore.setState({ notifications: [] });
  });
  afterEach(() => vi.useRealTimers());

  it("初始不显示", () => {
    const { container } = render(<ArrivalToast onOpen={() => {}} />);
    expect(container.querySelector(".at-toast")).toBeNull();
  });

  it("push 后显示最新通知（标题 + 来源）", () => {
    render(<ArrivalToast onOpen={() => {}} />);
    act(() => push("通知 A"));
    expect(screen.getByText("通知 A")).toBeInTheDocument();
    expect(screen.getByText("prod-1")).toBeInTheDocument();
  });

  it("3.5s 后进入退出动画，再 200ms 完全隐藏", () => {
    const { container } = render(<ArrivalToast onOpen={() => {}} />);
    act(() => push("通知 A"));
    act(() => vi.advanceTimersByTime(3500));
    expect(container.querySelector(".at-toast-leaving")).not.toBeNull();
    act(() => vi.advanceTimersByTime(200));
    expect(container.querySelector(".at-toast")).toBeNull();
  });

  it("hover 暂停自动隐藏", () => {
    const { container } = render(<ArrivalToast onOpen={() => {}} />);
    act(() => push("通知 A"));
    fireEvent.mouseEnter(container.querySelector(".at-toast")!);
    act(() => vi.advanceTimersByTime(6000));
    expect(container.querySelector(".at-toast")).not.toBeNull();
  });

  it("点击触发 onOpen", () => {
    const onOpen = vi.fn();
    render(<ArrivalToast onOpen={onOpen} />);
    act(() => push("通知 A"));
    fireEvent.click(screen.getByRole("status"));
    expect(onOpen).toHaveBeenCalledOnce();
  });

  it("连发替换内容并累计 +N", () => {
    render(<ArrivalToast onOpen={() => {}} />);
    act(() => push("通知 A"));
    act(() => push("通知 B"));
    expect(screen.getByText("通知 B")).toBeInTheDocument();
    expect(screen.queryByText("通知 A")).toBeNull();
    expect(screen.getByText("+1")).toBeInTheDocument();
  });
});
```

- [x] **Step 2: 运行确认失败**

Run: `npm run test:run -- src/shell/ArrivalToast.test.tsx`
Expected: FAIL（找不到模块）

- [x] **Step 3: 实现 ArrivalToast.tsx**

```tsx
// src/shell/ArrivalToast.tsx
// 新通知到达预览条：TopBar 下方居中悬浮，短暂展示最新一条通知
// 行为：显示 3.5s 自动收起；hover 暂停计时；点击打开通知中心；
//       连发替换内容并重置计时，期间错过的条目累计为「+N」

import { useEffect, useRef, useState, useCallback } from "react";
import { useNotificationStore } from "../stores/notificationStore";
import type { AppNotification, NotificationUrgency } from "../stores/notificationStore";
import { SymbolicIcon } from "../components/symbolic";
import "./ArrivalToast.css";

const DISPLAY_MS = 3500;
const EXIT_MS = 200;

/** urgency → Symbolic 图标 */
function urgencyIcon(urgency: NotificationUrgency) {
  switch (urgency) {
    case "critical": return "dialog-error" as const;
    case "normal": return "dialog-warning" as const;
    case "low": return "dialog-information" as const;
  }
}

export function ArrivalToast({ onOpen }: { onOpen: () => void }) {
  const notifications = useNotificationStore((s) => s.notifications);
  const [current, setCurrent] = useState<AppNotification | null>(null);
  const [missed, setMissed] = useState(0);
  const [visible, setVisible] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const shownIdRef = useRef<string | null>(null);
  const hideTimerRef = useRef<number | null>(null);
  const leaveTimerRef = useRef<number | null>(null);

  const clearTimers = useCallback(() => {
    if (hideTimerRef.current !== null) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
    if (leaveTimerRef.current !== null) {
      window.clearTimeout(leaveTimerRef.current);
      leaveTimerRef.current = null;
    }
  }, []);

  /** 收起：进入 200ms 退出动画后完全隐藏 */
  const dismiss = useCallback(() => {
    clearTimers();
    setLeaving(true);
    leaveTimerRef.current = window.setTimeout(() => {
      setVisible(false);
      setLeaving(false);
      setMissed(0);
      setCurrent(null);
    }, EXIT_MS);
  }, [clearTimers]);

  const scheduleHide = useCallback(() => {
    if (hideTimerRef.current !== null) window.clearTimeout(hideTimerRef.current);
    hideTimerRef.current = window.setTimeout(dismiss, DISPLAY_MS);
  }, [dismiss]);

  // 监听新通知（store 头部插入最新）
  useEffect(() => {
    if (notifications.length === 0) return;
    const latest = notifications[0];
    if (shownIdRef.current === latest.id) return;
    if (visible) setMissed((m) => m + 1); // 展示期间连发 → 累计
    shownIdRef.current = latest.id;
    setCurrent(latest);
    setLeaving(false);
    setVisible(true);
    scheduleHide();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [notifications]);

  // 卸载清理
  useEffect(() => clearTimers, [clearTimers]);

  if (!visible || !current) return null;

  return (
    <div
      className={`at-toast${leaving ? " at-toast-leaving" : ""}`}
      role="status"
      onClick={() => {
        dismiss();
        onOpen();
      }}
      onMouseEnter={() => {
        // hover 暂停自动隐藏计时
        if (hideTimerRef.current !== null) {
          window.clearTimeout(hideTimerRef.current);
          hideTimerRef.current = null;
        }
      }}
      onMouseLeave={() => {
        if (!leaving) scheduleHide();
      }}
    >
      <SymbolicIcon
        name={urgencyIcon(current.urgency)}
        size={16}
        className={`at-sev at-sev-${current.urgency}`}
      />
      <span className="at-title">{current.title}</span>
      {current.source && <span className="at-source">{current.source}</span>}
      {missed > 0 && <span className="at-missed">+{missed}</span>}
      <button
        className="at-close"
        aria-label="关闭预览"
        onClick={(e) => {
          e.stopPropagation();
          dismiss();
        }}
      >
        <SymbolicIcon name="window-close" size={12} />
      </button>
    </div>
  );
}
```

- [x] **Step 4: 实现 ArrivalToast.css**

```css
/* src/shell/ArrivalToast.css */
/* 到达预览条：TopBar 下方居中悬浮，全局弹窗层级 */

.at-toast {
  position: fixed;
  top: calc(var(--topbar-height) + 12px);
  left: 50%;
  transform: translateX(-50%);
  z-index: var(--z-notification);
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 280px;
  max-width: min(480px, 90%);
  height: 44px;
  padding: 0 12px;
  background: var(--quirel-window-bg);
  border: 1px solid var(--quirel-border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-popup);
  cursor: pointer;
  animation: at-slide-in 200ms var(--ease-out);
}

.at-toast-leaving {
  animation: at-slide-out 200ms var(--ease-out) forwards;
}

/* 严重度图标着色（urgency → 语义令牌） */
.at-sev { flex: none; }
.at-sev-critical { color: var(--quirel-danger-fg); }
.at-sev-normal { color: var(--quirel-warning-fg); }
.at-sev-low { color: var(--quirel-success-fg); }

.at-title {
  flex: 1;
  min-width: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--quirel-text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.at-source {
  font-size: 12px;
  color: var(--quirel-text-secondary);
  background: var(--quirel-card-bg);
  border: 1px solid var(--quirel-border-color);
  border-radius: var(--radius-pill);
  padding: 1px 8px;
  flex: none;
}

.at-missed {
  font-size: 12px;
  font-weight: 600;
  color: var(--quirel-accent-fg);
  background: var(--quirel-accent-bg);
  border-radius: var(--radius-pill);
  padding: 1px 8px;
  flex: none;
}

/* 关闭按钮仅 hover 显示 */
.at-close {
  display: none;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  background: transparent;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--quirel-text-secondary);
  cursor: pointer;
  flex: none;
}

.at-toast:hover .at-close {
  display: inline-flex;
}

.at-close:hover {
  background: var(--quirel-card-hover);
  color: var(--quirel-text-primary);
}

/* 动效（reduced-motion 降级为直接显隐） */
@keyframes at-slide-in {
  from { opacity: 0; transform: translate(-50%, -8px); }
  to { opacity: 1; transform: translate(-50%, 0); }
}

@keyframes at-slide-out {
  from { opacity: 1; transform: translate(-50%, 0); }
  to { opacity: 0; transform: translate(-50%, -8px); }
}

@media (prefers-reduced-motion: reduce) {
  .at-toast,
  .at-toast-leaving {
    animation: none;
  }
}
```

- [x] **Step 5: 运行确认通过**

Run: `npm run test:run -- src/shell/ArrivalToast.test.tsx`
Expected: PASS（6 个测试）

- [x] **Step 6: 检查点**

提示用户：Task 6 完成，建议提交。

---

### Task 7: NotificationCenter 呈现层重写

**Files:**
- Modify: `src/shell/NotificationCenter.tsx`（整文件替换）
- Modify: `src/shell/NotificationCenter.css`（令牌化 + 新增块）

- [x] **Step 1: 重写 NotificationCenter.tsx**

```tsx
// src/shell/NotificationCenter.tsx
// 通知中心：右侧滑出面板（呈现层，Adwaita Symbolic 风格）
// 数据源 notificationStore；交互（过滤/已读/清除）与数据层不变

import { useState, useEffect } from "react";
import "./NotificationCenter.css";
import { useNotificationStore } from "../stores/notificationStore";
import type { NotificationUrgency } from "../stores/notificationStore";
import { SymbolicIcon } from "../components/symbolic";

/* ── Utility Functions ───────────────────────────────── */

function formatTime(timestamp: number): string {
  const now = Date.now();
  const diff = now - timestamp;

  if (diff < 1000 * 60) return "刚刚";
  if (diff < 1000 * 60 * 60) return `${Math.floor(diff / 60000)} 分钟前`;
  if (diff < 1000 * 60 * 60 * 24) return `${Math.floor(diff / 3600000)} 小时前`;
  return `${Math.floor(diff / 86400000)} 天前`;
}

/** urgency → Symbolic 图标名 */
function getUrgencyIcon(urgency: NotificationUrgency) {
  switch (urgency) {
    case "critical": return "dialog-error" as const;
    case "normal": return "dialog-warning" as const;
    case "low": return "dialog-information" as const;
  }
}

/** urgency → 语义类名（左边框 + 图标着色由 CSS 令牌控制） */
function getUrgencyClass(urgency: NotificationUrgency): string {
  return `nc-notif-urgency-${urgency}`;
}

/* ── Main Component ─────────────────────────────────── */

interface NotificationCenterProps {
  isOpen: boolean;
  onClose: () => void;
}

export function NotificationCenter({ isOpen, onClose }: NotificationCenterProps) {
  // 真实数据源：zustand 内存 store（连接失败/断连/恢复通知）
  const { notifications, markAsRead, dismiss, clearAll, markAllRead } = useNotificationStore();
  const [filter, setFilter] = useState<NotificationUrgency | "all">("all");

  const unreadCount = notifications.filter((n) => !n.read).length;
  const criticalCount = notifications.filter((n) => n.urgency === "critical" && !n.read).length;

  const filteredNotifications = notifications.filter((n) => {
    if (filter === "all") return true;
    return n.urgency === filter;
  }).sort((a, b) => b.timestamp - a.timestamp);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isOpen) {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  return (
    <div className="nc-overlay" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="nc-panel">
        {/* Header */}
        <div className="nc-header">
          <div className="nc-header-title">
            <span className="nc-title">通知</span>
            {unreadCount > 0 && (
              <span className="nc-badge">{unreadCount}</span>
            )}
          </div>
          <div className="nc-header-actions">
            <button className="nc-action-btn" onClick={markAllRead} title="全部标记已读">
              <SymbolicIcon name="emblem-ok" size={14} />
              全部已读
            </button>
            <button className="nc-action-btn nc-action-danger" onClick={clearAll} title="清除全部">
              <SymbolicIcon name="user-trash" size={14} />
              清除
            </button>
            <button className="nc-close-btn" onClick={onClose} aria-label="关闭通知中心">
              <SymbolicIcon name="window-close" size={14} />
            </button>
          </div>
        </div>

        {/* Filters（文本 + 语义色点） */}
        <div className="nc-filters">
          <button
            className={`nc-filter-btn ${filter === "all" ? "active" : ""}`}
            onClick={() => setFilter("all")}
          >
            全部 ({notifications.length})
          </button>
          <button
            className={`nc-filter-btn ${filter === "critical" ? "active" : ""}`}
            onClick={() => setFilter("critical")}
          >
            <span className="nc-dot nc-dot-critical" />
            紧急 ({notifications.filter((n) => n.urgency === "critical").length})
          </button>
          <button
            className={`nc-filter-btn ${filter === "normal" ? "active" : ""}`}
            onClick={() => setFilter("normal")}
          >
            <span className="nc-dot nc-dot-normal" />
            普通 ({notifications.filter((n) => n.urgency === "normal").length})
          </button>
          <button
            className={`nc-filter-btn ${filter === "low" ? "active" : ""}`}
            onClick={() => setFilter("low")}
          >
            <span className="nc-dot nc-dot-low" />
            低 ({notifications.filter((n) => n.urgency === "low").length})
          </button>
        </div>

        {/* Notification List */}
        <div className="nc-list">
          {filteredNotifications.length === 0 ? (
            <div className="nc-empty">
              <SymbolicIcon name="mailbox" size={32} className="nc-empty-icon" />
              <div className="nc-empty-text">没有通知</div>
            </div>
          ) : (
            filteredNotifications.map((notif) => (
              <div
                key={notif.id}
                className={`nc-notif ${getUrgencyClass(notif.urgency)} ${!notif.read ? "nc-unread" : ""}`}
                onClick={() => markAsRead(notif.id)}
              >
                <SymbolicIcon
                  name={getUrgencyIcon(notif.urgency)}
                  size={16}
                  className="nc-notif-sev-icon"
                />
                <div className="nc-notif-content">
                  <div className="nc-notif-header">
                    <span className="nc-notif-title">{notif.title}</span>
                    <span className="nc-notif-source">{notif.source}</span>
                  </div>
                  <div className="nc-notif-body">{notif.body}</div>
                  {notif.action && <div className="nc-notif-action">{notif.action}</div>}
                  <div className="nc-notif-time">{formatTime(notif.timestamp)}</div>
                </div>
                <button
                  className="nc-notif-dismiss"
                  onClick={(e) => {
                    e.stopPropagation();
                    dismiss(notif.id);
                  }}
                  aria-label="删除通知"
                  title="删除"
                >
                  <SymbolicIcon name="window-close" size={12} />
                </button>
              </div>
            ))
          )}
        </div>

        {/* Footer */}
        <div className="nc-footer">
          <span className="nc-footer-text">
            {criticalCount > 0 && (
              <span className="nc-footer-warning">⚠️ {criticalCount} 个紧急通知未处理</span>
            )}
            {criticalCount === 0 && unreadCount > 0 && (
              <span>{unreadCount} 个未读通知</span>
            )}
            {criticalCount === 0 && unreadCount === 0 && (
              <span>所有通知已处理</span>
            )}
          </span>
        </div>
      </div>
    </div>
  );
}

/* ── Notification Badge Component ────────────────────── */

interface NotificationBadgeProps {
  count: number;
  criticalCount: number;
}

export function NotificationBadge({ count, criticalCount }: NotificationBadgeProps) {
  if (count === 0) return null;

  return (
    <span className={`nc-topbar-badge ${criticalCount > 0 ? "nc-badge-critical" : ""}`}>
      {count > 9 ? "9+" : count}
    </span>
  );
}
```

- [x] **Step 2: NotificationCenter.css 令牌化改造**

先在文件中 `grep -n "#[0-9a-fA-F]\{3,8\}\|rgba(" src/shell/NotificationCenter.css` 找出全部硬编码颜色，按映射表替换（出现几处替换几处）：

| 旧值 | 新值 |
|---|---|
| `#e01b24` / `#f66151` / `#ff7b63`（红色系） | `var(--quirel-danger-fg)` |
| `rgba(224, 27, 36, …)` / `rgba(246, 97, 81, …)`（红色系背景） | `var(--quirel-danger-bg)` |
| `#f5c211` / `#e5a50a` / `#f8ad34`（黄色系 fg） | `var(--quirel-warning-fg)` |
| 黄色系半透明背景 | `var(--quirel-warning-bg)` |
| `#2ec27e` / `#33d17a` / `#26a269`（绿色系 fg） | `var(--quirel-success-fg)` |
| 绿色系半透明背景 | `var(--quirel-success-bg)` |
| `#99c1f1` / `#62a0ea`（蓝色提示文字） | `var(--quirel-text-secondary)` |
| `#fff` / `#ffffff`（徽标文字） | `var(--quirel-accent-fg)`（若为徽标背景文字）或 `var(--quirel-window-bg)`（按语境） |

删除 `[data-theme="dark"] .nc-unread` 等暗色补丁块（令牌化后不再需要，半透明令牌双主题通用；若删除后布局异常则保留结构仅改色值）。

- [x] **Step 3: NotificationCenter.css 新增块**

文件末尾追加：

```css
/* ── Symbolic 图标（条目/空态）───────────────────────── */
.nc-notif-sev-icon {
  flex: none;
  margin-top: 2px;
}

.nc-notif-urgency-critical .nc-notif-sev-icon { color: var(--quirel-danger-fg); }
.nc-notif-urgency-normal .nc-notif-sev-icon { color: var(--quirel-warning-fg); }
.nc-notif-urgency-low .nc-notif-sev-icon { color: var(--quirel-success-fg); }

/* 严重度左边框（未读时着色，已读中性） */
.nc-notif {
  position: relative;
  border-left: 3px solid transparent;
}

.nc-unread.nc-notif-urgency-critical { border-left-color: var(--quirel-danger-fg); }
.nc-unread.nc-notif-urgency-normal { border-left-color: var(--quirel-warning-fg); }
.nc-unread.nc-notif-urgency-low { border-left-color: var(--quirel-success-fg); }

/* 删除按钮 hover 显示（降低视觉噪音） */
.nc-notif-dismiss {
  opacity: 0;
  transition: opacity var(--duration-fast) var(--ease-out);
}

.nc-notif:hover .nc-notif-dismiss,
.nc-notif-dismiss:focus-visible {
  opacity: 1;
}

/* 筛选行语义色点 */
.nc-dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  margin-right: 4px;
  vertical-align: middle;
}

.nc-dot-critical { background: var(--quirel-danger-fg); }
.nc-dot-normal { background: var(--quirel-warning-fg); }
.nc-dot-low { background: var(--quirel-success-fg); }

/* 空态图标 */
.nc-empty-icon {
  color: var(--quirel-text-disabled);
}

/* 头部操作按钮图标 */
.nc-action-btn .symbolic-icon {
  vertical-align: -2px;
  margin-right: 2px;
  color: var(--quirel-text-secondary);
}

.nc-action-danger .symbolic-icon {
  color: var(--quirel-danger-fg);
}

.nc-close-btn .symbolic-icon {
  color: var(--quirel-text-secondary);
}
```

注意：`.nc-notif` 原有样式若已含 `position`/`border`，合并而非重复声明；`.nc-notif-dismiss` 原有 `display` 保留，仅追加 opacity 过渡。

- [x] **Step 4: 验证**

Run: `npx tsc --noEmit`
Expected: 零错误

Run: `npm run test:run -- src/shell/ArrivalToast.test.tsx`
Expected: PASS（回归）

- [x] **Step 5: 检查点**

提示用户：Task 7 完成，建议提交。

---

### Task 8: useOpenApp hook + Desktop 挂载 + TopBar 铃铛

**Files:**
- Create: `src/window-system/hooks/useOpenApp.ts`
- Modify: `src/shell/Desktop.tsx`（import + render）
- Modify: `src/shell/TopBar/TopBar.tsx:85-91`

- [x] **Step 1: 实现 useOpenApp hook**

```ts
// src/window-system/hooks/useOpenApp.ts
// "聚焦或创建"应用打开逻辑（提取自 Desktop createApp，供应用内按钮复用）
// 行为：已打开→恢复/聚焦；未打开→创建新窗口

import { useCallback } from "react";
import { useWindowManager } from "../WindowManagerContext";

export function useOpenApp() {
  const { manager } = useWindowManager();

  return useCallback(
    async (appId: string) => {
      const existing = manager.getByAppId(appId);
      if (existing.length > 0) {
        if (existing[0].minimized) {
          manager.restore(existing[0].id);
        }
        manager.focus(existing[0].id);
        return;
      }
      await manager.create(appId);
    },
    [manager]
  );
}
```

（`IWindowManager.create(appId, options?)`、`focus`、`restore`、`getByAppId` 签名见 `src/window-system/types.ts:110-120`）

- [x] **Step 2: Desktop 挂载 ArrivalToast**

`src/shell/Desktop.tsx` import 区新增：

```ts
import { ArrivalToast } from "./ArrivalToast";
```

在 `<NotificationCenter ... />`（约 385 行）后追加：

```tsx
      {/* 新通知到达预览条（TopBar 下方居中，点击打开通知中心） */}
      <ArrivalToast onOpen={() => setNotificationOpen(true)} />
```

- [x] **Step 3: TopBar 铃铛替换**

`src/shell/TopBar/TopBar.tsx` import 区新增：

```ts
import { SymbolicIcon } from "../../components/symbolic";
```

替换 85-91 行的 `{/* Notification */}` 块：

```tsx
      {/* Notification */}
      <button className={styles.notificationBtn} onClick={onNotificationClick} aria-label="通知">
        <SymbolicIcon name={unreadNotifications > 0 ? "bell" : "bell-outline"} size={14} />
        <NotificationBadge
          count={unreadNotifications}
          criticalCount={criticalNotifications}
        />
      </button>
```

并在 `src/shell/TopBar/TopBar.module.css` 的 `.notificationBtn` 块追加（若已有 font-size emoji 调整可保留）：

```css
.notificationBtn svg {
  display: block;
}
```

- [x] **Step 4: 验证**

Run: `npx tsc --noEmit`
Expected: 零错误

Run: `npm run build`
Expected: 构建成功

- [x] **Step 5: 检查点**

提示用户：Task 8 完成，建议提交。

---

### Task 9: 应用接入（SystemMonitor / FileManager / BrowserApp）

**Files:**
- Modify: `src/apps/SystemMonitor.tsx`（import 区 + 离线分支）
- Modify: `src/apps/SystemMonitor.css`（offline-badge 图标）
- Modify: `src/apps/FileManager.tsx`（import 区 + isOffline 分支 + emoji 圆点）
- Modify: `src/apps/FileManager.css`（osb-dot）
- Modify: `src/apps/BrowserApp.tsx`（import 区 + 未连接分支）

**统一模式**（三应用相同）：断连后 `activeServerId` 已清空，从 `servers` 中按状态找"故障服务器"：

```tsx
// 断连故障服务器（activeServerId 已清空，按状态识别；reconnecting 优先于 error）
const troubledServer =
  servers.find((s) => s.status === "reconnecting") ||
  servers.find((s) => s.status === "error") ||
  null;
```

已知小限制：多台服务器同时 error 时取数组第一台（重试按钮按台可用，可接受）。

- [x] **Step 1: SystemMonitor 接入**

`src/apps/SystemMonitor.tsx` import 区新增：

```ts
import { SymbolicIcon } from "../components/symbolic";
import { ConnectionErrorState, ReconnectingState } from "../components/ConnectionState";
import { getServerErrorInfo } from "../stores/serversStore";
import { useOpenApp } from "../window-system/hooks/useOpenApp";
```

第 141 行解构扩展（原 `const { activeServerId, activeServer } = useServerManager();`）：

```tsx
  const { activeServerId, activeServer, servers, connectServer } = useServerManager();
  const openApp = useOpenApp();

  // 断连故障服务器（activeServerId 已清空，按状态识别；reconnecting 优先于 error）
  const troubledServer =
    servers.find((s) => s.status === "reconnecting") ||
    servers.find((s) => s.status === "error") ||
    null;
```

替换离线分支（原 305-313 行 `showOffline ?` 块）：

```tsx
        {showOffline ? (
          troubledServer ? (
            /* 连接中断：自动重连轻量态 / 最终失败错误态 */
            <div className="sm-offline-state">
              <MonitorSkeleton />
              <div className="sm-offline-overlay">
                {troubledServer.status === "reconnecting" ? (
                  <ReconnectingState serverName={troubledServer.name || troubledServer.host} />
                ) : (
                  <ConnectionErrorState
                    {...getServerErrorInfo(troubledServer)}
                    onRetry={() => connectServer(troubledServer.id)}
                    onOpenSettings={() => openApp("settings")}
                  />
                )}
              </div>
            </div>
          ) : (
          /* 层 3: 离线占位符（从未连接/用户主动断开） */
          <div className="sm-offline-state">
            <MonitorSkeleton />
            <div className="sm-offline-overlay">
              <div className="offline-badge">
                <SymbolicIcon name="network-offline" size={16} className="offline-badge-icon" />
                未连接
              </div>
              <div className="offline-hint">连接到远程服务器以查看系统监控数据</div>
            </div>
          </div>
          )
        ) : showSkeleton ? (
```

`src/apps/SystemMonitor.css` 的 `.offline-badge` 块内追加图标样式（flex 布局）：

```css
.offline-badge {
  /* …既有样式保留… */
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.offline-badge-icon {
  color: var(--quirel-text-secondary);
}
```

（若 `.offline-badge` 既有样式与 flex 冲突，仅添加 `.offline-badge-icon` 着色，图标仍内联显示）

- [x] **Step 2: FileManager 接入**

`src/apps/FileManager.tsx` import 区新增：

```ts
import { SymbolicIcon } from "../components/symbolic";
import { ConnectionErrorState, ReconnectingState } from "../components/ConnectionState";
import { getServerErrorInfo } from "../stores/serversStore";
import { useOpenApp } from "../window-system/hooks/useOpenApp";
```

在 `isOffline` 定义（约 136 行）附近、组件渲染前新增：

```tsx
  const openApp = useOpenApp();

  // 断连故障服务器（activeServerId 已清空，按状态识别；reconnecting 优先于 error）
  const troubledServer =
    servers.find((s) => s.status === "reconnecting") ||
    servers.find((s) => s.status === "error") ||
    null;
```

（`servers`/`connectServer` 已在作用域内——离线服务器列表已使用）

替换离线分支开头（原 1708-1713 行 `isOffline ? (` 块）：

```tsx
        {isOffline ? (
          troubledServer ? (
            /* 连接中断：自动重连轻量态 / 最终失败错误态 */
            <div className="fm-offline">
              {troubledServer.status === "reconnecting" ? (
                <ReconnectingState serverName={troubledServer.name || troubledServer.host} />
              ) : (
                <ConnectionErrorState
                  {...getServerErrorInfo(troubledServer)}
                  onRetry={() => connectServer(troubledServer.id)}
                  onOpenSettings={() => openApp("settings")}
                />
              )}
            </div>
          ) : (
          /* ── 离线空状态 ─────────────────────────────── */
          <div className="fm-offline">
            <div className="offline-icon">
              <SymbolicIcon name="network-offline" size={32} />
            </div>
            <div className="offline-title">无远程连接</div>
            <div className="offline-desc">请先连接到远程服务器以浏览远程文件系统。</div>
```

（后续"可用服务器"列表保持原样，仅下一步替换 emoji）

替换服务器列表 emoji 圆点（原 1725-1729 行）：

```tsx
                      <span className={`osb-dot osb-dot-${server.status}`} />
```

`src/apps/FileManager.css` 在 `.osb-status` 样式附近新增：

```css
/* 服务器状态色点（替代 emoji，令牌化） */
.osb-dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex: none;
}

.osb-dot-connected { background: var(--quirel-success-color); }
.osb-dot-connecting { background: var(--quirel-warning-color); }
.osb-dot-reconnecting { background: #ff7800; }
.osb-dot-error { background: var(--quirel-error-color); }
.osb-dot-disconnected { background: #9a9996; }
```

- [x] **Step 3: BrowserApp 接入**

`src/apps/BrowserApp.tsx` import 区新增：

```ts
import { SymbolicIcon } from "../components/symbolic";
import { ConnectionErrorState, ReconnectingState } from "../components/ConnectionState";
import { getServerErrorInfo } from "../stores/serversStore";
import { useOpenApp } from "../window-system/hooks/useOpenApp";
```

第 38 行解构扩展（原 `const { activeServer } = useServerManager();`）：

```tsx
  const { activeServer, servers, connectServer } = useServerManager();
  const openApp = useOpenApp();
```

替换未连接分支（原 138-149 行）：

```tsx
  // ── 未连接：占位提示 ────────────────────────────────────
  if (!connected) {
    // 断连故障服务器（activeServerId 已清空，按状态识别；reconnecting 优先于 error）
    const troubledServer =
      servers.find((s) => s.status === "reconnecting") ||
      servers.find((s) => s.status === "error") ||
      null;

    if (troubledServer) {
      return (
        <div className="ba">
          <div className="ba-empty">
            {troubledServer.status === "reconnecting" ? (
              <ReconnectingState serverName={troubledServer.name || troubledServer.host} />
            ) : (
              <ConnectionErrorState
                {...getServerErrorInfo(troubledServer)}
                onRetry={() => connectServer(troubledServer.id)}
                onOpenSettings={() => openApp("settings")}
              />
            )}
          </div>
        </div>
      );
    }

    return (
      <div className="ba">
        <div className="ba-empty">
          <div className="ba-empty-icon">
            <SymbolicIcon name="network-offline" size={32} />
          </div>
          <div className="ba-empty-title">未连接服务器</div>
          <div className="ba-empty-desc">连接服务器后，可通过服务器网络浏览网页</div>
        </div>
      </div>
    );
  }
```

- [x] **Step 4: 验证**

Run: `npx tsc --noEmit`
Expected: 零错误

Run: `npm run build`
Expected: 构建成功

- [x] **Step 5: 检查点**

提示用户：Task 9 完成，建议提交。

---

### Task 10: 全量验证

**Files:** 无新改动（仅验证）

- [x] **Step 1: 类型检查**

Run: `npx tsc --noEmit`
Expected: 零错误

- [x] **Step 2: 全量前端测试**

Run: `npm run test:run`
Expected: 既有 136 个中 130 过（6 个历史遗留失败：Terminal.test.tsx ×4、WindowManagerContext.test.tsx ×2）+ **本计划新增测试全过**（symbolic 4 + serversStore 5 + ConnectionState 6 + ArrivalToast 6 = 21 个新测试）。任何非历史遗留的新失败都必须修复。

- [x] **Step 3: 构建**

Run: `npm run build`
Expected: 成功

- [x] **Step 4: 手动冒烟清单（提示用户执行）**

1. 错误密码连接 → 通知中心"用户名或密码错误"卡片（danger 边框+图标）+ 到达预览条弹出 3.5s
2. 断网 → SystemMonitor 显示"正在重新连接…"轻量态 → 3 次失败后切换完整错误态（含重试/查看服务器按钮）
3. 重连成功 → 通知"连接已恢复"（low/绿色图标）
4. 多服务器场景（1 台连接 + 1 台失败）→ 错误卡与 Settings 显示"其他 N 台服务器连接正常"对比提示
5. TopBar 铃铛随未读数切换实底/描边；通知中心筛选/已读/清除/空态（mailbox 图标）
6. 主题切换（paper/neutral/dark）→ 通知与错误态颜色跟随令牌

- [x] **Step 5: 完成提示**

提示用户：全部任务完成，建议整体 review 后提交；服务器 Agent 无需更新（本次纯客户端）。

---

## Self-Review 记录

- **规格覆盖**：§3 令牌→Task 1；§3.1 图标→Task 2；§4 面板→Task 7；§5 预览条→Task 6+8；§6.1/6.2 组件→Task 5；§6.3 数据流→Task 3+4；§6.4 接入→Task 9；§7 TopBar→Task 8；§9 验证→各 Task+Task 10。无缺口。
- **占位符**：无 TBD/TODO；CSS 颜色映射表为穷举式指令（grep+替换），非占位。
- **类型一致性**：`StructuredError`（Task 3）↔ `setServerStatus` 签名（Task 3）↔ ServerManager 写入（Task 4）↔ `getServerErrorInfo` 返回 `ServerErrorDisplay`（Task 3）↔ `ConnectionErrorStateProps` 字段（Task 5，title/message/action/hint/detail 逐一对应）↔ `{...getServerErrorInfo(...)}` 展开（Task 9）一致。`SymbolicIconName` 各处引用的字面量（Task 2 定义）均存在。
- **已核实的 API 事实**：`IWindowManager.create(appId, options?)`/`focus`/`restore`/`getByAppId`（types.ts:110-120）；`ServerManager` 上下文暴露 `servers`/`activeServer`/`connectServer`（ServerManager.tsx:40-49）；vitest jsdom 环境（vitest.config.ts）。
