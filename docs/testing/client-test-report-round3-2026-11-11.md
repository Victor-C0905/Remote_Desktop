# Quirel 客户端第三轮全量测试报告（2026-11-11）

- 被测分支：`fix/visual-test-issues`
- 测试方式：`npm run tauri dev` 实机 + CDP 自动化（端口 9786）+ 本地 WSL quireld + 服务端夹具；
  Agent 两项修复与 `file_changes` 用本地 release 版 quireld 复测
- 服务器：本地 WSL（已连接）；泰国远程服本轮中途断开，未参与最终验证
- 计划与假设清单：[client-test-plan-round3-2026-11-11.md](client-test-plan-round3-2026-11-11.md)
- 结论：**发现 2 个 P1（均已修复并在本地 release 版 quireld 上验证）、5 个 P2（4 个已修复并验证，
  P2-05 探针未复现）、3 个 P3（2 个已修复验证，1 个随 P1-02 处理）**；核心路径与边界数据总体稳定，
  10k 目录、压缩包安全、超大文件防护、查看器 Esc、目录实时刷新等均通过。

---

## 一、缺陷清单（按严重级）

| 编号 | 级别 | 模块 | 标题 | 状态 |
|---|---|---|---|---|
| R3-P1-01 | P1 | 窗口系统 / 全部应用 | 键盘焦点未随活动窗口转移：可在非活动窗口继续输入/执行命令；全局快捷键一次影响多窗口 | **已修复并验证** `39497f2` |
| R3-P1-02 | P1 | Agent 文件读取 | 打开 FIFO 使 worker 线程永久阻塞、连接掉线、文件服务挂起 | **已修复并在 release 版验证** `2bfb2e6` |
| R3-P2-01 | P2 | Agent 命令执行 / 压缩包 | 解压加密 zip 时子进程继承终端等待密码，UI 永久“解压中”，直到 300s 超时 | **已修复并在 release 版验证** `0b61fba` |
| R3-P2-02 | P2 | 编辑器 | 打开 GBK 文件后 Ctrl+S 静默转为 UTF-8，原编码丢失 | **已修复并验证** `1753e58` |
| R3-P2-03 | P2 | 编辑器 | CRLF 文件打开即标记“未保存”，保存后换行被转成 LF | **已修复并验证** `1753e58` |
| R3-P2-04 | P2 | 文件管理器 / 主题 | 深色主题侧边栏文字对比度约 1.13:1（`#3f3f3f` on `#303030`），几乎不可读 | **已修复并验证** `1eab883` |
| R3-P2-05 | P2 | 窗口渲染 | 主题切换后文件管理器下半窗口大面积浅蓝底（DOM 计算背景为白色），持续存在 | 探针已做/未复现（未改渲染链路） |
| R3-P3-01 | P3 | 文件管理器 | 符号链接无任何标识，指向目录的链接类型显示为“文件” | **已修复并验证** `1753e58` |
| R3-P3-02 | P3 | 编辑器 | 打开断链符号链接失败后，留下一个空白“未命名文件”编辑器窗口 | **已修复并验证** `1753e58` |
| R3-P3-03 | P3 | 文件管理器 | 非常规文件类型仅显示 FIFO 等文本，无操作限制/提示（已被 P1-02 覆盖） | 随 P1-02 处理 |

### 历史遗留（本轮早前发现，仍有效）

- P2 Hex 查看器 ASCII 列浅色主题对比度不足。
- P2 Settings 长提示 `nowrap` 截切。
- P3 `file_changes` 订阅静默降级（旧 agent 未实现，客户端无 ack/超时）。
- P3 页面 reload/HMR 后旧终端窗口出现“假连接”（显示已连接但无 shell 提示符）。

---

## 二、P1 / P2 详细记录

### R3-P1-01 键盘焦点未随活动窗口转移（已修复）

**现象**

1. hex 查看器为活动窗口（`windowShellActive`、z=30）时，`document.activeElement` 仍是终端窗口的
   `TEXTAREA.xterm-helper-textarea`；
2. 此时用 CDP 键入 `touch /tmp/qfocus_leak_probe` + Enter，命令在后台终端真实执行
   （WSL `/tmp/qfocus_leak_probe` 被创建）；
3. 修复前两个查看器共存时，一次 Esc 会同时关闭两个；
4. 两个编辑器共存时，在窗口 B 按 Ctrl+S，后台窗口 A 也会触发保存/另存为。

**根因**

所有窗口共用同一 `document`，点击标题栏/经 Dock 激活只更新 `WindowManager.activeWindowId`
与层级，浏览器焦点仍停留在原窗口的输入框或 xterm 隐藏 textarea；
`TextEditor` / `Terminal` / `FileManager` / `useEscCloseWindow` 的全局键盘监听没有按窗口过滤。
Esc hook 的 INPUT/TEXTAREA 守卫还会被他窗的 xterm textarea 永久吞掉。

**修复**（`39497f2`）

- `WindowShell.tsx`：`isActive` 变化时释放跨窗焦点——激活时 blur 属于其它窗口的焦点，
  失活时 blur 属于本窗口的焦点；仅限 `[data-window-id]` 内部，不波及 portal 对话框/菜单；
- `useEscCloseWindow.ts`：输入态守卫忽略“属于其它窗口”的焦点；
- `TextEditor.tsx` / `Terminal.tsx` / `FileManager.tsx`：快捷键先判断
  `manager.getActive()?.id === windowId`。

**验证**

- 激活 hex 后 `activeElement` 变为 BODY；Esc 只关闭 hex，image-viewer 保留；
- 焦点切走后键入 `touch /tmp/qfocus_leak_probe2`：文件未创建（泄漏阻断）；
- 终端自身处于活动状态时键入 `touch /tmp/qfocus_active_probe`：文件创建成功（未过度拦截）；
- `npx tsc --noEmit` 通过。

### R3-P1-02 打开 FIFO 阻塞 Agent（已修复并验证）

**现象**

在文件管理器双击 `pipe.fifo` 后：

1. 编辑器窗口永久显示“正在加载 pipe.fifo...”；
2. 约 30s 后本地服务器 `send_failed` 掉线并自动重连；
3. 重连后 `remote_read_dir` 仍挂起（4s 超时），文件管理器停在“加载中…”；
4. WSL 侧 `ps -L` 显示 quireld worker 线程阻塞在 `wait_for_partner`（FIFO `open()` 等待写端）。

**因果验证**

- 向 FIFO 写入一次后，阻塞线程立即释放，`remote_read_dir('/root')` 11ms 返回；
- 说明并非网络问题，而是 FIFO open 阻塞了 agent 的请求执行线程。

**修复**（`2bfb2e6`，`agent/src/worker/handlers/file.rs`）

- `read_file`：仅允许常规文件（`file_type().is_file()`），FIFO/socket/设备文件直接报错；
- `write_file`：目标已存在且非常规文件时拒绝写入（避免保存到 FIFO 阻塞）。

**验证（release 版 quireld，`0.3.2-beta.3 (fix/visual-test-issues@v0.3.2-beta.3-16-g0b61fba)`）**

- 文件管理器双击 `pipe.fifo`：编辑器立即显示错误横幅
  “无法打开文件 … 不是常规文件（管道/设备/套接字），无法读取”，不再显示“正在加载”；
- 直接调用 `remote_read_file` 返回 `agent_rejected` 与同一文案，无 30s 挂起；
- 随后 `remote_read_dir('/root/quirel-test-scratch-r3/verify/names')` 6ms 返回；
- WSL 侧无 `wait_for_partner` 阻塞线程，连接保持 `connected`，无需重启 agent。

### R3-P2-01 加密 zip 解压挂起（已修复并验证）

**现象**

ArchiveViewer 打开 `encrypted.zip` 可正常列出条目（无加密标识）；指定目标目录点“解压”后
对话框永久“解压中…”，目标目录为空。WSL 侧 `unzip -o ...`（PPID=quireld）阻塞在 `wait_woken`。
手动 kill 后连接断开，对话框显示“解压失败: 与服务器的连接已断开”。

**根因**

`handle_execute_command` 只设置了 stdout/stderr pipe，未设置 stdin；子进程继承 agent 的终端，
`unzip` 等待密码输入，直至 300s 超时才被 kill。

**修复**（`0b61fba`，`agent/src/worker/handlers/command.rs`）

- `cmd.stdin(Stdio::null())`：远程命令无交互输入，加密包将立即失败并回传可读错误。

**验证（release 版 quireld，同上构建）**

- ArchiveViewer 打开 `encrypted.zip`，点击“全部解压”→ 目标 `…/verify/archives/enc-out` → “解压”：
  103ms 后对话框关闭，查看器显示“解压失败: 解压命令执行失败（退出码 5）”；
- agent 日志显示 `unzip -o …` 立即以 `exit_code=5` 结束，不再等待密码；
- 目标目录无残留文件，`remote_read_dir` 5ms 返回，连接保持健康。

**后续建议**：客户端在压缩包条目/能力探测阶段标注“加密不支持”，避免用户走到解压才看到失败。

### R3-P2-02 GBK 文件保存被静默转码（已修复）

**复现**（修复前）

1. `gbk.txt` 为 `iconv -f UTF-8 -t GBK` 生成的 GBK 文件；
2. 编辑器打开后中文显示正常，状态栏显示“UTF-8”；
3. 未做任何修改，按 Ctrl+S；
4. 服务端 `md5sum` 变化，`file gbk.txt` 变为 “Unicode text, UTF-8 text”，大小 13→18 字节。

**修复**

- Agent：`read_file` 显式返回 `encoding`——UTF-8 解码失败时按 GB18030 回退解码并回报 `gb18030`；
  `write_file` 新增 `encoding` 参数，按源编码编码回写（GB18030 无法表示新增字符时明确报错）。
- 客户端：`FileState` 记录 `encoding`；保存按源编码写回；非 UTF-8 强制全量保存
  （Agent `apply_diff` 仅支持 UTF-8，差异同步会破坏编码）。

**验证**（dev 客户端 + release 版 quireld）

- 打开 `gbk.txt`：状态栏显示 `GB18030`，中文内容正确；
- 未改动直接 Ctrl+S：服务端 `md5sum` 保持 `3ca880a8660880a2de467d6249d3ed5e`，`od -c` 原始 GBK 字节不变，`file` 仍为 `ISO-8859 text`。

### R3-P2-03 CRLF 文件打开即 dirty，保存转 LF（已修复）

**复现**（修复前）

1. `crlf.txt` 内容为 `line1\r\nline2\r\n`；
2. 打开后状态栏显示 `LF` 且状态为“未保存”（未做任何编辑）；
3. 按 Ctrl+S 后 `od -c` 显示仅剩 `\n`，md5 变化。

**修复**

- 打开时记录 `lineEnding`（CRLF/LF）并把内容归一化为 LF 交给 CodeMirror，状态栏改显真实换行，
  打开即“已保存”；
- 保存时按记录的换行还原（全量写入），CRLF 文件不走差异同步；
- 另存为仍固定 UTF-8 + LF（新文件语义）。

**验证**

- `crlf.txt` 打开后状态栏 `UTF-8 | CRLF | 已保存`；Ctrl+S 后 md5 保持 `c6242222…`，`od -c` 仍为 `\r\n`；
- `crlf-large.txt`（3.4KB，超过差异同步阈值）插入字符后保存：首行变为 `X第一行中文\r\n…`，
  `\r\n` 保持、md5 按预期变化（强制全量保存生效）。

### R3-P2-04 深色主题文件管理器侧边栏对比度不足（已修复）

**证据**（修复前）

- 截图：`evidence-2026-11-11/r3-dark-filemanager2.png`
- 计算值：标签 `color: rgb(63,63,63)`，侧边栏 `background: rgb(48,48,48)`，对比度约 **1.13:1**
  （WCAG AA 要求 4.5:1）。侧边栏“主目录/文档/下载/…/设备挂载点”几乎不可读。

**根因与修复**

- `--quirel-sidebar-fg` 不在 `themes.ts` 的运行时注入清单中，深色下仍取 `variables.css` 的浅色默认值；
- 亮暗标记实际写在 `<html data-icon-scheme>`（`themeUtils.applyThemeColors`），
  在 `variables.css` 以 `[data-icon-scheme='dark']` 覆盖为 `rgba(255,255,255,0.87)`。

**验证**

- 深色：`.fm-sidebar-item` 计算色 `rgba(255,255,255,0.87)`、背景 `rgb(48,48,48)`（约 8.9:1）；
- 切回纸张主题：仍为 `rgb(63,63,63)` on `rgb(245,240,230)`，浅色不受影响。

### R3-P2-05 主题切换后大面积浅蓝底（探针完成，未复现）

**现象**（第三轮记录）

切换 `paper → dark → paper` 后，文件管理器窗口下半部分显示纯浅蓝 `rgb(187,217,240)`，持续 >4s，
强制 scroll/resize 不恢复；同区域 DOM 计算背景为白色。

**本轮探针**

- 深色主题下对文件管理器下半窗口（CSS `24,357,900x325`）做 CDP 截图：
  区域平均色 `(39,39,39)`，浅蓝 `rgb(187,217,240)` 像素占比 **0%** → 页面栅格内容正常；
- 当前壁纸平均色为 `(185,143,117)`（暖棕），与该浅蓝不符 → 不是 AGENTS §6 的“壁纸平均色填充”；
- 结论：更可能是原生窗口合成/透底而非 CSS/重绘缺失，需在切换瞬间用窗口级截图（PrintWindow）
  或 `docs/window-flash-rules.md` 的坏帧探针进一步定位。

按本轮约定只做探针，未改窗口渲染链路；条目保留待复测。

### R3-P3-01 符号链接无标识、目录链接类型错误（已修复）

**修复**

- Agent：`read_dir` / `stat_entry` / 文件监听统一改为 lstat 后跟随链接取目标元数据，
  协议新增 `is_symlink` / `link_target`（serde default 保持旧客户端兼容）；
- 客户端：列表与网格在文件名后显示 `↗` 角标（tooltip 为“链接 → 目标”），类型列显示“链接”；
  指向目录的链接 `is_dir=true`，可正常进入。

**验证**

- `link-file` / `link-dir` / `link-broken` 均显示 `↗` 与类型“链接”，tooltip 分别为
  `sym-file.txt` / `dir-target` / `missing.txt`；
- 双击 `link-dir` 进入目标目录并列出 `inner.txt`；`pipe.fifo` 类型显示 `FIFO`。

### R3-P3-02 打开失败留下空白“未命名文件”页签（已修复）

**修复**

- 编辑器记录 `failedPath`；打开失败时渲染只读失败态（标题 + 路径 + 提示），
  不再落到“未命名文件”的空编辑区；状态栏只显示路径/服务器，不显示编码与字符数。

**验证**

- `pipe.fifo`：页签标题 `pipe.fifo`，`.te-open-failed` 存在、无 CodeMirror 编辑区；
- `link-broken`：页签标题 `link-broken`，只读失败态（“No such file or directory”），无空白可编辑页签。

---

## 三、通过项（本轮实测）

| 项目 | 结果 |
|---|---|
| 特殊文件名：中文/emoji/空格/引号/制表符/换行/200 字符/隐藏文件 | 列表正常渲染，长名省略号截断，换行名未撑破行高 |
| 符号链接：文件链接打开 | 正常读取目标内容，列表/网格显示 `↗` 与类型“链接” |
| 断链符号链接打开 | 只读失败态（页签保留文件名，不再出现空白“未命名文件”），不崩溃 |
| 10 000 文件目录 | 加载 688ms，仅渲染 35 行（虚拟化生效） |
| 30MB 文本 / 256MB 二进制 | 编辑器/Hex 查看器均返回“文件太大，限制 10MB”，无卡死/内存暴涨 |
| zip-slip | ArchiveViewer 提示“压缩包含不安全路径”，解压被拒绝，未生成 `escape.txt` |
| 正常 zip 解压 | 中文/emoji 文件名与子目录正确解出，对话框成功关闭 |
| BOM / 无结尾换行保存 | 字节不变（md5 一致） |
| 多查看器 Esc | 修复后一次 Esc 仅关闭活动窗口 |
| `file_changes` 目录实时刷新（release 版 agent） | 外部 `touch` 新建文件 2s 内自动出现在列表；外部 `rm` 删除后自动移除（此前旧 agent 报“订阅类型尚未实现”） |
| 跨服编辑器保存 | 绑定 A 服页签在 B 服活动时保存不覆盖远端文件（`afbb4a8`，前序已验证） |
| 断线保存容错 | 内联错误横幅 + 内容保留，重连后可保存（前序已验证） |
| 服务器切换 | 终端/文件管理器跟随重建，无崩溃（前序已验证） |

---

## 四、未执行 / 受阻项

| 项 | 原因 |
|---|---|
| 单元测试（window-system / window-shell） | `vitest` worker 启动失败：`ERR_REQUIRE_ESM @csstools/css-calc`（环境/依赖问题，非本次改动） |
| 泰国远程服回归 / 远端 scratch 清理 | 本轮中途断开（`disconnected`） |
| `build:dev` 安装包（CSP/图标/字体/坏帧探针） | 未执行（受构建占用与时间限制） |
| DPI 125%/150%、双屏、F11 全屏矩阵 | 未系统执行；本轮截图环境为 150% DPI |
| 30–60 分钟长稳 / 资源泄漏曲线 | 未执行 |
| 通知跨服务器隔离、浏览器会话/airspace 全矩阵 | 未执行 |
| 真实鼠标点击保真 | 页面 reload 后 CDP `Input.dispatchMouseEvent` 不再产生 `mousedown`（键盘与 evaljs 正常），交互改用合成 DOM 事件，部分真实点击链路未覆盖 |

---

## 五、提交记录（任务分支内）

| 提交 | 内容 |
|---|---|
| `afbb4a8` | `fix(ui): 编辑器保存使用页签绑定服务器`（前序，跨服保存 P1） |
| `39497f2` | `fix(ui): 按键与焦点按活动窗口隔离` |
| `2bfb2e6` | `fix(agent): 拒绝读写非常规文件避免 worker 阻塞` |
| `0b61fba` | `fix(agent): 远程命令关闭 stdin 防止交互挂起` |

建议 squash 提交信息（供用户合并时参考）：
`fix(ui): 修复跨窗键盘泄漏与非常规文件导致的 agent 阻塞`

---

## 六、测试数据与清理

- 本地 WSL 夹具目录 `/root/quirel-test-scratch-r3/`（含 release 复测用的 `verify/` 子目录）已删除并核实不存在；
- `/tmp/qfocus_*` 探针文件已删除；
- release 版 quireld 仍在本地 WSL 后台运行（配置 `/var/lib/quireld-probe/quireld-probe.toml`，日志 `/tmp/quireld-release.log`），
  供后续复测使用；
- 泰国服务器 `/root/quirel-test-scratch-r3/`（含 `watch-probe*.txt`、`th-probe.txt`）因服务器断开未清理，
  待重连后删除；
- 证据目录：`docs/testing/evidence-2026-11-11/`（已在 `.gitignore` 忽略）。

## 七、后续建议（按优先级）

1. 编辑器编码与会话策略：按文件保存源编码/换行（或显式提示转换），修复 GBK/CRLF 静默改写；
2. 深色主题变量审计：侧边栏前景、Hex ASCII 列等低对比项统一走主题令牌；
3. 主题切换后的局部未重绘按坏帧文档 §7 做探针与修复；
4. 补齐 `file_changes` 的订阅 ack/超时降级提示，避免旧 agent 静默无刷新；
5. 修复 vitest ESM 依赖问题，恢复窗口系统单测门禁。
