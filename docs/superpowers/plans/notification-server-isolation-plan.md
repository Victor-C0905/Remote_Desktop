# 通知按服务器隔离 — 重做计划

## Context（为什么重做）

上一版只做了「展示时按 `activeServerId` 过滤」，但没有解决两个根本问题：

1. **历史数据泄漏**：`notifications.bin` 里旧版本落盘的通知（尤其 FileManager / useFileTransfer 推送的文件操作通知）没有 `serverId`，`isNotificationVisible` 把它们当成系统通知，导致切到任何服务器都能看到 → 用户反馈的「跨服务器通知泄漏」。
2. **无法回看历史、无法辨认归属**：通知中心只有 urgency 筛选，没有服务器切换；条目来源显示「文件管理」而非服务器名。

目标形态（用户已确认）：
- 通知中心默认只看「当前服务器 + 系统/连接通知」；
- 顶部提供服务器切换器（全部 / 各服务器）回看历史；
- 条目显示服务器名；
- 连接类通知（连接失败/断开/重连）保持全局可见（用户此前已确认）。

## 一、数据模型与持久化迁移

文件：`src/stores/notificationStore.ts`、`src/utils/storage.ts`

在现有 `serverId?` / `global?` 基础上只加两个可选字段（不引入 scope 枚举，避免双真相）：

- `serverName?: string`：推送时快照服务器名，供服务器删除/改名后兜底显示。
- `legacy?: true`：仅迁移写入，标记「无法判断归属的旧记录」。

**persist 升版 + 迁移**（settingsStore L201-213 是既有先例）：

- `version: 1`
- `migrate`：对 `!serverId && !global` 的旧条目标 `legacy: true`，其余字段保持不变；兼容 `persisted` 为空，幂等。
- **同步修改** `EXPECTED_STORE_VERSIONS["notifications"] = 1`。不同步会导致 `evaluatePersistedValue` 把新文件判为「更高版本」并阻断整个 notifications.bin（高风险点）。
- 旧版系统通知（凭据/壁纸）也会被标 legacy，只在「全部」可见；可接受，避免误伤。

## 二、可见性选择器

文件：`src/stores/notificationStore.ts`

新增纯函数（便于单测）：

```ts
type NotificationView =
  | { kind: 'current'; activeServerId: string | null }
  | { kind: 'all' }
  | { kind: 'server'; serverId: string };
```

规则：

| 视图 | 命中条件 |
|---|---|
| current | `legacy` 一律排除；`global` 命中；无 `serverId`（系统通知）命中；`serverId === activeServerId` 命中 |
| server | `serverId === view.serverId`（含该服的连接通知；global 也命中） |
| all | 全部（legacy 也在内） |

- `isNotificationVisible(n, activeServerId)` 保留为 `current` 视图的包装，供 Desktop / ArrivalToast 继续使用，行为不变。
- **角标（Desktop→TopBar）与 ArrivalToast 固定用 current**，不受通知中心里切换的影响。
- `activeServerId` 为 null（断线/重连窗口）时，current 只剩 global + 系统通知，连接中断通知不会丢。

## 三、push 侧收口（避免再次漏标）

新增 `src/utils/notify.ts`：

```ts
pushServerNotification(serverId, payload)
```

- 从 `useServersStore.getState().servers` 查 `name || host`，写入 `serverId` + `serverName`；
- `serverId` 为空时按系统通知推送（不写 serverId）；
- `source` 仍由调用方给模块名。

改造范围（替换直接 `pushNotification`）：
- `src/apps/FileManager.tsx`（文件操作，约 15 处；`notConnected` 分支保持系统通知）
- `src/apps/useFileTransfer.ts`（3 处）
- `src/components/TransferStatusBar/TaskCard.tsx`（1 处，`task.session_id` 即服务器 ID）
- `src/apps/ArchiveViewer/notifications.ts`（已有 serverId，补 serverName）
- `src/context/ServerManager.tsx`（4 处连接通知，保留 `global: true`）
- 不改造：`WallpaperContext`、`credentials`（系统级，无服务器归属）

## 四、通知中心 UI

文件：`src/shell/NotificationCenter.tsx`、`NotificationCenter.css`、`src/components/ToolbarMenu/ToolbarMenu.tsx`

- **切换器位置**：标题行下方新增 scope bar，复用 `ToolbarMenu`（自带 portal、`data-browser-overlay`、外部点击关闭）。给 `ToolbarMenu` 增加可选 `label?: string`，触发按钮显示「范围：当前服务器（A）⌄」而不是只有图标；CSS 增加带 label 的样式。
- **选项**：`当前服务器（名称）`、`全部通知`、分隔线、各服务器（来自 `serversStore.servers` ∪ 通知中出现过的 serverId；已删除的显示 `serverName || id` +「已删除」）。每项带该视图未读数，`checked` 标记当前选中。
- **状态**：`view` 默认 `current`；`isOpen` 变为 true 时重置为 `current`（每次打开都回到当前服务器）。
- **计数/列表/空态**：全部基于 `matchesNotificationView(view)`；header 未读数与 urgency chips 计数同步为所选视图的统计。
- **已读**：打开面板时只标记 current 视图为已读（维持现有语义）；在切换器切到其它视图时**不自动已读**（便于查看历史未读）。
- **清除**：作用于所选视图；「全部」视图下清空所有记录（按钮文案保持「清除」，title 现有文案即可）。
- **条目来源 chip**：`servers 实时名 → serverName 快照 → source`；legacy 条目标一个「历史」小标签。设计文档本来就规定来源 chip 用 `server.name || server.host`。
- **i18n**（zh-CN / en-US 同步）：`notifications.scopeLabel`、`scopeCurrent`（可带 `{{name}}`）、`scopeAll`、`scopeDeleted`、`legacyChip`。

## 五、Desktop / ArrivalToast

文件：`src/shell/Desktop.tsx`、`src/shell/ArrivalToast.tsx`

- Desktop 角标计数继续用 `isNotificationVisible`（current），无需改逻辑。
- ArrivalToast 继续按 current 过滤；`legacy` 已在 current 中排除，预览条不会弹出旧记录。
- `NotificationCenter` 新增 `servers` 订阅仅用于切换器与名称显示。

## 六、测试与验证

单测（新增 `src/stores/notificationStore.test.ts`）：
- `matchesNotificationView`：current（global / 系统 / 本服 / 他服 / legacy / active=null）、server、all 六类用例。
- 迁移函数：旧无 serverId 记录 → legacy；有 serverId/global 的记录原样保留；空数据不报错。
- 现有 `ArrivalToast.test.tsx` 的推送无 serverId（系统通知），current 仍可见，预期不回归。

命令：
- `npx tsc --noEmit`
- `npx vitest run src/stores/notificationStore.test.ts src/shell/ArrivalToast.test.tsx`（本地已知 `@csstools/css-calc` ERR_REQUIRE_ESM，若仍失败则记录为环境问题，单测交 CI）
- 无 Rust 改动，不需要 cargo。

手工回归（`npm run tauri dev`）：
1. 用旧版 notifications.bin 启动：旧文件操作通知不出现在当前服务器/角标/预览条，只在「全部」可见且带「历史」。
2. 连接 A 制造文件通知 → 切到 B：A 的通知不出现；切换器选 A 能看到历史，条目显示 A 名称。
3. 断线/重连：连接通知始终可见；current 在 active 为空时只显示系统/连接通知。
4. 重启应用：迁移只执行一次，新通知带 serverId/serverName，legacy 标记保留。
5. 删除服务器：`purgeServer` 清理其通知；删除后切换器不再出现该服务器（或显示「已删除」直到清理完成）。
6. 服务器改名：条目 chip 跟随新名称（实时查询优先）。

## 不做什么

- 不改 Rust / 协议 / 传输逻辑。
- 不做「通知按服务器分组平铺」，不做未读状态跨视图自动同步。
- 不把连接可用性通知改成服务器隔离（用户已明确要求始终可见）。
