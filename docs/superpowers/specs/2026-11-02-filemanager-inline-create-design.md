# 文件管理器：新建文件/文件夹对齐 Windows 内联命名体验 — 设计

- 日期：2026-11-02
- 分支：feature/v0.4
- 状态：已确认（用户拍板：默认文件名 `新建文件`（无扩展名）；重名递增对齐 Windows，从 ` (2)` 起）

## 1. 背景与目标

现状：三处新建入口（工具栏「新建文件夹」`handleMkdir`、空白右键菜单「新建文件夹」
`handleNewFolder` /「新建文件」`handleNewFile`）全部用阻塞式 `prompt()` 先要名字再创建；
无重名预检（重名靠后端报错通知）、无内联命名体验。

目标（对齐 Windows Explorer）：

1. 触发新建 → **立即**在当前目录创建默认名的空文件夹/空文件；
2. 新条目自动进入内联编辑态，光标聚焦、名称预选中（文件只选基名，同 F2 重命名惯例）；
3. 默认名重名时自动递增：`新建文件夹 (2)`、`新建文件夹 (3)`…；
4. Enter / 失焦提交（未改名不发请求）；Esc 取消命名但条目保留（Windows 同款）。

## 2. 方案取舍

- **方案 A（采纳）**：立即创建真实条目 + 复用现有内联重命名基建
  （`editingEntry`/`editingName`/`handleEditKeyDown`，列表/网格两处 input 均已存在）。
  后端 `remote_mkdir`、`remote_write_file(content="")` 均已具备，零协议改动。
- 方案 B（否决）：虚拟条目确认后才创建——与"直接创建"目标相反，渲染特判多。
- 方案 C（否决）：后端加"找空闲名"命令——前端 `entries` 已够用，不值协议变更。

## 3. 行为规格

### 3.1 唯一默认名 `generateUniqueName(base, isDir, entries)`

纯函数，落 `src/apps/fileTable.ts`（与排序/筛选同层，单测覆盖）：

- 候选名与现有条目名精确比较（Linux 大小写敏感）；无冲突 → 原名。
- 冲突 → 目录：`base (N)`；文件：`lastIndexOf('.') > 0` 时拆基名/扩展名 →
  `基名 (N).扩展名`，无扩展名 → `base (N)`。
- N 从 2 起递增到首个空闲值（Windows 规则；不剥离已有 ` (N)` 后缀再叠加）。

### 3.2 新建流程（三入口收敛为 `createEntry(kind: "folder" | "file")`）

1. `setContextMenu(null)`；若处于搜索模式先 `exitSearch()`（搜索结果视图无处落新条目，
   回浏览视图在当前目录创建，两个入口行为一致）。
2. `generateUniqueName(默认名, isDir, entries)`：
   - 文件夹默认名复用 i18n `fileManager.newFolder`（新建文件夹 / New folder）；
   - 文件默认名新 key `fileManager.defaultNewFileName`（新建文件 / New file）。
3. `invoke` 创建：文件夹 `remote_mkdir`；文件 `remote_write_file`（content=""）。
4. `await loadDir(currentPath)`：**成功**（返回 true）才进入编辑态——合成匹配键
   `FileEntry`（name=唯一名、path=undefined；仅作 `name+path` 匹配键，渲染取真实条目）；
   失败（网络断等）只弹错误通知，不进编辑。
5. 编辑态：输入框 `autoFocus` + 名称预选中（见 3.3）。
6. 提交成功后保持新条目选中（`setSelected([新路径])`，浏览模式 await loadDir 后设置，
   规避 loadDir 内部 clearSelection 时序）。

### 3.3 内联编辑初始选中（顺带修复现有 F2 重命名）

- 新增 `inlineEditRef` 挂两处内联 input（列表 `fm-fn-edit-input` / 网格 `fm-gi-edit-input`），
  `useEffect([editingEntry])` 内 setTimeout(0) 后执行选中：文件名含扩展名（`lastDotIndex > 0`）
  → 选 `0..lastDotIndex`，否则全选——与既有弹窗重命名 `renameInputRef` 逻辑同款。
- 现状缺陷一并修复：F2/右键重命名进入内联编辑后目前无任何预选中。

### 3.4 `finishRename` 增强

- **重名预检**：目标名与同父目录其他条目同名（排除自身）→ 错误通知
  （新 key `fileManager.nameExists`）且不退出编辑；浏览模式（`editingEntry.path === undefined`）
  与 `entries` 比，搜索模式与 `searchResults` 同父目录比（统一按 dirname 比较）。
- 提交成功：浏览模式 `await loadDir(parentDir)` 后 `setSelected([newPath])`；
  搜索模式沿用 `refreshAfterMutation()`（搜索重跑，选中时机复杂，不处理）。

### 3.5 边界行为

- Esc：取消编辑，条目保留默认名（Windows 同款，不删除已建条目）。
- 空名 Enter/失焦：取消编辑（现有行为，温和降级；Windows 会弹错，此处保持现状）。
- 创建失败（权限/重名竞态等）：错误通知，不进编辑态。
- 新建只影响当前目录浏览视图；列表按现有排序规则落位（刷新后天然排序正确）。

## 4. 实现要点

| 单元 | 位置 | 说明 |
| --- | --- | --- |
| `generateUniqueName` | `src/apps/fileTable.ts` | 纯函数 + 单测（`fileTable.test.ts`） |
| `createEntry(kind)` | `src/apps/FileManager.tsx` | 三入口收敛；替换 `handleMkdir`/`handleNewFolder`/`handleNewFile` |
| 内联选中 effect | `src/apps/FileManager.tsx` | `inlineEditRef` + `useEffect([editingEntry])` |
| `finishRename` 预检/选中 | `src/apps/FileManager.tsx` | 冲突预检 + 成功后保持选中 |
| i18n | `src/i18n/locales/{zh-CN,en-US}.ts` | +`defaultNewFileName`、`nameExists`；删死键 `promptNewFolder`/`promptFolderName`/`promptFileName` |

约束遵守：后端/协议零改动；不触碰窗口坏帧红线文件（`lib.rs`/`fullscreen.rs`/壁纸/webview bounds）。

## 5. 测试

- `fileTable.test.ts`：`generateUniqueName` — 无冲突 / 目录冲突递增 / 文件含扩展名拆分 /
  连号占用跳号 / base 本身带 ` (2)`。
- `npm run test:run` + `tsc`（`npm run build` 的类型检查部分）全绿。

## 6. 不做（YAGNI）

- Ctrl+Shift+N 快捷键、Esc 删除已建条目、空名弹错框、后端唯一名解析。
