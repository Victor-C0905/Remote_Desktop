# Quirel 客户端第四轮全量测试计划（2026-11-11）

- 被测分支：`fix/file-manager-sidebar`（基于 dev，含 0.3.3 与第三轮修复 squash `af065c2`）
- 前三轮：[第三轮计划](client-test-plan-round3-2026-11-11.md) / [第三轮报告](client-test-report-round3-2026-11-11.md)
- 本轮定位：**分层测试体系落地第一轮**——按「Vitest 单元层 → cargo Rust 层 → tauri-driver
  E2E 冒烟层」的分层标准组织全量测试，同时覆盖本分支专项回归与第三轮受阻项补测。
- 基本原则沿用第三轮：可复现、有证据；先排除测试自身伪影，再判定产品缺陷。

## 一、分层标准（采纳外部建议 + 本轮落地方式）

| 层 | 工具 | 覆盖 | 本轮状态 |
|---|---|---|---|
| L1 单元/组件 | Vitest + Testing Library（已有） | store、侧边栏、组件逻辑 | 直接全量跑 |
| L2 Rust 层 | cargo test | src-tauri / agent / quirel-protocol | 直接全量跑 |
| L3 E2E 冒烟 | tauri-driver + msedgedriver（新增） | 真实桌面客户端：真实 IPC、Rust 后端、原生窗口 | 本轮搭链路并跑冒烟 |
| L4 深度 UI 实测 | CDP（quirel-qa.mjs，既有） | 需要细粒度断言的分支专项 | 沿用 |

**依赖影响评估（用户红线：不得影响打包产物）**

- `tauri-driver`：cargo 全局工具（`~/.cargo/bin`），非项目依赖，零影响。
- `msedgedriver 157.0.4341.42`：独立 exe，放 `%TEMP%\quirel-qa\edgedriver\`，零影响。
- E2E 冒烟脚本：原生 WebDriver 协议（HTTP + Node fetch），零 npm 依赖，脚本置于
  `%TEMP%\quirel-qa\`（与既有 CDP 工具同目录），不进仓库。
- 本轮**不改 package.json / Cargo.toml**，打包产物构成零变化（B4 以 build:dev 实证）。
- WebdriverIO（devDependency）不影响打包（devDeps 不进 bundle），但产品化属独立任务，
  需另开分支，本轮不引入。

**对上会话「WebView2 损坏（0x800700AA）」结论的复核假设**：`0x800700AA = ERROR_BUSY`，
最可能是同 profile（`com.quirel.app.dev`）双实例并发导致 WebView2 用户数据目录锁冲突，
而非 Runtime 损坏——dev 日志中无任何 0x800700AA 记录，且 dev 应用当日 20:45 仍正常运行。
本轮以「严格单实例串行」实测验证。

## 二、基线与环境

| 维度 | 取值 |
|---|---|
| 构建 | target/debug/quirel.exe（devUrl ← vite 1420）；build:dev 安装包（B4） |
| WebView2 Runtime | 157.0.4341.42；msedgedriver 157.0.4341.42（精确匹配） |
| 服务器 | 本地 WSL quireld（release 版，`/var/lib/quirel-probe/quireld-probe.toml`，:8443，dev profile 已存凭据） |
| 数据隔离 | debug 构建 identifier `com.quirel.app.dev`，与 release 完全隔离；全程单实例 |
| 语言/DPI | zh-CN 为主，抽查 en-US；本机 150% DPI（不做系统级 DPI 切换，沿用第三轮环境） |
| 测试数据 | WSL `/root/quirel-test-scratch-r4/`（fixtures-r4.sh 已预置） |

## 三、回归重点（本分支 5 个提交）

| 提交 | 内容 | 风险 | 必测 |
|---|---|---|---|
| `7ffdfc9` | 侧边栏按服务器真实路径生成 | 路径规范化、跨服残留 | 换服后侧边栏条目 |
| `29eaf71` | 常用分组按目录访问统计生成 | 统计来源、空态 | 首次/常用区分 |
| `6094f4e` | 排序表头文字被高亮遮罩覆盖 | 纯视觉 | 深/浅主题表头 |
| `8d42f21` | 常用路径 frecency 权重模型 | 权重衰减、排序稳定性 | 访问-重排-衰减 |
| `62daad0` | frecency 场景测试 | — | L1 覆盖 |

新增代码：`fileSidebar.ts`（221 行）+ `filePathUsageStore.ts`（211 行）+ 383 行单测；
改动面：FileManager.tsx/css、usePreloader.ts、serversStore、storage.ts（迁移）。

## 四、E2E 冒烟场景（L3，只跑关键链路）

| 编号 | 场景 | 通过判据 |
|---|---|---|
| E2E-01 | 应用启动（tauri-driver 拉起 debug exe） | 主窗口出现，TopBar/Dock 渲染，无「界面发生错误」 |
| E2E-02 | 服务器连接（localhost:8443，存量凭据） | 状态转 connected，无证书告警 |
| E2E-03 | 打开文件管理器并进入目录 | 列表渲染，状态栏统计正确 |
| E2E-04 | 侧边栏导航（真实路径分组） | 侧边栏条目来自服务器路径，点击可导航 |
| E2E-05 | 常用分组显示 | 常用区域随访问生成/排序 |

> 约束：E2E-02~05 依赖 dev profile 存量凭据与 WSL quireld 存活；若凭据链路失效，
> 降级为 E2E-01 + UI 壳冒烟，并在报告中如实记录。

## 五、执行批次

| 批次 | 内容 | 出口 |
|---|---|---|
| B0 基线 | 环境/工具/WSL quireld/fixtures | 链路就绪，或明确阻塞点 |
| B1 单元层 | Vitest 全量（含第三轮 ESM 阻塞复核） | 全绿，或缺陷清单 |
| B2 Rust 层 | cargo test × 3 crate | 全绿，或缺陷清单 |
| B3 E2E 冒烟 | tauri-driver 链路 + §四场景 | 场景结论（含阻塞记录） |
| B4 打包验证 | build:dev + 产物构成对比（证明依赖零影响） | 安装包可产出，命名符合规范 |
| B5 分支专项 | CDP 实测 §三必测项 + 深水区（frecency 行为、迁移兼容） | 逐项结论 |
| B6 遗留复核 | 第三轮受阻/遗留项中本轮可低成本执行者 | 逐项结论 |
| B7 收尾 | 复现定级、清理测试数据、出报告 | 报告可评审 |

## 六、判定与报告

- P1：数据丢失/损坏、崩溃、核心流程不可用；P2：有绕行的功能缺陷、明显视觉/性能问题；
  P3：打磨/低频/观感。
- 第三轮遗留基线（本轮复核）：Hex ASCII 列对比度（P2）、Settings 长提示 nowrap 截切（P2）、
  `file_changes` 订阅静默降级无提示（P3）、reload 后终端假连接（P3）、P2-05 主题切换浅蓝底（未复现）。
- 每条记录：编号、模块、复现步骤、期望/实际、证据路径、严重级、状态。

## 七、安全与纪律

- WSL 操作仅限 `/root/quirel-test-scratch-r4/`，结束删除并核实。
- 不把密码、私钥、token 写入报告或日志；dev profile 的 servers.bin 仅读取非敏感字段。
- 严格单实例：同一时刻只允许一个 quirel.exe 挂 `com.quirel.app.dev` profile（B0 实证约束）。
- 证据目录：`docs/testing/evidence-2026-11-11/`（已 gitignore）。

## 八、交付物

- `docs/testing/client-test-report-round4-2026-11-11.md`（含证据索引、缺陷清单、
  分层测试体系落地结论与 WebdriverIO 产品化建议）。
