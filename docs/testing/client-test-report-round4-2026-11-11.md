# Quirel 客户端第四轮全量测试报告（2026-11-11）

- 被测分支：`fix/file-manager-sidebar`（`v0.3.3-5-g62daad0`，基于 dev，含 0.3.3 与第三轮修复 squash `af065c2`）
- 测试方式：分层测试体系首轮落地——L1 Vitest / L2 cargo test / L3 tauri-driver E2E 冒烟（新增）/ L4 CDP 深度实测
- 计划：[client-test-plan-round4-2026-11-11.md](client-test-plan-round4-2026-11-11.md)
- 结论：**L1/L3/L4 全绿，L2 发现 1 个 P2（两个 crate 的测试目标编译失败，dev 基线遗留）；
  分支 5 个提交的全部关键链路实测通过；tauri-driver 标准链路在本机打通（6/6），
  推翻上会话「WebView2 Runtime 损坏需重启」的结论；全程零项目依赖变更，打包产物零影响（实证）。**

---

## 一、缺陷与发现清单

| 编号 | 级别 | 模块 | 标题 | 状态 |
|---|---|---|---|---|
| R4-P2-01 | P2 | 测试门禁 | R3-P3-01 协议 `FileEntry` 新增 `is_symlink`/`link_target` 后，src-tauri 与 agent 两处测试初始化器未同步，`cargo test` 编译失败 | 待修复（dev 基线，非本分支引入） |
| R4-P3-01 | P3 | 常用路径 | `filePathUsageStore.ts:29` 注释写「衰减后分数 >= 6」，常量实为 `MIN_FREQUENT_SCORE = 5.5` | 注释漂移，待顺手修 |

### R4-P2-01 测试目标编译失败（1 个根因，2 处位置）

第三轮 P3-01 修复为协议 `FileEntry` 增加 `is_symlink`/`link_target` 字段（serde default 保持兼容），
但两处**测试代码**的 `FileEntry` 字面量初始化器没有补上新字段：

- `src-tauri/src/connection.rs:2865` → `error[E0063]: missing fields ... in initializer of quirel_protocol::FileEntry`
- `agent/src/manager/protocol_adapter.rs:556` → 同错误（`generated::FileEntry`）

影响：`cargo test --manifest-path src-tauri/Cargo.toml` 与 `cargo test --manifest-path agent/Cargo.toml`
均无法编译（lib 本体编译正常，仅 `lib test` 目标失败）——**Rust 单元测试门禁失效**（与第三轮
vitest ESM 阻塞同类，且更隐蔽：`cargo build`/`tauri dev` 完全无感）。
本分支未触碰 agent/，属 dev 基线问题。修复建议：两处初始化器补 `is_symlink: false, link_target: String::new()`
（或按测试场景填真实值）。

### R4-P3-01 frecency 门槛注释漂移

`src/stores/filePathUsageStore.ts:29-33`：注释「衰减后分数 >= 6 才进入常用」，常量 `MIN_FREQUENT_SCORE = 5.5`。
行为正确（单测覆盖），仅注释与实现不一致。

### 上会话「WebView2 损坏」结论推翻（重要澄清）

上会话报告称本机 WebView2 创建窗口报 `0x800700AA` / `0x8000FFFF`，E2E 无法运行、需重启修复 Runtime。
本轮实证：

1. `0x800700AA = ERROR_BUSY`，与 WebView2 **用户数据目录锁**（同 profile 双实例并发）特征一致；
2. dev 客户端日志（`client.log.2026-11-11`）中**无任何** 0x800700AA 记录，且上会话期间应用正常运行至 20:45；
3. 本轮 tauri-driver 冒烟在「严格单实例」下连续多次启动全部成功。

**结论：Runtime 未损坏；错误来自测试自身伪影（残留实例未清理 + 同 profile 互锁）。**
E2E 工具链必须带预清理（`taskkill quirel.exe` + 等待）作为标准步骤，已写入冒烟脚本。

---

## 二、分层结果

### L1 Vitest 单元/组件层 — 全绿

- `npx vitest run`：**61 个文件 / 568 个用例全部通过**（13.2s，本机）。
- 第三轮受阻项「vitest worker ERR_REQUIRE_ESM @csstools/css-calc」**已消失**（依赖状态变化，未改任何文件），窗口系统单测门禁恢复。
- 含本分支新增 383 行单测（`fileSidebar.test.ts` 178 行 + `filePathUsageStore.test.ts` 205 行）。

### L2 cargo test — protocol 通过，两个 crate 测试目标编译失败

| crate | 结果 |
|---|---|
| quirel-protocol | **37/37 通过** |
| src-tauri (quirel) | ❌ lib test 编译失败（R4-P2-01，connection.rs:2865） |
| agent (quireld) | ❌ lib test 编译失败（R4-P2-01，protocol_adapter.rs:556） |

### L3 tauri-driver E2E 冒烟（本轮新增） — 6/6 全绿

工具链：`tauri-driver`（cargo 全局安装）+ `msedgedriver 157.0.4341.42`（与 WebView2 Runtime
157.0.4341.42 精确匹配，`%TEMP%\quirel-qa\edgedriver\`）+ 原生 WebDriver 协议脚本
`%TEMP%\quirel-qa\wd-smoke.mjs`（**零 npm 依赖**，raw HTTP + fetch）。被测对象：`target/debug/quirel.exe`
（devUrl ← vite 1420）。

| 场景 | 结果 | 证据 |
|---|---|---|
| E2E-01 启动 + 壳渲染 | ✅ PASS | session 创建（tauri-driver→msedgedriver→WebView2 附加成功），title=Quirel |
| E2E-02 连接 localhost:8443 | ✅ PASS | TopBar chip「已连接 本地服务器」；密钥环存量凭据 + PAM 密码认证 |
| E2E-03 打开文件管理器 | ✅ PASS | 25 行列表；状态栏「12 个文件夹, 13 个文件 总大小 49.0 KB」 |
| E2E-04 侧边栏真实路径分组 | ✅ PASS | 位置(6 XDG 项) + 设备(19 挂载项) n=25 |
| E2E-05 常用分组显示 | ✅ PASS | frecency 条目 `["/root"]` |
| TEARDOWN 会话关闭 | ✅ PASS | 无残留进程 |

服务端旁证：quireld 日志 `✅ 密码认证成功: remote=127.0.0.1:59953, user=root`。

过程中定位并解决的冒烟脚本问题（均为测试自身问题，非产品缺陷，已沉淀为工具经验）：
WebDriver 定位策略名必须用 `css selector`；msedgedriver `/click` 端点需空 JSON body；
TopBar chip 可访问名来自 `title` 属性；TopBar 指针处理器需协议级点击（JS `.click()` 仅对
普通 onClick 元素有效）；会话启动时序需显式轮询（建议 ≤20s）；inline 调试命令的反斜杠
会被 shell 层层折叠（调试脚本必须落盘文件）。

### L4 CDP 深度实测（分支专项） — 通过

环境：dev 客户端（`0.3.3-dev` / `fix/file-manager-sidebar@v0.3.3-5-g62daad0`）+ WSL quireld
（**从当前源码**新构建的 release 版，`/var/lib/quireld-probe/quireld-r4.toml`，:8443）。

| 项 | 结果 |
|---|---|
| 侧边栏按服务器真实路径生成（`7ffdfc9`） | ✅ 位置=服务器 XDG 目录；设备=真实挂载点+使用率（/ 7%、/mnt/c 62%、/snap/* 100%…） |
| 常用分组生成与排序（`29eaf71`/`8d42f21`） | ✅ 分组顺序 常用→位置→设备；条目 tooltip 显示完整路径（`/root`）；落盘分数 9.99 过线（迁移旧数据+新计分） |
| frecency 门槛与去抖行为 | ✅ 单次访问不过线（sidebar 5 < 5.5）；30 分钟 SESSION_GAP 内重复进入同目录只刷新 lastUsed 不加分（三次双击仅计 1 次，2 分 < 5.5 不显示）——与设计注释「单次访问无法过线」一致 |
| 排序表头高亮遮罩（`6094f4e`） | ✅ 列表视图表头文字清晰（名称^/大小/修改时间/类型/权限），无遮罩覆盖 |
| `file_changes` 目录实时刷新（新 quireld 回归） | ✅ WSL `touch` 新文件 ~2.5s 内自动出现在列表（本轮全新构建的 quireld 验证，非第三轮旧二进制） |
| 连接链路 | ✅ 存量凭据（keyring）+ PAM 密码认证一次成功 |

证据截图：`docs/testing/evidence-2026-11-11/sm-open.png`（服务器管理）、`fm-open.png`（文件管理器+
侧边栏真实路径）、`fm-frequent.png`（常用分组）。

### B4 打包验证 — 通过

- `npm run build:dev` 产出 `target/release/bundle/nsis/Quirel_0.3.3-dev.5-g62daad0_x64-setup.exe`
  ——命名符合 `<版本>-dev.N-g<sha>` 规范，未写任何版本文件；
- **本轮零依赖变更**（package.json / package-lock.json / Cargo.toml / Cargo.lock 均未改动，
  git status 实证），打包产物构成与 0.3.3 完全一致——「第三方依赖不影响打包」以最强形式满足：
  没有引入任何依赖。tauri-driver/msedgedriver/测试脚本全部位于仓库外（`~/.cargo` 与 `%TEMP%`）。

---

## 三、未执行 / 受阻项（沿第三轮清单，本轮未覆盖）

| 项 | 原因 |
|---|---|
| 跨服务器侧边栏残留专项（`7ffdfc9` 换服场景） | 本轮仅 WSL 单服务器可用（泰国服第三轮断开后未恢复）；无第二服务器可切换 |
| Hex ASCII 列对比度、Settings 长提示 nowrap（第三轮遗留 P2） | 需要专项会话逐项复核，本轮时间盒未纳入 |
| P2-05 主题切换浅蓝底复测 | 第三轮探针未复现，需窗口级捕获链路（坏帧文档 §7），单独立项 |
| DPI 125%/150% 矩阵、双屏、F11 全屏 | 环境限制（沿用第三轮 150% DPI 单屏） |
| 30–60 分钟长稳 / 资源泄漏曲线 | 未执行 |
| 通知跨服隔离 UI（`.trae` 新计划文档主题） | 独立任务分支范畴，未并入本轮 |

---

## 四、工具链沉淀（%TEMP%\quirel-qa\，均在仓库外）

| 工具 | 用途 |
|---|---|
| `wd-smoke.mjs` | tauri-driver E2E 冒烟套件（原生 WebDriver 协议，零依赖，E2E-01..05 + 预清理 + 会话重试） |
| `wd-debug-{chip,locate,click}.mjs` | 定位策略/点击协议调试脚本（本轮排障过程沉淀） |
| `edgedriver/msedgedriver.exe` | 157.0.4341.42（须与 WebView2 Runtime 主版本精确匹配） |
| `quireld-r4.toml`（WSL /var/lib/quireld-probe/） | 当前 schema 探针配置（旧 probe.toml 含已废弃 `security` 段不可用） |

## 五、清理记录

- WSL：`/root/r4-watch-probe-*.txt` 已删除并核实不存在；`/root/quirel-test-scratch-r4/` 未创建
  （本轮夹具未用到该目录）；quireld（当前源码构建）仍在 WSL 运行供复测，配置/日志路径见上；
- Windows：quirel.exe / msedgedriver / tauri-driver / vite 均已停止，无残留实例；
- 证据目录 `docs/testing/evidence-2026-11-11/`（已 gitignore）新增 3 张截图；
- 工作区仅新增本报告与计划两份文档，无其他改动。

## 六、后续建议（按优先级）

1. **修复 R4-P2-01**（两处测试初始化器，恢复 cargo test 门禁）——建议 `fix/test-fileentry-init` 分支；
2. **WebdriverIO 产品化**：冒烟脚本已验证标准链路，若要长期维护建议单开
   `feature/e2e-smoke` 分支引入 `@wdio/*`（devDependencies，不进打包产物）+ `wdio.conf.ts`，
   并把 E2E-01..05 迁移为正式 spec；本轮零依赖方案可作为 CI 前的手动门禁继续使用；
3. **单实例纪律入工具**：所有 E2E/CDP 启动前强制 `taskkill quirel.exe` + 等待（防止 0x800700AA 伪影复发）；
4. 顺手修 `MIN_FREQUENT_SCORE` 注释漂移（R4-P3-01）；
5. 恢复第二台测试服务器（泰国服）后，补跨服务器侧边栏残留专项。
