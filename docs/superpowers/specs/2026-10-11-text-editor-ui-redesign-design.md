# 文件编辑器 UI 重构设计（CodeMirror 6 内核）

日期：2026-10-11
状态：已确认（用户选定 CodeMirror 6 方案）

## 背景与问题

TextEditor（`src/apps/TextEditor/`）当前使用原生 `<textarea>` 作为编辑区，
与项目内其他应用（FileManager 的侧边栏 + 分组工具栏 + 高密度状态栏）相比
是全项目最简陋的窗口，与大型项目编辑器（VS Code / Zed / GNOME Text Editor）
的布局差距集中在：

| 维度 | 现状 | 大型项目标准 |
|---|---|---|
| 行号 gutter | 无 | 必备 |
| 语法高亮 | 无（状态栏检测了语言却未用于高亮） | 按文件类型着色 |
| 查找/替换 | 无 | Ctrl+F 面板 |
| 当前行高亮/括号匹配 | 无 | 默认提供 |
| 错误呈现 | 整个内容区被替换成错误卡片 | 内联横幅，内容保持可见 |
| 加载态 | 一行文字 | 居中 spinner + 文件名 |
| 二进制文件 | 提示"请手动切换十六进制模式" | 打开时自动切换 |

另有一个真 bug：`TextEditor.tsx` 存在两个重复的预加载 `useEffect`
（第 122 行的有 `lastOpenedPath` 去重保护，第 211 行的没有），
导致文件被重复 `openFile` 两次。

## 目标布局

```
┌──────────────────────────────────────────────────────────┐
│ Toolbar 48px（保留现有 Adwaita 风格与分组）                 │
│  [📄 文件名 ● / 服务器:路径]    [保存][另存为] │ [文本|十六进制] │
├──────────────────────────────────────────────────────────┤
│ 内联错误横幅（仅出错时出现，--quirel-danger-* 令牌）         │
├──────────────────────────────────────────────────────────┤
│ CodeMirror 6 编辑区                                       │
│  行号 gutter │ 语法高亮 │ 当前行高亮 │ 括号匹配              │
│  Ctrl+F 查找替换面板 · Ctrl+G 跳转行 · 自动换行可切换         │
├──────────────────────────────────────────────────────────┤
│ StatusBar 30px（结构保留）                                 │
│  [行,列│行数│字符数] [语言│编码│EOL│换行开关│保存态│时间]      │
└──────────────────────────────────────────────────────────┘
```

## 方案（已确认）

引入 CodeMirror 6 作为编辑内核。Tauri 桌面应用资源本地加载，体积不敏感；
语言包通过 `@codemirror/language-data` 按需动态加载（打开 .py 才加载 Python 语法）。

### 依赖

- `codemirror`（basicSetup 元包：行号、活动行高亮、括号匹配、历史、搜索快捷键）
- `@codemirror/language-data`（语言描述注册表 + 动态加载）
- 直接 import 所需 API：`@codemirror/state`、`@codemirror/view`、
  `@codemirror/language`、`@codemirror/commands`、`@codemirror/search`

### 组件改动

1. **TextEditorPane 重写**（`components/TextEditorPane.tsx`）
   - `EditorView` + `basicSetup` 替换 `<textarea>`；
   - `LanguageDescription.of` + `languages`（language-data）按扩展名匹配，
     `LanguageDescription.load` 异步加载后 `Compartment` 切换；
   - `EditorView.theme` 把 `--quirel-*`、`--font-mono` CSS 变量映射进
     CM（背景、gutter、活动行、选区、搜索面板），亮暗主题自动跟随变量；
   - `EditorView.lineWrapping` 通过 Compartment 开关，默认关（对齐 VS Code）；
   - `updateListener`：docChanged → onChange；selectionChanged → onCursorChange；
   - 外部 content 变化（文件加载完成）且与内部 doc 不一致时 dispatch 替换 doc，
     保持光标尽量稳定。
   - 光标/选区统计（选中字符数）同步给 StatusBar。

2. **TextEditor.tsx**
   - 删除重复的第二个预加载 `useEffect`（bug 修复）；
   - 错误改为内联横幅（编辑区上方），不再替换内容区；
   - 加载态：居中 spinner（复用 FileManager `.spinner`）+ 文件名；
   - 打开 `fileType === 'binary'` 的文件时自动切到 hex 模式；
   - 工具栏文件图标按扩展名映射（与 FileManager 的 FILE_ICONS 同语言）；
   - "自动换行"开关状态提升至此，透传给 Pane 与 StatusBar。

3. **StatusBar**
   - 行数/字符数改由 CM 文档驱动（props 传入）；
   - 语言检测复用现有 `detectLanguage`（保留）；
   - 右组新增"自动换行"切换项（点击切换，等宽 UI 字体）。

4. **HexEditorPane / SaveDialog / EditorMode**：本次不动。

### CSS

- 新增 `.te-cm-theme` 相关适配（gutter 背景/边框、活动行、选中、搜索面板
  全部走 `--quirel-*` 变量，禁止硬编码色值，徽标语义色除外）；
- 新增内联错误横幅（`--quirel-danger-*`）与加载态样式；
- 清理未使用的 `.te-empty*` 死样式。

### 测试

- `src/test/setup.ts` 增加 `ResizeObserver` mock（jsdom 缺失，CM 渲染必需）；
- 新增纯函数单测：扩展名 → 语言映射（新建 `utils/language.ts`，
  从 StatusBar 的 detectLanguage 逻辑提取共用）；
- 回归：`npm run test:run` 全量通过；`tsc && vite build` 通过。

## 不做（YAGNI）

- 窗口内多标签页（需改 window-system 实例模型 → 独立迭代）；
- minimap、代码折叠导航、自动补全扩展、字号缩放；
- Monaco / 语言服务器（LSP）。
