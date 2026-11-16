# 通知功能优化设计（2026-11-16）

- 状态：已确认（用户拍板：方案 A 聚合降噪通过；偏好设置不做；中心组织与可靠性通过；
  追加「清空按钮跟随当前筛选视图」语义）
- 分支：`feature/notifications`（从 dev 4565cf5 拉出）
- 涉及文件：`notificationStore.ts`、`NotificationCenter.tsx`、`ArrivalToast.tsx`、
  `notify.ts`、各调用点、i18n 词典；不涉及 Rust/协议/持久化版本升级

## 1. 目标与范围

- 重复通知合并降噪：断连重试、批量传输失败等同源通知不再刷屏。
- 通知中心更好组织：未读过滤、搜索、聚合徽标、清空二次确认。
- 清空语义跟随当前筛选视图：选中某筛选时只清该视图内通知，全选时才清全部。
- 明确不做：通知偏好设置（toast 策略/时长/类别静音/免打扰，用户决定砍掉）、
  系统级 OS 通知开关、稍后处理/归档、多选批量操作。

## 2. 数据模型与聚合

`AppNotification` 新增可选字段（磁盘 `notifications.bin` 版本保持 1，migrate 不变）：

- `kind`: `"connection" | "transfer" | "file" | "archive" | "system" | "other"`；
  旧记录缺省 `other`。
- `count`: 聚合条数，缺省 1。

聚合键不落盘，在 `pushNotification` 内计算：`serverId + kind + source + title`。

聚合规则（提取为导出纯函数 `mergeIncoming(existing, incoming)`，便于单测）：

- 命中条件（全部满足）：同 serverId（均 null 视为系统通知同类）∧ 同 kind ∧
  同 source ∧ 同 title ∧ 目标未读（`read === false`）∧
  `incoming.timestamp - existing.timestamp < MERGE_WINDOW_MS`（60 秒）。
- 命中行为：保留原 id；`count++`；`timestamp`、`body` 刷新为最新；条目保持头部
  置顶（store 数组天然头部插入）；`details` 追加去重，累计上限 30 行；
  `live` 保持 true（预览条仍可弹）。
- 不聚合：目标已读、超 60s 窗口、existing 带 `onAction`（避免撤销动作被覆盖）、
  kind/source/title 任一不同、跨服务器（serverId 不同，含 null 与非 null 混合）。

容量交互：合并不新增条目，NOTIFICATION_CAP=100 压力下降；已读目标超窗后自然
新建条目（预期行为，测试覆盖）。

## 3. 调用点 kind 标注

| 调用点 | kind |
|---|---|
| `ServerManager.tsx`（断连/重连/连接成功） | connection |
| `useFileTransfer.ts`、`FileManager.tsx`（移动/复制结果与失败） | file |
| `TaskCard.tsx`（传输任务通知） | transfer |
| `ArchiveViewer/notifications.ts` | archive |
| `credentials.ts`、`WallpaperContext.tsx` | system |

`notify.ts` 的 `pushServerNotification` 透传 `kind`（入参类型放开），直接调
`pushNotification` 的系统级调用点显式补 `kind: "system"`。

## 4. 通知中心组织（NotificationCenter.tsx）

- 筛选行：现有严重度 chips（全部/严重/普通/低/其他）不变；其后增加「未读」
  toggle 按钮（同 chip 样式，active 高亮）。
- 搜索框：筛选行下方单行输入；匹配 `title/body/originOf(...)` 子串（不区分
  大小写）；与严重度 chip、未读开关 AND 叠加。
- 聚合徽标：`count > 1` 时卡片时间行旁显示 `×N`。
- 过滤逻辑提取纯函数（severity + unread + query 叠加）并导出，供单测。
- 清空二次确认：复用 `components/Dialog/ConfirmDialog`（初始焦点在取消）；
  仅清空需要，单条删除不需要。
- 清空跟随筛选视图（用户追加要求）：
  - `clearAll` 的过滤 = `isNotificationVisible` ∧（严重度 chip 匹配，all 时
    不限）∧（未读开时仅 `!read`）；
  - 即选中「严重」chip 时只清严重通知，未读开时只清未读，叠加生效；
    filter=all 且未读关时才是清空当前服务器全部；
  - 确认弹窗文案动态说明范围与条数（i18n 插值）；
  - 「全部已读」按钮保持现状（始终作用于当前服务器可见集，不跟随筛选）。

## 5. 到达预览条（ArrivalToast.tsx）

- 合并通知 = 同 id 的后续更新：现有 effect 以 `notifications` 数组驱动，
  id 未变时 `shownIdRef` 拦截——需调整为「同 id 但 timestamp 变化」重置
  停留计时并显示 `×N`（读 `count`），不再当作新条目累计 missed。
- `toast: false`、`live !== true` 的拦截规则不变。

## 6. i18n

`notifications.*` 新增（zh-CN 与 en-US 同步）：
`unreadOnly`、`searchPlaceholder`、`clearConfirmTitle`、`clearConfirmMessage`
（含范围与 {{count}} 插值）、`mergedBadge`（如「×{{n}}」直接复用文本亦可）。

## 7. 测试计划

- `notificationStore.test.ts`：mergeIncoming 矩阵——窗口内合并、跨窗不合并、
  已读不合并、onAction 不合并、跨服务器不合并、details 去重与 30 行上限、
  count 累加、global（null serverId）合并。
- NotificationCenter 过滤纯函数：severity/unread/query 叠加、大小写不敏感、
  清空 filter 构造（chip × 未读组合）。
- ArrivalToast：同 id 合并更新重置计时与 ×N 显示（补测）。
- 门禁：`npx tsc --noEmit`、`npm run verify:fe`；完成声明前 `npm run verify:full`。

## 8. 风险与兼容

- 旧记录无 kind → other：与带 kind 的新通知聚合键不同，不会误合并。
- 搜索匹配 originOf 依赖 servers 列表，服务器已删除时回退 serverName/source
  （originOf 现有行为，不改动）。
