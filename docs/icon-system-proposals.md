# Quirel 图标体系统一化研究与参考方案

> **状态（2026-10-25）**：已采纳 **C + B** 双档落地——classic 档 = 方案 C 修复版、
> paper 档 = 方案 B 纸墨双奏，实现于 feature/new_ui 分支 5 个提交
> （7de8b2f / 41fa09d / 3ab33ce / bddd021 / b81990e）。

> 研究范围：`src/components/AppIcon.tsx`、`AppBadge.tsx`、`symbolic.tsx`、`iconNames.ts`、
> `src/styles/variables.css`（--ic-* 色板）、`src/apps/fileIcons.ts`、`src/config/themes.ts`。
> 结论先行：**四套可选方案（A/B/C/D）+ 一组无论选哪套都必须先做的公共修复**。
> 附带可视化对比稿：四方案 × 亮暗 × 四个使用场景的真实 SVG 渲染（见会话内嵌预览）。

---

## 1. 现状审计

### 1.1 架构（三层 + 变量矩阵）

| 层 | 组件 | 视图 | 尺寸场景 | 语言 |
|---|---|---|---|---|
| 应用徽章 | `AppBadge` | 48 | 桌面 44 / Dock 48 / 概览 34 / 设置预览 30 | classic 彩色徽章+白图形 ／ paper 白卡+本色图形 |
| 内容图标 | `AppIcon` | 24 | 14–48（FM 列表 16、网格 36、菜单 14、属性 28、空状态 40） | 磁贴徽章 `B(c)` + 图形，约 40 个语义 key |
| 系统铬层 | `SymbolicIcon` | 16 | TopBar / 通知中心 / 浏览器导航 / FM 表头 | 单色 currentColor，剪影+线条混排 |

色彩由 `--ic-*` 变量驱动（8 色相 × light/surface/deep/ink/solid + folder + paper 系 + hi），
`data-icon-theme`（classic/paper）× `data-icon-scheme`（light/dark）共 4 组覆盖，随 UI 主题联动。

### 1.2 发现的问题

**🔴 P0 · 渐变死引用（当前构建的实际渲染 bug，浏览器实测确认）**

- `AppIcon.tsx` 的 `DEFS` 只生成 8 个基础色的渐变（`icg-blue`…`icg-gray` 及 `*d` 暗档），
  但代码引用了两个**从未定义**的渐变：
  - `url(#icg-folder)` —— fa0cfc0（文件夹回归经典全彩）引入；
  - `url(#icg-paper)` —— 7a9ce45（磁贴内白纸+投影+实边）引入。
- SVG 规范：引用不存在的 paint server 且无回退色 ⇒ 按 `fill: none` 渲染。
  实测（Chromium）：**文件夹前板完全不可见（只剩页签和描边）、文件纸身透明（磁贴底透出）**。
- 因果链：此后 f54d4d0、303cf2d、377ea83 三次「根治隐形」提交全部只调
  `--ic-folder-*` / 纸系变量——这些变量只作用于页签与描边，**从未触及前板/纸身本体**，
  所以问题反复回潮。修一行 DEFS 即根治。
- 另：`variables.css` paper-light 组 folder 变量重复两行（#208-209，后者覆盖前者，属编辑残留）。

**🟠 P1 · AppBadge classic 硬编码 10 个 hex 底色**（`APP_COLORS.c`）
不随 `--ic-*`、不随 UI 主题、不随亮暗变——paper/neutral/dark 三套 UI 下桌面徽章完全同色，
与 AppIcon 磁贴（随主题换挡）产生同屏色差（如 files 徽章恒 `#3584e4`，暗色下 AppIcon 蓝已提亮为 `#62a0ea`）。

**🟡 P2 · 一致性/覆盖清单**

| # | 问题 | 位置 |
|---|---|---|
| 1 | `hex`（十六进制查看器）图形路径坐标损坏（`M10.2 8.4-9.2 11.2…` x 负值出画布） | AppIcon.tsx:72 |
| 2 | `bell` 与 `bell-outline` 路径完全相同（注释也承认） | symbolic.tsx:103-115 |
| 3 | `FILE_ICONS` 缺 `.pdf`（`pdf` 图标已存在却未映射）、`.xlsx/.csv/.doc`、`.7z/.rar`、`.wav/.flac/.ogg`、`.iso/.img` 等 | fileIcons.ts |
| 4 | 蓝色过载：13 个语义项用 blue（files/browser/notifications/connection/documents/music/videos/network/about/search/database/file-audio/file-video），色相失衡 | AppIcon.tsx |
| 5 | `DEFS` 每个 AppIcon 实例内嵌 16 个相同渐变（N 个图标 = 16N 个节点），且 folder/paper 想用全局渐变却没地方共享 | AppIcon.tsx:21-26 |
| 6 | folder 不响应 `iconTheme` 切换（classic/paper 下同形同色），设置页的图标主题预览卡无法体现差异 | AppIcon.tsx:139 |
| 7 | SymbolicIcon 混排无成文规则：剪影（bell/trash/gear）与描边（refresh/external-link/chevron）混用，线宽 1.5–2.4 浮动 | symbolic.tsx |

---

## 2. 四套参考方案

> 每套均覆盖三层 + 变量矩阵，均先含第 4 节的公共修复。
> 「成本」：S = 半天内；M = 1–2 天；L = 2 天以上（含全量图形重绘）。

### 方案 A · 徽章星球（全磁贴化，统一度最高，成本 M）

**理念**：除 symbolic 外，一切内容图标共用同一容器语法——圆角磁贴（rx 7/24 ≈ 29%）
+ 垂直渐变（light→surface）+ 深色体积描边 + 白色/深色图形。文件夹放弃「全出血轮廓」隐喻，成为琥珀磁贴 + 白色文件夹图形。

- **形状语法**：磁贴 21×21 rx7 @24；图形居中 10–12px 占位；文件类型保留「着色纸形」作磁贴内层。
- **色彩**：完全复用现有 8 色相 + folder 琥珀三阶；纸形 = `color-mix(surface 80%, white)`，暗色不变（白纸语义跨主题恒定）。
- **各层落地**：AppBadge classic 接令牌（同磁贴语法，48 视图等比）；folder/folder-open/folder-plus 改磁贴构图；symbolic 维持现状（P2-7 语法成文）。
- **改动清单**：`AppIcon.tsx`（DEFS 修 + folder 三键重绘 + DEFS 提为全局单例）、`AppBadge.tsx`（去硬编码）、`variables.css`（清理重复行）。
- **保留细节**：AppBadge classic 的光影层**原样保留**——两条白色高光弧（`M4 26 Q24 8 44 26…` opacity .07 与 `M4 14 Q24 4 44 14…` opacity .12，即徽章上「若隐若现的眼睑纹」）、白色 28% 外描边、白色图形；仅底色来源由硬编码 hex 换成 `--ic-*` 令牌。
- **优**：FM 网格/列表里文件夹与文件节奏完全一致；16px 辨识度最好；「一切皆徽章」与 AppBadge 完全同构，心智统一。
- **劣**：放弃你反复调校出的经典文件夹轮廓（与 fa0cfc0 的方向相逆）；文件夹色相被锁死在琥珀磁贴里，与「锁=黄」的区分度下降（需把 lock 调向卡其/铜色）。

### 方案 B · 纸墨双奏（duotone 去渐变化，辨识度最高，成本 M）

**理念**：全站内容图标只有一对关系——**淡彩平涂底（16–30% tint）+ 深墨图形**。
无渐变、无高光、无体积描边。与 paper/neutral 主题的「暖纸白卡」气质同源，是四套中最有辨识度的一套。

- **形状语法**：徽章 = tint 圆角方块 + 本色实边 + 深墨图形；文件夹 = 全出血双色调（深墨页签 + tint 前板 + 深墨 1.4px 勾边）；文件类型 = tint 纸形 + 深墨标记（无磁贴）。
- **色彩**：新增 `--ic-<c>-ink` 全量启用（paper 组已有一半：#1a5fb4/#0f8658/#a34a00…，dark 组换浅亮墨）；tint 全部由 `color-mix(<solid> N%, card-bg)` 派生——**自动跟随三套 UI 主题 × 亮暗，无需手工调四组色板**。
- **各层落地**：AppBadge 两档主题合一（paper 语法即 B，classic 档退役或保留为渐变变体）；symbolic 收敛为纯剪影。
- **改动清单**：`variables.css` 重构（ink 提升为主角色，删 light/deep 渐变档）、`AppIcon.tsx` 全量换渲染配方（图形可 90% 复用，换色即可）、`AppBadge.tsx` 简化。
- **优**：渲染最便宜（零渐变零 defs，DEFS 直接删除）；四组主题自适应最强；纸墨气质与产品名 Quirel（纸张联想）契合；暗色模式天然成立。
- **劣**：饱和度和「 juicy 感」下降，桌面徽章 44px 下比 classic 安静；与 GNOME Circle 彩色传统拉开距离；**有意去掉**双高光弧与渐变（平涂是 B 的核心语言，若想保留眼睑纹可将弧线降档为 4% 一条）。

### 方案 C · 经典双轨·修复版（保守派，成本 S）★ 推荐先落这版

**理念**：保留现状方向（经典全彩文件夹 + 磁贴徽章 + 单色铬层），把 P0/P1/P2 修干净、
把隐性规则写成显性规范。即「你已经在做的事，做到位」。

- **修复后形态**：folder 三层结构不变，前板渐变真正渲染（light→surface + hi 线 + 深色底边）；
  文件纸形恢复白纸 + 同色实边 + 投影；AppBadge classic 底色改读 `--ic-<c>-surface`（暗色自动换挡）。
- **成文语法**（新增 `docs/icon-grammar.md` 或组件头注释）：
  - AppIcon 磁贴：rx7/24，边 1.2 深色 90%，内高光 1px 白 42%；图形线宽 1.9±0.2，圆头圆角；
  - AppBadge classic：平涂徽章底 + **双白色高光弧（.07/.12，即「眼睑纹」）+ 白 28% 外描边**——此光影层为 classic 档的签名细节，任何改动不得移除；
  - SymbolicIcon：物体类=剪影+evenodd 镂空；方向/动作类=描边 1.5±0.2 @16 网格；对勾/箭头可加粗至 2.4；
  - 色相语义表（见 §4 色相再平衡）。
- **优**：风险最小、零学习成本、完全保留既有调校成果；当天可完成。
- **劣**：双层语言（磁贴 vs 全出血 folder）依旧存在，「统一」是规范层面的而非形态层面的；蓝色过载只能靠色相再平衡缓解。

### 方案 D · 墨线工坊（全线稿语言，成本 L）

**理念**：内容图标与系统栏 symbolic 收敛为**同一套线稿语法**——1.6–1.8px（24 网格）/1.5px（16 网格）
圆头线条 + 同色 10–14% 淡底圆角方块。终端、监控等开发工具气质最强，整体接近 Linear/Raycast 的图标观感。

- **形状语法**：所有内容图标 = 淡底磁贴 + 线稿图形（线即形，无填充块面）；文件夹 = 线稿轮廓 + 淡琥珀填充；文件类型 = 线稿纸形 + 线稿标记。
- **色彩**：线色 light 面 = surface 色，dark 面 = 浅亮档（现有 light 档变量直接可用）；淡底 = 10% tint。
- **改动清单**：AppIcon 全量重绘为线稿（40 个图形，工作量主体）；AppBadge 换线稿图形；symbolic 线稿化（部分已是）。
- **优**：真正的「一套语法管三层」；尺寸缩放最稳（线条语义不随大小变质）；最贴 Quirel 的服务器工具定位。
- **劣**：桌面徽章失去彩色徽章的传统锚点，与 GNOME 桌面生态观感偏离最远；全量重绘成本最高；16px 下线稿辨识度依赖线宽纪律；**有意去掉**高光弧与渐变（线稿语言无块面光影）。

### 对比总表

| | A 徽章星球 | B 纸墨双奏 | C 经典双轨·修复 | D 墨线工坊 |
|---|---|---|---|---|
| 统一度 | ★★★★★（形态级） | ★★★★（配方级） | ★★★（规范级） | ★★★★★（语法级） |
| 美观倾向 | 秩序感、桌面正统 | 素雅、纸质、独特 | 现状改良、GNOME 味 | 冷静、工具感 |
| 文件夹形态 | 琥珀磁贴+白图形 | 全出血双色调 | 经典全彩轮廓（现状） | 淡底线稿 |
| 与 paper 主题契合 | 良 | 优 | 良 | 优 |
| 与 dark 主题契合 | 优（变量已备） | 优（tint 自动派生） | 优（修复后） | 优 |
| 迁移成本 | M | M | **S** | L |
| 风险 | 文件夹隐喻争议 | 饱和度下降 | 无 | 观感偏离 GNOME |

**推荐**：先落 **C**（它同时就是公共修复本身，半天见效）；若想更进一步，在 C 之上做 **B** 的
「去渐变化」变量重构最划算（图形复用率最高）；A 与 D 适合作为 iconTheme 的第三/第四档长期演进，
不建议一次到位。

---

## 3. 色相语义再平衡（四方案通用）

现状 13 项挤在 blue。建议按「类目→色相」固定映射，书签区立刻可扫读：

| 色相 | 语义域 | 调整项 |
|---|---|---|
| blue | 网络与连接 | browser、network、connection（保留）；**移出**：documents→gray、music→purple、videos→orange、about→gray、search→gray、database→teal（新增色相或并入 blue 但图形区分）、file-audio→purple、file-video→orange |
| green | 成功/监控/下载 | monitor、download、file-json（保留） |
| orange | 创作/编辑 | editor、home、file-code（保留）；videos 加入 |
| purple | 媒体/图片 | image、pictures、music 加入 |
| red | 危险/删除 | trash、pdf（保留） |
| yellow | 警示/锁/压缩 | lock、file-archive（保留） |
| brown | 归档/包 | archive、package（保留） |
| gray | 文本/系统 | file-text、config、settings、terminal、disk、hex（保留）；documents、about 加入 |

同时补 `FILE_ICONS` 缺口：`.pdf→pdf`、`.xlsx/.csv→新增 spreadsheet（green）`、
`.7z/.rar→file-archive`、`.wav/.flac/.ogg→file-audio`、`.doc/.docx→file-text`、`.iso→disk`。

---

## 4. 公共修复清单（无论选哪套，先做这些）

1. **P0**：`AppIcon.tsx` DEFS 色相数组加入 `'folder', 'paper'`
   （`icg-folder` 用 `--ic-folder-light/surface`，`icg-paper` 用 `--ic-paper-1/2`）——一行根治纸身/前板透明。
2. **P0**：删 `variables.css` paper-light 组重复的 folder 行（#209）。
3. **P1**：`AppBadge.tsx` `APP_COLORS.c` 全部改 `var(--ic-<c>-surface)`（或 classic 专用三阶变量），classic 徽章随主题/亮暗换挡。
4. **P2**：修 `hex` 图形路径（对齐 AppBadge hex-viewer 的四线构图）；删 `bell-outline` 或赋予真 outline 形。
5. **P2**：`DEFS` 提为全局单例（App 根部渲染一个隐藏 `<svg><defs>`，各图标引用文档级 id），16N→16 个渐变节点。
6. **P2**：按 §3 补 `FILE_ICONS` 映射与色相再平衡。
7. 建议分支：`fix/icon-defs-missing-gradient`（P0 两项可先行单独合入 dev）。

## 5. 后续可做的基建

- 图标即构建产物：加一个 `scripts/gen-icons.ts`，把 SVG 字符串表改为从 `.svg` 源文件生成 + 单测校验
  「DEFS 引用必有定义」（本可直接拦住 P0）。
- `symbolic.test.tsx` 增加断言：所有 `url(#…)` 引用 id 必须存在于同文档。
- 设置页图标主题预览卡（Settings.tsx:916-953）已支持 `data-icon-theme` 容器覆盖，新方案可直接挂进现有双卡预览。
