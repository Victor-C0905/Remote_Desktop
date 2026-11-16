# Quirel 客户端第十轮测试计划：两批修复实机验证（2026-11-15）

- 被测：`fix/file-manager-sidebar` @ `bc230b2`（工作树）
- 前端：vite dev 实时服务（含 §六/§七 全部 6 项修复）；客户端 Rust 零改动 →
  复用 `target/debug/quirel.exe`（2026-11-12 03:15，连 devUrl:1420）
- quireld：`/root/quirel-r4/agent/target/release/quireld` 重构建（含实例锁），
  配置 `/var/lib/quireld-probe/quireld-r4.toml`（:8443）
- 环境配方：沿用第八轮报告 §一（vite 先起 → quireld → 客户端 CDP:9223 →
  `node %TEMP%/quirel-qa/quirel-qa.mjs targets`）
- 前置阅读：第九轮报告 §六/§七（修复定性）+ 第八轮 §六陷阱清单

## 验证项与判据（对应第九轮 §六 1-4 + §七 5-7）

| # | 项 | 判据 |
|---|---|---|
| V1 | R8-N1 终端卸载 TypeError | 开终端跑 `sleep 800` → JS 点关窗 → console 无 uncaught TypeError；控制组：修复前该路径必现 |
| V2 | 启动重连确定性（R8-N2） | 连接中强杀客户端 → 冷启动 ×3 → chip 均为「未连接」（确定性，不再时有时无）；同进程 reload → 恢复连接 |
| V3 | Settings nowrap 配方 | 窄窗口下按 §六-3 配方枚举 `scrollWidth>clientWidth+1 && nowrap` 元素：命中则记录待修，空则销项 |
| V4 | FM 重连假空态（R9-N2） | kill quireld → 重连中 → 重启 quireld：FM 重连窗口应显示 spinner 而非「空目录」占位，直至 25 行恢复 |
| V5 | Dock 双击/三态（R6-?-02） | 双击 Dock 图标首窗立即正常显示；（等 >500ms）单击已聚焦窗口仍最小化、再击恢复（三态回归） |
| V6 | 标准位置实时性（R6-?-01 数据层） | WSL 建 /root/Documents、/root/Downloads → 重开 FM → 侧边栏「位置」正确出现两项；删除即回落 |
| V7 | 实例锁（R6-?-01 加固） | 新 quireld 运行中再次启动同配置实例 → 第二实例 error「另一 quireld 实例正在运行（PID …）」并非零退出；不同端口实例不受影响（单测已覆盖，实机验主路径） |
| V8 | P2-05 主题切换浅蓝底 | 视时间窗：连续帧捕获探针按 window-flash-rules §7，命中即记录（允许遗留） |

## 判据之外

- 全程 console 噪音基线（历轮既有 404 资源噪音忽略）。
- 每项证据存 `docs/testing/evidence-2026-11-15/`（QA 截图/console 导出）。
- 新发现按第九轮惯例记录（R10-NN 编号），P3 以下可当轮顺手修。
- 完成后产出 `client-test-report-round10-2026-11-15.md`。
