# 文件管理器：目录实时一致性（inotify 推送 + 文件级增量）

## Context

选定路线 C：agent 侧 inotify 监听当前浏览目录并主动推送变更，客户端增量更新列表。
关键设计决定：**推送的是文件级增量（upsert/remove/rename），不是“目录变了，你自己重读”**。
弱网/大目录下，全量 `read_dir` 的数据量是增量的一百倍以上，必须避免。

严格强一致不可能（目录是外部可变资源，任何读都是快照）；本方案做到
「亚秒级最终一致 + 溢出/超限/断线时才全量重读」。

## 一、现状与脚手架

- 协议已有 `SubscriptionType::FileChanges { path, recursive }`
  （quirel-protocol/src/subscription.rs L19-28）。
- agent 已有 EventBus + 订阅推送回路（agent/src/server/quic.rs L1271-1361）；
  客户端有 `subscribe/unsubscribe` 与 `subscription_event` 转发（connection.rs L1742-1950）；
  前端已有 `useFileChangesSubscription`（src/hooks/useSubscription.ts L90-110）。
- 缺口：agent 采集器未实现（非 metrics 直接 `bail!`）；客户端每连接只允许一个订阅
  （metrics 已占用，FileManager 再订阅会被静默忽略）；FileManager 未接入；
  刷新路径的旧目录回跳/选区丢失/旧读覆盖补丁仍需修。

## 二、设计

### 1) Agent：inotify 采集器（文件级增量）

监听当前目录（非递归），mask：`CREATE|DELETE|MODIFY|ATTRIB|MOVED_FROM|MOVED_TO|CLOSE_WRITE`。

- **按 100ms 窗口合并**（复用 `file_changes_delay_ms`），窗口内按条目名归并：
  - 新建 / 移入 / 写入完成 / 属性变化 → 需要条目元数据（upsert）
  - 删除 / 移出 → remove
  - `MOVED_FROM` + `MOVED_TO` 用 inotify cookie 配对 → rename（保留目录改名后
    搜索结果的后代路径改写语义）
- **一次 fork 批量 stat**：窗口结束时用目标用户身份（复用 `UserExecutor`）
  对需要 upsert 的名称做一次批量 metadata → 构造 `FileEntry`；
  找不到（窗口内又被删）→ 转 remove。
- 推送 payload（`Event{event_type:"file_changes"}`）：
  `{ dir, upserts: [FileEntry], removes: [name], renames: [{from,to,entry}] }`；
  `IN_Q_OVERFLOW` → `{ dir, resync: true }`。
- **批次上限**：单窗口变更条目 > 200 条（如解压几千个文件）→ 直接 `resync`
  （全量读一次比推几千条更省）。
- 限速：同一目录 ≤2 次/秒推送（合并窗口自然满足）；每连接 watch 上限 4、全局 256。
- 权限：订阅时对路径做 `validate_path` + 目标用户可读性探测；不通过拒绝订阅。
  stat 以目标用户身份执行，避免 root 视角泄露无权限条目的元数据。

### 2) Agent：SubscriptionManager
- 记录每 stream 的订阅类型（已有）+ 全局 `(uid, path) -> 引用计数`；
  首个订阅启动 watch，最后一个取消时移除 watch。
- 订阅失败返回 `SubscribeAck{success:false}`（而非直接断流），客户端可明确降级。
- 事件推送按 stream 的订阅类型过滤（当前是全局 `has_subscribers`，多流会互发无关事件）。

### 3) 客户端：多订阅流
- `ActiveConnection.subscription_task` → `subscriptions: HashMap<String, JoinHandle>`；
  `subscribe/unsubscribe` 增加 `key`（缺省按 types 生成，兼容 metrics 现有调用）；
  每个 key 一条独立 QUIC 流与监听任务；断开时 abort 全部。
- `file_changes` 事件沿用现有 `subscription_event` 转发。

### 4) 前端：FileManager 接入
- 使用 `useFileChangesSubscription(serverId, connected, currentPath, false, handler)`
  （增加 `connected` 依赖：重连后自动重新订阅）。
- handler 按事件内容处理，**不发全量请求**：
  - `upserts/renames/removes` → `applyViewChanges`（浏览 + 搜索同时补丁、重排、
    清理被删路径的选区）；目录事件不匹配当前目录则忽略；
  - `resync: true` → 全量重读（带守卫）；
  - 事件本身已合并/限速，前端再 100-150ms 防抖合并多次推送。
- 全量 `read_dir` 只在：首次进入目录、`resync`、单批超限、断线重连后补读。
- 降级（旧 Agent/无权限/watch 超限）：订阅失败 warn 一次，回到
  「upload-completed 定向补丁 + 窗口重新激活重读 + 30s TTL + 手动刷新」。
- 保留现有本地增量补丁作为即时反馈；服务端增量随后到达，重复项由 upsert 幂等吸收。

### 5) 一致性守卫（必需）
- 所有异步回调读 `currentPathRef`，不用渲染闭包目录（防拉回旧目录）；
- `loadDir` 增加 `keepSelection`；刷新保留仍存在的选区与滚动位置；
- `applyViewChanges` 搜索补丁读 `searchQueryRef` 最新值；
- 目录 `revision` 单调递增：本地补丁与被接受的重读都递增，过期 `loadDir` 结果丢弃；
- 内联重命名期间跳过外部更新，结束后下一次事件/TTL 补齐。

## 三、弱网 / 性能分析（为什么必须是文件级）

| 场景 | 全量重读 | 文件级增量 |
|---|---|---|
| 10k 条目目录，新增 1 个文件 | 一次 fork + 10k stat + 解析 passwd/group + 约 1.5-2.5MB 传输 | 1 次 fork 仅 stat 1 个条目 + 约 200B 推送 |
| 1 Mbps 弱网下的延迟 | 十几秒级，列表卡住 | 亚秒级 |
| 解压 3k 个文件 | 一次全量（可接受，若逐事件则灾难） | 超限阈值 → 自动转 resync 一次全量 |
| 编辑器保存 | 全量 | 单条 upsert（大小/mtime） |

结论：**增量推送把“变更成本”从目录规模解耦**；全量只发生在首次加载与重同步。
首次加载大目录在弱网仍慢（本次不做分页/流式首屏，可作为后续项）。

服务端成本：inotify 内核事件可忽略；每个窗口一次 fork + N 次 stat；
100ms 合并与 200 条上限把最坏情况钉死。客户端无额外 RTT（agent 主动推）。

## 四、代码量（估算）

- agent：inotify 采集器（事件归并、rename cookie、批量 stat、resync/限速）+ 订阅管理
  + 权限校验 + 测试：~450-650 行
- agent：订阅回执/按流过滤小改：~40-80 行
- 客户端：多订阅流 + key + 清理 + 重连：~150-250 行
- 前端：hook 参数 + FileManager 增量应用 + 守卫/选区/revision：~120-200 行
- 合计约 **750-1150 行**（协议与前端钩子已存在）。

## 五、能做到什么程度

- 当前浏览目录的变更（本机/解压/编辑器/终端/其他客户端）端到端约 100-300ms；
- 事件溢出、断线、单批超限时自动全量重同步收敛；
- 递归子目录不实时（非递归）；首次加载大目录在弱网仍取决于目录规模；
- 无 inotify 的旧 Agent 降级为激活/TTL/手动刷新。

## 六、验证

1. `cargo check/test`（agent、client）；`npx tsc --noEmit`。
2. 手工：
   - 小目录/大目录（>5k 条目）分别做：新建、删除、改名、保存、上传、解压；
     大目录下只应传输增量（日志核对推送条目数），弱网/限速条件下列表不整屏卡住；
   - 终端 `touch`/`rm`/`mv` 当前目录 → 1 秒内出现/消失/改名；
   - 解压 3000 条目 → 触发一次 resync，列表最终正确、无重复；
   - 循环 `touch` 100 次 → 限速与合并生效，无风暴、无重复条目；
   - 断网重连 → 自动重新订阅并补读；
   - 旧 Agent → 不刷错误，降级可用；
   - 无权限目录 → 订阅被拒，不泄露事件。
