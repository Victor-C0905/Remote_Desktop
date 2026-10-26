# PROGRESS — 项目进度与交接记录

> 面向 AI/Agent 工具的活文档：每次重要迭代后更新，新条目加在最前。
> 规则见根目录 `AGENTS.md`；图标体系设计详见 `docs/icon-system-proposals.md`。
> 最后更新：2026-10-26（会话：图标体系双档落地 + 0.3.0-beta.2 发版 + CSP 根因修复）

---

## 当前状态速览（2026-10-26）

| 项 | 状态 |
|---|---|
| 最新发布版 | **v0.3.0-beta.2**（tag 在 main `514c2f9`），安装包 `src-tauri/target/release/bundle/nsis/Quirel_0.3.0-beta.2_x64-setup.exe` |
| main | `514c2f9` chore(release): 0.3.0-beta.2 —— 含 0.3.0-beta.1 之后全部 67 项迭代 |
| dev | `8f45ae0` —— 领先 main 1 个提交（终端错误提示 CSP 修复，未进包） |
| push | **未推送**（main/dev/tag 均只在本地，由用户执行） |
| 待归档分支 | feature/new_ui、fix/tauri-plugin-version-alignment、fix/build-info-tags、fix/cli-version-flag、fix/gradient-stop-csp、fix/terminal-init-failed-style（均已 squash 合入，待用户改名 archive/） |
| 测试 | `vitest run` 383 用例全过；`tsc --noEmit` 干净 |

## 本轮（0.3.0-beta.1 → beta.2）完成内容

### 功能与修复（全部已在 beta.2 包内）

1. **桌面 UI 体系翻新**（65 项，squash 提交 `fd2bcd2`）：设计令牌/排版/CJK 字体栈、壳层与窗口自适应、
   resize 闪屏 workaround、Tauri 2.12.5、内置四主题壁纸、凭据 keyring 信封加密体系、
   apply_diff 双端协同修复、本地数据清除体系。详单见 `git log v0.3.0-beta.1..v0.3.0-beta.2`。
2. **图标三层体系双档落地**（方案 C+B，见 `docs/icon-system-proposals.md`）：
   - `AppIcon`（24 视图内容图标）：按 `settingsStore.iconTheme` 双档构建（`makeIcons` 缓存，
     组件新增可选 `iconTheme` 覆盖参数）。classic = 渐变磁贴+白图形+双高光弧；
     paper「纸墨」= 平涂淡彩磁贴+本色实边+深墨图形，文件类型为着色纸形（无磁贴），
     文件夹为双色调全出血轮廓。
   - `AppBadge`（48 视图桌面徽章）：classic 底色从 10 个硬编码 hex 改为 `--ic-*` 令牌
     （双高光弧光影层保留，是 classic 档签名细节，不得移除）。
   - `SymbolicIcon`（16 视图单色）：`bell-outline` 改真描边形态（TopBar 无未读态区分）。
   - 色相再平衡去蓝过载：music/file-audio→紫、videos/file-video→橙、documents→灰。
   - `FILE_ICONS` 补全约 20 个扩展名映射（.pdf/文档/压缩/音视频/镜像等）。
3. **关键 bug 修复**（每个都有浏览器/构建级实证，勿回退）：
   - `icg-folder`/`icg-paper` 渐变死引用（`AppIcon.tsx` DEFS 曾未定义 → fill:none，
     文件夹只剩页签、纸身透明，此前三次"隐形"修复无效的根因）。
   - `hex` 图标路径坐标损坏重绘；`variables.css` paper 组重复 folder 行清理。
   - **CSP 拦截渐变发黑**（beta.2 首发实锤）：Tauri release 向 `style-src` 追加注入样式
     nonce/hash → 按 CSP 规范 `'unsafe-inline'` 失效 → innerHTML 动态创建的
     `style="stop-color:var(…)"` 被拦 → stop-color 回落黑色。修复：`G()` 改用
     `stop-color="var(…)"` presentation attribute（不受 style-src 管）。已用模拟 Tauri
     CSP 的对照实验验证（style 写法 rgb(0,0,0)，属性写法正常琥珀）。
4. **开发体验**：`build.rs`（客户端+quireld）describe 加 `--tags`（轻量 tag 参与基点匹配）；
   客户端支持 `--version`/`-V`（AttachConsole 后输出版本与构建信息，实测
   `quirel 0.3.0-beta.2 (main@v0.3.0-beta.2)`）。
5. **发版过程波折**（已解决）：首发构建被 Tauri 插件版本对齐检查拦下（npm 侧
   plugin-fs/opener 落后 Rust 侧两个 minor）→ `fix/tauri-plugin-version-alignment`
   钉死 npm 精确版本 2.7.1/2.7.2。beta.2 tag 曾因该失败本地撤回重取一次
   （tag 从未推送，不复用已发布版本号的规则不受影响）。

### 已合入 dev、尚未进包（下次发版自动带入）

- `8f45ae0` 终端初始化失败提示 style→`.terminal-init-failed` 样式类（同类 CSP 问题，
  beta.2 包内该提示仍会退化无样式，罕见错误路径仅外观）。

## 遗留事项 / 待办

- [ ] **用户**：push（`git push origin main dev v0.3.0-beta.2`，gitee 远端按需）+ 6 个分支改名归档。
- [ ] **用户**：安装新 beta.2 包，验收设置 → 图标主题 classic 卡观感（琥珀文件夹/彩色磁贴/白纸身）。
- [ ] 色相再平衡第二阶段（database→teal 新色相、search 归属等，见 proposals 文档 §3）。
- [ ] `AppIcon` DEFS 提为全局单例（当前每个实例内嵌 20 个渐变 defs，N 图标 = 20N 节点）。
- [ ] 新增 spreadsheet 图标（.xlsx/.csv 当前暂映射 file-text）。
- [ ] `tauri.conf.json` 的 bundle identifier `com.quirel.app` 以 `.app` 结尾，构建有警告（macOS 语义冲突）。
- [ ] Rust 侧 9 个编译警告（browser_window.rs 死代码、fullscreen.rs 函数指针 casts 等），可顺手清理。

## Agent 必读的技术坑（本会话实证）

1. **CSP 与动态内联样式**：release 经 `tauri://` 伺服且 CSP 被 Tauri 改写（注入 nonce/hash），
   一切运行时 `innerHTML` 里的 `style="…"` 都会被拦（拦后回落 CSS 初始值，如 stop-color=黑）。
   SVG 一律用 presentation attribute（`fill=`/`stroke=`/`stop-color="var(--x)"`）；
   React 的 style prop 走 CSSOM 不受影响。dev（vite，无 CSP 头）不复现——**不要以 dev 观感替代安装包验收**。
2. **SVG 属性不支持 `color-mix()`**（解析失败按黑处理）；需要混色时在 JS 里算好 hex
   （参考可视化稿的做法）或走外部样式表。
3. **轻量 tag 与 describe**：release 脚本打的是轻量 tag，`git describe` 必须带 `--tags`
   （两个 build.rs 已加）；tag 只打在 main。
4. **url(#id) 引用必须有定义**：SVG 渐变引用不存在 id 时按 fill:none 渲染（透明非黑）；
   改 DEFS 色相数组时保持"引用与定义成对"。
5. **版本号**：唯一入口 `npm run release <版本号>`（仅 main，工作区须干净）；代码内一律
   `env!("CARGO_PKG_VERSION")` / 前端 `getVersion()`；dev 构建靠 QUIREL_BUILD_INFO
   （分支@describe）区分，不动版本号。
6. **分支纪律**：AI 产出一律走 `feature/|fix/` 分支 → squash 进 dev（信息须规范重写）→
   发版时 dev 合 main；push/归档改名由用户执行。

## 验证基线（改动图标/前端后建议过一遍）

```bash
npx tsc --noEmit && npx vitest run        # 类型 + 383 用例
npm run build                              # 前端产物
grep -c 'style="stop-color' dist/assets/*.js   # 应为 0（CSP 回归哨兵）
npm run tauri build                        # 安装包（仅发版时）
```
