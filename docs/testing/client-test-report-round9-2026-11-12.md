# Quirel 客户端第九轮测试报告：换角度专项（2026-11-12）

- 被测：`fix/file-manager-sidebar` @ `54117ef`（同第七轮构建，零代码修改轮延续）
- 本轮定位：**换角度**——覆盖前八轮未触及的故障模式与交互视角：
  挂起（SIGSTOP）而非杀死、双编辑器一致性、连接循环泄漏、纯键盘操作、
  失败通知风暴、kill -9 数据完整性。
- 前置阅读：[第八轮报告](client-test-report-round8-2026-11-12.md) §一环境快照与 §六陷阱清单
  对本轮完全适用（环境同源）。

## 一、结果总表

| 角度 | 场景 | 结果 |
|---|---|---|
| A | **SIGSTOP 挂起服务器**（静默故障，非杀死） | ✅ 冻结后 ≤12s 客户端检出并进入「重连中...」；SIGCONT 解冻后 ≤15s 自动恢复连接。悬挂期间退避重试持续（验证了第七轮退避改造在真实悬挂场景的表现） |
| B | 双编辑器打开同一文件 | ⚠️ 部分覆盖：两窗口内容独立、无实时同步；**JS 聚焦未激活窗口后 Ctrl+S 不保存**（R3-P1-01 按窗隔离正确生效，防跨窗误保存 ✓）。真实激活后的保存与冲突检测路径受自动化工具限制，留人工 |
| C | 连接/断开循环 ×5 | ✅ 终态已连接；服务端 manager+worker 进程数恒为 2（无 worker 泄漏）；QUIC 连接收支 9 建 7 关（差额=当前活跃连接+计数口径差，无异常增长） |
| D | 纯键盘 FM 操作 | ✅ 行选中后 ArrowDown 移动选中正常（重载完成后的稳态下） |
| F | 失败通知风暴 | ⚠️ 部分覆盖：合成输入链在本会话不稳定（evaljs Promise 输出丢失 + 侧效未发生）。已有证据：R6 实证同类失败通知可在中心堆叠（3 条同型条目）；toast 突发捕获需 OS 级输入 |
| I | **kill -9 强杀应用 → 数据完整性** | ✅ 5 个 .bin 校验和仅 servers.bin 因重连语义更新（预期）；无 storage-recovery 对话框；3 服务器完整保留；重录凭据后重连即通 |
| — | 启动自动重连 | ⚠️ 再次观察「时有时无」（R8-N2 累计 2/4 次触发），维持观察项 |

## 二、角度 A 详录（最有价值发现）

故障注入：`kill -STOP $(pgrep -x quireld)`（manager+worker 全冻结，TCP/QUIC 无 FIN、纯静默）。

```
T+0    SIGSTOP，chip=已连接
T+12s  chip=重连中...（客户端心跳/发送超时检出静默故障）
T+32s  chip=重连中...（指数退避对冻结服务端持续尝试）
T+32s  SIGCONT 解冻
T+47s  chip=已连接（自动恢复）
T+67s  chip=已连接（稳定）
```

结论：**静默故障模式的检出、持续重试、自动恢复全链路 ✅**——第七轮退避改造
在「冻结后恢复」这类最常见真实故障（网络抖动/服务端卡顿/宿主休眠）下表现正确。
旧逻辑（3 次×5s）在同类场景会于冻结期内耗尽并永久放弃。

## 三、新发现

| # | 现象 | 倾向 |
|---|---|---|
| R9-N1 | 角度 B：JS 聚焦未激活编辑器后 Ctrl+S 被忽略 | **非缺陷**——R3-P1-01 按窗键盘隔离的正确表现（防跨窗误保存），记录为行为基线 |
| R9-N2 | 角度 C 后 FM 列表一度 0 行（重连后 ~3s 内） | 重连后 FM 初始加载竞态窗口，随后 25 行恢复——**低优观察项**，如复现配合截图 |
| R9-N3 | 启动自动重连再次未触发（累计 2/4） | 维持 R8-N2 观察项 |

## 四、验证方法补充（追加至第八轮 §六陷阱清单）

11. **编辑器（CodeMirror）输入**：`cm-content.focus()` 只聚焦不激活窗口——跨窗快捷键
    （Ctrl+S）会被 R3-P1-01 隔离拦截。要测「激活窗口的保存」必须真实点击窗口内容。
12. **FM 列表行数断言**：断线重连后 ~3s 内列表可能处于重载空窗，行数断言需等待稳态；
    网格模式下 `.fm-list-row` 恒为 0（类名差异，非空列表）。
13. **async evaljs 链**：多步 await 链的输出丢失率高于单步——每步副作用用独立断言
    验证（文件系统/落盘/通知计数），不依赖链返回值。
14. **通知计数**：面板「全部 (N)」读取需在面板打开态；badge 文本有「9+」饱和形态。

## 五、清理与状态

- WSL：/root/r5-scratch/ 夹具保留（含 sample.bin）；/root/prune-x 已于第七轮清理；
  quireld 运行中（本轮经历 STOP/CONT 与 5 次重连循环，状态健康）。
- 应用运行中，已连接本地服务器；dev profile 数据零污染（校验和比对通过）。
- 本轮代码零修改 ✓（lostAction 孤儿文案仍未提交，见第八轮 §五-0）。

## 六、修复记录（2026-11-15，`fix/file-manager-sidebar` 分支）

按第八轮 §七 未决问题台账推进，本轮处理 2 项可修项 + 1 项结案归档：

### 1. R8-N1 终端卸载 uncaught TypeError（已修复，等实机复验）

- **定性**：`r is not a function` 来自依赖预打包 chunk 内部；确切抛点无 stack
  无法归因，但抛出通道属「dispose 后仍触达已销毁终端」一类。三个已确认通道：
  1. 飞行中事件——unlisten 是异步 invoke，清理发起后 Rust 侧早已发出的
     `terminal-output` / `terminal-disconnected` 仍可送达 JS，回调对已
     dispose 的终端 `write()`；
  2. 清理链裸 forEach——任一 unlisten 在 chunk 内抛同步错即中断整链并
     升级为 uncaught（三处：teardownNow / 幽灵会话补清 / 断连回调）；
  3. `searchAddonRef` 不清空，残留指向已释放实例。
- **修复**：①`connectRemotePty` 注册 `sessionDead` 死标记 setter 进
  unlistenRefs，两个事件回调入口先查标记（teardown/断连清理都会置位）；
  ②新增 `src/utils/unlisten.ts` `safeUnlistenAll`（逐个吞错不断链）替换
  三处裸 forEach；③teardownNow 清 `searchAddonRef`；④新增
  `installGlobalErrorReporting`（environment.ts，main.tsx 安装）——uncaught
  error 的完整 stack 经 logger 落盘 client.log，下次出现可直接归因。
- **测试**：`unlisten.test.ts`（4 条，含 R8-N1 回归用例）、
  `environment.test.ts`（2 条）。

### 2. 启动自动重连时有时无（R8-N2/R9-N3，已按设计修复确定性）

- **定性澄清**：冷启动不自动重连是**既定设计**（StorageInitializer 纪元闸门
  故意清空会话镜像，「关闭项目 = 干净桌面」；纪元 = PID-时间戳，每次启动必
  不同）。实测到的「2/4 成功」是闸门被绕过的副作用，非功能时序竞态：
  闸门与存储预加载共居同一 try，预加载抛错 → catch 直通渲染 → 上一世代
  `sm.session` 泄漏进新进程 → 恢复路径照常消费。dev 环境（R8-N3 三者全停）
  预加载失败概率不低，故时有时无。
- **修复**（用户拍板方向：按设计修确定性，不做启动自动重连功能）：
  ①纪元闸门独立成块、移到预加载之前，catch 内 fail-closed（纪元读取失败 =
  无条件清镜像；纯浏览器调试模式整体豁免）；②会话镜像统一盖纪元戳
  （`src/utils/sessionMirror.ts`：`serialize/parse/clearSessionMirrors`），
  ServerManager 写入带戳、消费端校验纪元——闸门再被绕过时消费端仍拒绝
  （纵深防御）；③纪元读取失败时本轮放弃恢复且不写镜像（fail-closed）。
- **测试**：`sessionMirror.test.ts`（8 条：纪元不匹配/旧版无戳/损坏 JSON/
  结构非法一律拒绝）。

### 3. Settings 长提示 nowrap 截切（R3-P2）：静态审计结案，不改码

- 审计 Settings.css 全部 12 处 nowrap：或带 ellipsis 兜底（`.st-sb-label`/
  `.st-conn-value`/`.st-server-*` 等），或内容为固定短词（`未连接`、`QUIC`、
  侧栏标签），无「长文本 + nowrap + 无溢出处理」组合；`.st-form-hint`/
  `.st-hint` 本身可换行。与第七轮「原始元素无法定位、盲目改 CSS 有回归
  风险」一致，维持不改码。
- **下轮定位配方**：在设置各页于窄窗口下执行
  `document.querySelectorAll('*')` 过滤 `el.scrollWidth > el.clientWidth + 1
  && getComputedStyle(el).whiteSpace === 'nowrap'` 的文本元素——命中即原始
  缺陷元素（若为空则该缺陷已被历轮布局重构消除，可销项）。

### 验证

- `npx tsc --noEmit` 干净；`node scripts/check-sensitive-logs.mjs` 通过。
- `NODE_OPTIONS=--experimental-require-module npx vitest run`：64 文件 / 579
  用例全绿（含新增 14 条）。
- 注：`npm run verify:fe` / `verify:full`（dev `3c59b60` 引入的门禁脚本）尚未
  进入本分支，上两条为等价手工执行；分支合并 dev 后门禁自动生效。

### 第十轮待验证项

1. 关闭带活动的远程终端窗口（如运行 `sleep 800`）：console 应无 uncaught
   TypeError；若复现，GlobalErrors 日志现带完整 stack，可直接归因。
2. 连接中强杀客户端 → 冷启动：应**确定性**显示「未连接」（连续多次启动
   行为一致）；页面刷新（同进程）→ 应照常恢复连接（纪元一致路径）。
3. 按上文配方复查 Settings 截切元素（命中则修，空则销项）。
4. R9-N2（重连后 FM 列表 ~3s 空窗）——本批已修复假空态（见 §七-4），改为
   复验：重连窗口应显示 spinner 而非「空目录」占位。

## 七、修复记录第二批（2026-11-15 晚，同分支）

台账剩余项继续处理 3 项（P2-05 维持待复现不动）：

### 4. R9-N2 重连后 FM 列表 ~3s 空窗（已修复，等实机复验）

- **根因**（静态定位）：重连清空 effect 把视图置成 entries=[]/loading=false
  的真空态，渲染落在「空目录」占位而非 spinner；恢复链被 homeDir 清空串行
  门控（断线清 homeDir → 重连需先 `remote_get_current_user` 再 loadDir），
  两次串行 RPC + 会话冷启动走完前一直是假空态。无重试链路、无首快照
  （file_watcher 为纯 inotify 增量），「自动恢复」即恢复链自然走完。
- **修复**（`6781074`）：重连清空 effect 改置 loading=true（假空态 → 加载中）；
  get_current_user 失败路径补 setError + setLoading(false)（失败可见化，
  同 R6-P2-01 原则；防 homeDir=null 时 spinner 卡死）。
- **残留已知项**（不改，留观察）：重连 effect 的 `setCurrentPath("/")` 使
  file_changes 订阅抖动（/root→/→/root），resync 经 refreshAfterMutation
  以更高 epoch 丢弃在途响应（放大器，触发需 ~1s 窗口内出现事件）；
  homeDir 按服务器缓存可省一次串行 RPC（后续优化候选）。
- 说明：FM 组件挂载级复现测试因 mock 成本过高未入库，修复正确性由
  代码路径推演 + 第十轮实机复验闭合。

### 5. R6-?-02 首窗 display:none 竞态（已修复，等实机复验）

- **根因**（静态定位，替代原「时序竞态」猜测）：`openApp` 对已聚焦窗口执行
  最小化（三态循环设计），但 `manager.create` 对未注册 onBeforeCreate 的
  应用同步完成——「创建中防重入集合」对此是零宽窗口；双击 Dock 图标的
  第二下落进「已聚焦→最小化」把刚创建的窗口立刻 display:none。
  「挂载→卸载→重挂载 + 2×404」经查为 StrictMode 双跑 + Google Fonts 外链
  离线失败的测试伪影，不作为竞态证据。
- **修复**（`5115a79`）：`shell/dockAction.ts` 动作矩阵纯函数 + 创建后 500ms
  宽限期内最小化降级为聚焦；宽限期外三态循环不变（R2 基线保持）。
  测试 5 条入库。

### 6. R6-?-01 agent 标准位置探测陈旧（定性 + 加固 `fdbafb5`）

- **定性**：数据层无缓存（collect_common_locations 每请求实时 stat、worker
  不参与该请求、客户端无持久缓存）——陈旧层在「哪个进程在服务」：QUIC
  socket 的 SO_REUSEADDR（restart 端口释放延迟既有方案）在 Linux 下允许
  第二个进程静默绑定同一 UDP 端口，陈旧实例继续持有客户端连接。
  systemd `KillMode=process`（Phase 4 热更新刻意设计）+ update.sh
  自重启陷阱是运维侧成因。**非代码缓存缺陷，第八轮「代码无缓存」结论成立。**
- **加固**（`fdbafb5`）：Manager 启动实例锁（flock，锁文件置于配置同目录、
  按 QUIC 端口命名）——双实例静默共存改为大声失败（error 日志含对方 pid
  + 非零退出），flock 随进程死亡自动释放无陈锁；不同端口多实例互不影响。
  测试 3 条（红→绿）入库，agent lib 全量 172 用例通过。
- **运维建议**（未实施，供部署参考）：服务器可用 `ss -ulpn | grep <端口>`
  查重复绑定；`/proc/<pid>/exe` 比对各 quireld 二进制版本。

### 第二批验证

- `npx tsc --noEmit` 干净；vitest 65 文件 / **584 用例**全绿（新增
  dockAction 5 条）。
- agent（WSL）：`cargo check` 干净（仅存量 quic.rs:395 unused warning，
  非本次引入）；`cargo test --lib` **172 用例**全绿（新增 instance_lock 3 条）。

### 第十轮待验证项（追加）

5. 连接/断开循环后重连：FM 列表应显示 spinner（而非「空目录」占位）直至
   25 行出现；get_current_user 失败场景（如断网重连竞态）应显示错误而非空态。
6. 双击 Dock 图标打开首窗：窗口应立即正常显示；单击已聚焦窗口 Dock 图标
   仍应最小化（三态回归）。
7. 服务器建目录后重开 FM：标准位置应即时正确（数据层本就实时）；如再现
   陈旧值，按 §六-R6-?-01 检查是否存在双 quireld 进程（实例锁日志会报
   「另一 quireld 实例正在运行（PID …）」）。
