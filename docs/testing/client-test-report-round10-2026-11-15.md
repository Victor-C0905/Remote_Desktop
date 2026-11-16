# Quirel 客户端第十轮测试报告：两批修复实机验证（2026-11-15）

- 被测：`fix/file-manager-sidebar` @ 工作树（含 §六/§七 全部修复 + 本轮新增 R3-P2 修复）
- 前端：vite dev（1420）实时服务；客户端 Rust 零改动 → 复用 debug exe
  （`fix/file-manager-sidebar@v0.3.3-18-g1ebdf55`）
- quireld：`/root/quirel-r4/agent` 源码同步自仓库（rsync）后重构建 release
  （1m25s，含 instance_lock）；配置 `/var/lib/quireld-probe/quireld-r4.toml`（:8443）
- 证据：`docs/testing/evidence-2026-11-15/`（截图）+ `%TEMP%/quirel-qa/r10-v8-frames/`（93 帧连拍，临时目录）
- 计划：[client-test-plan-round10-2026-11-15.md](client-test-plan-round10-2026-11-15.md)

## 一、结果总表

| # | 项 | 结果 | 关键证据 |
|---|---|---|---|
| V1 | R8-N1 终端卸载 TypeError | ✅ **通过** | 终端跑 `sleep 800`（WSL PID 1088）→ JS 点关窗 → console **零 uncaught**；quireld 日志「Session 收到 Close…status=9」；sleep 进程消失 |
| V2 | 启动重连确定性（R8-N2） | ✅ **通过** | 冷启动 ×3 全部确定性「未连接」（启动日志均见「检测到进程重启，已清空上一世代的会话镜像」）；同进程 `location.reload()` → 已连接（纪元一致路径恢复） |
| V3 | Settings 长提示 nowrap 截切（R3-P2） | 🎯 **发现原始元素并修复** | 见 §二——五轮悬案收案 |
| V4 | FM 重连假空态（R9-N2） | ✅ **通过** | kill quireld → 离线占位；重启后 16 个采样点（450ms 间隔）：`offline:false+empty+rows:0` 假空态 **零出现**，离线占位直接转 25 行 |
| V5 | Dock 双击/三态（R6-?-02） | ✅ **通过** | 双击打开首窗 display:flex（宽限期拦截误最小化）；宽限期外单击已聚焦→`display:none` 最小化 ✓→再击恢复 ✓（三态基线不变） |
| V6 | 标准位置实时性（R6-?-01 数据层） | ✅ **通过** | 建 /root/Documents+Downloads → 重开 FM 即现（27 行）；删除 → 回落 25 行 |
| V7 | 实例锁（R6-?-01 加固） | ✅ **通过** | 新 quireld 运行中同配置二启：ERROR「另一 quireld 实例正在运行（PID 1187）…锁文件 …8443.lock」并立即退出；原实例（manager+worker）不受影响；锁文件落盘 |
| V8 | P2-05 主题切换浅蓝底 | ⚪ **未复现（第三次独立证实）** | 14s 全屏连拍 93 帧（含深→浅→深三次切换瞬间）：浅蓝主导像素最高 **0.06%**（噪声级，无大面积异常）。扫描脚本 `.zcode/r10-v8-scan.ps1` 可复用 |

## 二、V3 收案：R3-P2「Settings 长提示 nowrap 截切」定位与修复

**第三轮悬置至今的原始元素终于定位**（历轮报告均记「原始元素无法定位、盲目改 CSS 有回归风险」）：

- **定位方法**：第九轮 §六-3 配方（`scrollWidth > clientWidth+1 && computed nowrap`）在设置 9 节逐节扫描——**唯一命中**：浏览器节 SOCKS5 端口提示 `.st-option-hint`（socks5PortHint 需 751px 被压到 379px）。
- **根因**：该元素 className 挂了全局排版工具类 **`.text-caption`（typography.css:46 自带 `white-space: nowrap`）**，且其注入顺序晚于 Settings.css（同特异性后者胜），长文本被截断在卡片边缘。其余 8 节零命中——`text-caption` 的其他使用处均为短文本/自带 ellipsis，不受影响。
- **修复**：`Settings.tsx` 去掉该元素上的 `text-caption` 工具类；`.st-option-hint` 字号/颜色自持（`--font-small` + `text-secondary`，后者为修复前实际呈现色）+ `white-space: normal`。
- **验证**：修复后该提示换行为两行（clientHeight 36px）；9 节复扫全零命中；前后截图 `r10-v3-before-hint-clipped.png` / `r10-v3-after-hint-wrapped.png`。
- **教训入库**：全局排版工具类（nowrap 族）与长文本场景组合是本仓库的截切风险模式，后续新增长提示一律自持样式或用可换行的工具类。

## 三、修复有效性结论（对应第九轮 §六/§七 的 7 项待验证）

- 6 项代码修复全部实机通过：R8-N1 ✅ / 启动重连确定性 ✅ / FM 假空态 ✅ / Dock 双击 ✅ / 标准位置 ✅ / 实例锁 ✅。
- 第 7 项（Settings 配方）不但验证了方法，还**抓获并修复了原始缺陷**。
- R9-N2 残留观察项维持：重连窗口未见假空态；`setCurrentPath("/")` 的订阅抖动（H2）本轮未触发（localhost 恢复 <450ms，采样未捕捉到中间态），继续留观。
- P2-05 第三次未复现，建议销项或降级为「如再现再立项」（需用户确认）。

## 四、本轮改动

- `fix(ui)`: Settings.tsx / Settings.css（R3-P2 修复，见 §二）
- 测试数据：WSL `/root/Documents`、`/root/Downloads` 建删（已删）；`/tmp/quireld-r10-run*.log`
- 环境收尾：vite、quireld（新二进制，含实例锁）、客户端保持运行（R8-N3：会话间隔自然退场）

## 五、QA 驱动经验（追加至第八轮 §六陷阱清单）

15. **多行 evaljs 偶发静默失败**（无输出无报错）——单行表达式稳定；关键断言拆单行，输出尽量 ASCII 化（GBK 控制台码页会淹没 UTF-8 中文结果）。
16. **qa 元素路径格式为 `a>b>c`（nth-of-type 链）**——自拼 `/a/b/c` 斜杠格式不解析、点击静默无效；一律用 `snapshot` 命令输出的规范路径，或 `click --at x,y`（坐标由单行 evaljs gBCR 现算）。
17. **snapshot 只列 button/[button] 角色**——div onClick（Dock 项等）需自算路径或坐标点击。
18. **设置窗「连接」按钮需先点选 `.st-server-item` 行**才启用（卡片式两段交互）。

## 六、台账终态（全项目测试问题清单收口）

| 问题 | 终态 |
|---|---|
| R8-N1 终端卸载 TypeError | ✅ 已修 + 实机验证 |
| 启动自动重连时有时无（R8-N2/R9-N3） | ✅ 已修（确定性=设计）+ 实机验证 |
| Settings 长提示 nowrap 截切（R3-P2） | ✅ **本轮定位并修复** + 实机验证 |
| FM 重连 ~3s 空窗（R9-N2） | ✅ 已修 + 实机验证（H2 抖动留观） |
| 首窗 display:none（R6-?-02） | ✅ 已修 + 实机验证 |
| agent 标准位置探测陈旧（R6-?-01） | ✅ 实机证伪数据层缓存 + 实例锁加固 + 实机验证 |
| 主题切换浅蓝底（P2-05） | ⚪ 第三次未复现，建议销项（待用户确认） |
| 首窗 display:none 竞态（R6-?-02 同名旧项）/ agent 双 worker | 归并上述 |
| 桌面右键菜单 / 总览窗口管理 | 独立新功能，未立项（用户已确认范围外） |
