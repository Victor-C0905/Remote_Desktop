# Quirel 客户端第八轮测试报告：修复后纯验证（零修改轮）— 供后续 AI 使用

- 日期：2026-11-12（会话时间 08:12–08:20）
- 被测：`fix/file-manager-sidebar` @ `54117ef`，debug 构建（exe 2026-11-12 03:15，含第七轮全部修复）
- **本轮约束：零代码修改**——只测试与记录。发现的未提交修改与新问题全部原样记录于 §五/§六。
- 本文档写给**下一个 AI 会话**：包含精确环境快照、可复现命令、已验证的自动化配方、
  已知陷阱与未决问题台账。人类可读性让位于信息密度。

## 一、环境快照（接手前必读）

```
分支：fix/file-manager-sidebar @ 54117ef（领先 dev：侧边栏 5 提交 + 修复 9 提交 + 测试报告 5 提交）
工作区：2 个 tracked 文件未提交（见 §五-0）；.trae/documents/ 两个未跟踪计划文档（非本分支产物）
二进制：target/debug/quirel.exe（2026-11-12 03:15，含全部修复）；devUrl ← vite:1420
WSL：Ubuntu-22.04；quireld 运行中（~/quirel-r4/agent/target/release/quireld，
     配置 /var/lib/quireld-probe/quireld-r4.toml，:8443，含会话击杀修复）
凭据：localhost 服务器密码已于本轮重录为 r5test123（PAM common-auth）；
      注意第五轮「清除所有本地数据」会清空 Windows 凭据管理器条目，.bin 还原不恢复凭据
客户端数据：%APPDATA%/com.quirel.app.dev/（dev profile）；filemanager.bin 为明文 JSON
壁纸目录已被第五轮 S-03 擦除（WallpaperStore 启动 ERROR 属预期残留，非缺陷）
前端：vite dev server 必须先起（debug exe 连 devUrl:1420）；CDP 端口 9223
```

**启动配方（实测顺序敏感）**：
```bash
npm run dev &                        # vite 先起（≤10s）
wsl -d Ubuntu-22.04 -e bash -lc "cd /root/quirel-r4/agent && setsid nohup \
  ./target/release/quireld --config /var/lib/quireld-probe/quireld-r4.toml > /tmp/quireld-r4-run.log 2>&1 &"
(WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9223" ./target/debug/quirel.exe &)
sleep 10
node "$TEMP/quirel-qa/quirel-qa.mjs" targets   # 应看到 http://localhost:1420/
```
**连接**：启动后**不自动重连**（时有时无，见 §六-3）——手动：点 TopBar chip → 点 .st-server-item
首行 → 点最后一个未禁用的「连接」。实测 RTT ~5ms。凭据缺失（tokenExists:false）时需在
编辑对话框重录密码（本轮后密码 = r5test123）。

## 二、测试结果总表（全部基于实机 CDP 驱动）

| 编号 | 项 | 结果 | 证据要点 |
|---|---|---|---|
| R8-01 | 启动 + 手动连接 | ✅ | QUIC 成功 RTT 4.7ms |
| R8-02 | 侧边栏新形态 | ✅ | 分区=[位置:1 项(主目录)]；常用无合格条目时整组隐藏；无设备组 |
| R8-03 | 搜索子串 | ✅ | `ash_h` → [.bash_history]（中缀命中，前缀语义不可能） |
| R8-04 | 右键隐式选中 | ✅ | 右键未选中的 .bashrc → 文件菜单弹出 + 该行进入 .selected |
| R8-05 | 排序翻转 | ✅ | 点名称表头 → 目录降序（新建文件夹→volumes→snap） |
| R8-06 | 地址栏导航失败链 | ⚠️ **自动化受阻** | 见 §六-1：CDP 真实键入对本应用不可靠（typex 报 chars=31 实际 20、未触发导航、path input 被清空）；合成 set-value 路径 R6/R7 已验证（回读+通知+横幅） |
| R8-07 | 终端关窗终止会话 | ✅ | `sleep 800` → 关窗 → `ps -eo pid,comm | grep -w sleep` 空；quireld 日志「收到 Close，终止会话…status=1」 |
| R8-08 | 断线自动重连 | ⚠️ 未重测 | 本轮未执行 kill 场景（第七轮已实证：+40s 持续重连、恢复后 ≤30s 自动接上） |
| R8-09 | Hex ASCII 对比度 | ✅ | sample.bin → FileOpener 路由 HexViewer；`.hv-ascii` computed = rgba(0,0,0,0.87) on light ✓ |
| R8-10 | 设置/关于 | ✅ | 版本 0.3.3-dev + `fix/file-manager-sidebar@v0.3.3-18-g1ebdf55` |
| R8-11 | console | ⚠️ | **新发现 R8-N1**（§六-1）：终端卸载瞬间 `Uncaught TypeError: r is not a function`；另有历轮既有 404 资源噪音 |

## 三、已确认良好的行为基线（回归时可直接比对）

- 连接后 FM：/root 列表 25 行（13 文件夹 + 12 文件），状态栏「13 个文件夹, 12 个文件 总大小: 49.6 KB」
- 搜索清空后浏览列表恢复；搜索态状态栏与列表行数一致
- file_changes：WSL `touch` → 列表 ~2s 内自动出现；外部 `rm` → ~2s 自动移除
- 演示模式：未连接开终端显示「[演示模式] 输入 help 查看可用命令」，连接后自动升级真会话
- 断线占位：FM 显示「无远程连接」+ 内联可用服务器快连列表
- 通知中心：计数口径当前数据 100=14+72+14（无缺类条目）；「其他」chip 为 0 时隐藏

## 四、本轮新发现

### R8-N1 终端窗口卸载瞬间未捕获异常（P3，新）

- 现象：关闭正在运行 `sleep 800` 的终端窗口时，console 出现
  `Uncaught TypeError: r is not a function`（时间戳与 teardown 精确吻合）。
- 会话击杀本身**成功**（异常是 teardown 链中的次生错误，不影响主流程），
  但 uncaught 异常可能中断同一任务内后续清理代码。
- 排查建议：在 main.tsx 挂 `window.addEventListener('error')` 捕获 stack；
  嫌疑面：teardownNow 中 `terminal.dispose()`（WebGL addon 释放回调）与
  unlistenRefs 清理的第三方回调。注意 debug 构建未压缩，`r` 命名说明异常来自
  依赖预打包 chunk。

### R8-N2 启动自动重连时有时无（观察项，非本轮引入）

- 08:12 启动：未自动重连（chip 未连接）；03:15 启动：自动重连成功。
- 两次前置状态相同（上次连接中强杀）。疑似启动时序竞态。已有指数退避兜底
  手动场景；如需定位，优先查 ServerManager 挂载时的恢复连接调度。

### R8-N3 vite/quireld/应用三者会随时间全部退出

- 两轮会话间隔（03:15→08:12）后三者全停。quireld 停止原因未查（可能 WSL 空闲回收）；
  vite npm 后台任务同样退场。**每轮开始必须按 §一配方重建环境**。

## 五、工作区待办（下一会话第一件事）

0. **未提交孤儿修改**：`src/i18n/locales/{zh-CN,en-US}.ts` 的 lostAction 文案
   （「最多 3 次」→「将持续重试直至恢复」）——这是 `1ebdf55`（重连退避）的配套文案，
   当轮漏提交。**处理建议：直接提交** `fix(ui): 重连通知文案与退避策略同步`。
   本轮按零修改约束未动。
1. e2e 冒烟套件在 `feature/e2e-smoke` 分支（E2E-05 假阳性已修）；合并侧边栏分支后
   E2E-05 自动转为真实断言。
2. WSL 夹具：`/root/r5-scratch/`（含 a.txt/b.txt/dir1/link-*/pipe.fifo/sample.bin）
   保留供复测；`/root/prune-x`、`/root/Documents`、`/root/Downloads` 已清理。
3. 第七/八轮报告均在 `docs/testing/`；第五轮报告 §六有验证方法修正（pgrep 自匹配陷阱）。

## 六、自动化配方与陷阱（针对本应用实测沉淀）

### 可靠做法
- **元素定位**：evaljs 现场计算 csspath → 立即 quirel-qa click（间隔 >300ms 会因
  React 重渲染失效）；或全部在同一次 evaljs 内完成（配合 el.click()）。
- **可 JS 点击**：窗口关闭/最小化按钮、Dock 项、设置侧栏项、右键菜单项（React onClick）。
- **必须协议点击**：TopBar chip（指针处理）。判别法：JS click 无效 → 改协议点击。
- **CJK**：bash 命令行传参会损坏中文实参——evaljs 内一律用 `\uXXXX` 转义；
  typex 实测对 CJK 亦可靠（终端中文输入验证过）。
- **evaljs 支持 Promise**（会 await）；但偶发输出丢失，关键断言需二次确认。
- **状态栏/文本断言**：从 `document.body.textContent` 正则抓取最稳。
- **终端驱动**：`evaljs` 聚焦 `.xterm .xterm-helper-textarea` → typex → key Enter；
  输出读取用截图（WebGL 渲染器无 .xterm-rows DOM）。

### 已知陷阱（按踩雷频率排序）
1. `pgrep -f 'sleep N'` 自匹配探针 bash → 假阳性孤儿报告。用 `pgrep -x sleep`
   或 `ps -eo pid,comm | grep -w sleep`。
2. React 重渲染使 nth-of-type 路径在 ~300ms 间隙失效 → 路径计算与使用必须同帧。
3. TopBar chip / 窗口控制等指针处理 UI 对 JS `.click()` 无响应 → 协议点击。
4. `offsetParent` 对 position:fixed 元素恒 null → 不可用作可见性判据，用截图或
   `getBoundingClientRect` 面积。
5. grep 输出遇 GBK 控制台码页 mojibake → 用 UTF-8 字面 pattern 匹配会 miss，
   改用 `grep -a` + 十六进制无关的特征（数字/英文）。
6. quirel-qa 的 console N 只保留最近 N 条；关窗瞬间的诊断需立即抓取。
7. `wsl -e sh -c` 非登录 shell PATH 不含 rustup 工具链 → 用 `bash -lc`。
8. `pkill -f 'xxx'` 会自匹配承载命令的 bash → 用 `pkill -x` 或 PID。
9. Git Bash 的 `\\\\` 经多层折叠——文件路径一律正斜杠或写脚本文件。
10. filemanager.bin / notifications.bin 为明文 JSON（zustand persist 包裹），
    可停机后直接注入测试数据（注意 EXPECTED_STORE_VERSIONS 版本门）。

## 七、未决问题台账（按优先级，均未修）

| # | 问题 | 已知信息 | 建议下一步 |
|---|---|---|---|
| 1 | R8-N1 终端卸载 uncaught TypeError | 时间与 teardown 吻合；会话击杀不受影响 | window error 捕获 stack → 定位 dispose 链 |
| 2 | R6-?-02 首窗 display:none 竞态 | openApp 已加防重入集合；未再复现 | 复现时抓 WindowManager.minimize 诊断（埋点代码在 git 历史 54117ef^ 可考） |
| 3 | R6-?-01 agent 标准位置探测偶发 0 | 重启后恢复；代码无缓存 | 如再现：查当时并存的双 worker（ps 快照） |
| 4 | Settings 长提示 nowrap 截切（R3-P2） | 原始元素未定位 | 找 R3 证据截图（evidence-2026-11-11） |
| 5 | P2-05 主题切换浅蓝底 | 未复现；需 PrintWindow 级探针 | 按 window-flash-rules §7 |
| 6 | 启动自动重连时有时无（R8-N2） | 疑似启动时序竞态 | ServerManager 挂载恢复调度加日志 |
| 7 | 桌面右键菜单 / 总览窗口管理 | 用户确认为新功能，本轮不做 | 单独立项 |

## 八、本轮改动清单

- **代码：零修改**（用户约束）。
- 测试数据：新建 `/root/r5-scratch/sample.bin`（9B 二进制，Hex 验证用，保留）；
  其余环境仅启停（vite/quireld/应用）。
- 文档：本报告。
