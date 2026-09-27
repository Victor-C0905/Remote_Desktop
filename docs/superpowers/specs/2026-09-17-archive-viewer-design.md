# ArchiveViewer 压缩包内容浏览 — 设计文档

日期：2026-09-17
状态：已批准（用户确认独立窗口 + 提取并打开单文件）

## 1. 背景与目标

当前双击压缩包直接弹三选项解压对话框，用户无法在解压前查看内容。新增 ArchiveViewer 独立窗口应用：先浏览压缩包内容（目录树 + 大小 + 类型），支持双击包内文件提取并打开、按钮触发全部解压。

**目标**：
- 双击压缩包 → 打开内容浏览窗口（不再直接弹解压对话框）
- 双击包内文件 → 提取到服务器临时目录 → 按现有分类系统路由打开
- 工具栏「全部解压」→ 复用现有三选项解压对话框
- 零新协议：全程复用 ExecuteCommand + file_info

**非目标（YAGNI，后续迭代）**：
- 多选批量解压
- 在压缩包内直接编辑回写
- 音视频流式提取

## 2. 数据流

```
双击压缩包 → FileManager handleOpen 'archive' 分支改造
  → 打开 ArchiveViewer 窗口（preloadData: { serverId, path }）
  → ExecuteCommand 执行列表命令（按格式分发）
  → 解析输出为 Entry[] → 构建目录树 → 渲染
双击包内文件
  → ExecuteCommand 提取单文件到 /tmp/quireld-av/<hash>/
  → detectFileFormat(提取文件) → decideOpenTarget → 创建对应应用窗口
「全部解压」按钮 → 现有解压对话框（三选项：当前目录/新建子目录/自定义）
```

## 3. 命令构造（commands.ts，白名单，纯函数）

| 格式 | 列表命令 | 提取单文件命令 |
|------|---------|---------------|
| .zip | `unzip -l <archive>` | `unzip <archive> <entry> -d <tmpdir>` |
| .tar/.tar.gz/.tgz/.tar.bz2/.tar.xz | `tar -tvf <archive>` | `tar -xf <archive> -C <tmpdir> <entry>` |
| .7z | `7z l -slt <archive>` | `7z x <archive> -o<tmpdir> <entry> -y` |
| .rar | `unrar l <archive>` | `unrar x <archive> <entry> <tmpdir>/` |

- 提取目标目录：`/tmp/quireld-av/<archive名>-<8位hash>/`（前端用 path+serverId 哈希，首次打开时 mkdir）
- 提取目录的容器命令先 `mkdir -p`（对齐 buildExtractCommand 的先置目录模式）
- 列表命令 timeout_secs = 30（大 tar.gz 顺序解压兜底）

## 4. 列表输出解析（parsers.ts，纯函数）

四种格式各一个解析器，输出统一类型：

```ts
interface ArchiveEntry {
  path: string;    // 包内路径（去除前导 ./）
  size: number;    // 字节；解析失败时为 -1（显示 "-"）
  isDir: boolean;  // 尾斜杠/tar 权限位 d/unzip 行首 d
}
```

- `parseUnzipListing`：表格式输出，取 Length/Name 两列，目录行（Name 以 / 结尾）size 0
- `parseTarListing`：GNU tar -tvf（`权限 owner/group size date time path`），路径以 / 结尾或权限首字符 d → isDir；busybox 变体 size 缺失时 -1
- `parse7zListing`：-slt 机器可读（Path/Size/Attributes/IsDirectory key-value）
- `parseUnrarListing`：表格式，取 Size/Name 列，d 标志判目录
- 解析失败（输出格式不认识）→ 抛错让 UI 显示「无法解析列表输出」而非静默空列表

## 5. 树构建（tree.ts，纯函数）

- 平铺 Entry[] → 嵌套 TreeNode[]（children + 累计目录大小）
- 目录即使无显式条目也从子路径推导生成（zip 常见：只有文件路径，无目录行）
- 路径分段用 `/`，忽略 `./` 前缀

## 6. UI（ArchiveViewer.tsx + css）

- 单栏树视图（VSCode 风格）：目录可折叠，图标区分目录/文件（复用现有图标体系）
- 工具栏：压缩包名 + 条目数 + 总大小 + 「全部解压」+ 「刷新」
- 加载态：列表命令执行期间显示 spinner（大 tar.gz 可能数秒）
- 双击文件：提取 → 分类路由；提取期间该行 loading；失败 toast
- 空解析结果/命令失败：内联错误视图 + 重试按钮
- 窗口注册对齐 PDFViewer/HexViewer 模式（preloadData 携带 serverId/path）

## 7. 文件分类系统接入

- `handleOpen` 的 'archive' 分支：由「解压对话框」改为「打开 ArchiveViewer」
- 解压入口迁移：ArchiveViewer 工具栏按钮触发原对话框逻辑（buildExtractCommand 不动）
- 提取文件的打开路由完全复用 FileOpener.decideOpenTarget（含大小确认、hex 兜底）

## 8. 组件与测试

| 文件 | 职责 |
|------|------|
| src/apps/ArchiveViewer/commands.ts | 命令构造（纯函数） |
| src/apps/ArchiveViewer/parsers.ts | 四格式解析（纯函数） |
| src/apps/ArchiveViewer/tree.ts | 树构建（纯函数） |
| src/apps/ArchiveViewer/ArchiveViewer.tsx/.css | UI + 窗口注册 |
| src/apps/FileManager.tsx | 'archive' 分支改造 |
| src/apps/__tests__/archive-parsers.test.ts | 四解析器 fixture 测试（真实输出样例） |
| src/apps/__tests__/archive-commands.test.ts | 命令构造 + 树构建测试 |

## 9. 已知边界（第一版接受）

- 大 `.tar.gz` 列表需顺序解压整个流（gzip 不可随机访问）→ timeout + 加载态
- Zip Slip：依赖白名单工具自身防护（GNU tar 拒绝 `..`、unzip 提示跳过），提取目标限定 /tmp 专用子目录，影响面可控
- 提取打开的是临时副本，编辑保存需「另存为」（TextEditor 已支持）
- 非 UTF-8 locale 下 unzip 中文文件名可能乱码（显示层问题，不影响操作）
- 提取临时目录不主动清理（/tmp 重启自清；避免误删用户编辑中的副本）

## 10. 错误处理

- 列表命令失败（工具未安装等）：错误视图显示 stdout/stderr 摘要
- 提取失败：toast 错误，树保持可用
- 解析器输出为空/格式异常：明确错误而非空树
- 压缩包被删除/移动：刷新时报错（复用现有文件操作错误路径）
