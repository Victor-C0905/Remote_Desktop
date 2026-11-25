# Quirel 发布链路自动化可行性评估（含 quirel-web）

> 日期：2026-11-24
> 性质：只读评估。未修改代码、未提交、未部署、未发布任何产物。
> 评估对象：`E:\MyWork\Quirel`（客户端 + quireld）与 `E:\MyWork\Web\quirel_web`（官网与资源发布后台）。
> 前置研究：[2026-11-24-release-process-research.md](file:///e:/MyWork/Quirel/docs/superpowers/specs/2026-11-24-release-process-research.md)。

---

## 0. 直接结论

**当前项目不支持“一键自动发布”。**

现状是“每个环节都有脚本或后台能力，但没有人把它们串起来”：版本冻结、构建、校验、上传、发布说明、官网部署分别落在本地命令、手工操作和 quirel-web 后台之间，缺少统一编排、CI、发布凭据与回滚机制。

但距离“一键”并不远，最短可行路径是：

1. 用户仍在 main 上执行 `npm run release <版本号>` 并推送 tag（AGENTS.md 规定的不可让渡环节）。
2. 由 **tag 触发的 CI** 完成预检、Windows 客户端构建、Linux 双架构 quireld 构建、校验和生成。
3. 生产发布 job 通过人工审批后，调用 quirel-web 的原子发布 API 上传并发布 quireld 与客户端安装包。

这样可做到“**用户推一个 tag + 一次审批，其余自动**”。如果“一键”指从 dev 状态开始连版本号、tag、合并都自动完成，则**与 AGENTS.md §4 冲突，当前不支持，需先改治理规则**。

另外必须区分两件事：

- **自动发布**：把产物构建并放到官网/下载源。本文讨论的范围。
- **自动更新**：客户端自己发现并安装新版本。Quirel 客户端**没有** Tauri updater，即使发布全自动，老客户端也不会自动升级。

---

## 1. 评估范围与证据快照

### 1.1 两套仓库

| 仓库 | 路径 | 远端 | 作用 |
|---|---|---|---|
| Quirel | `E:\MyWork\Quirel` | GitHub `Victor-C0905/Remote_Desktop` + Gitee | 客户端、quireld、协议、发布脚本 |
| quirel_web | `E:\MyWork\Web\quirel_web` | **仅 Gitee** `qiang-wei-chen/quirel_web` | 官网、下载页、安装包/quireld 发布后台 |

### 1.2 研究时的重要仓库状态

- Quirel 当前在 `feature/fm-sidebar-tree`；`main` 为 `0.3.3`，带轻量 tag `v0.3.3`；另有异常 tag `v0.3.4`（详见前置研究 §8.1）。
- Quirel 根目录无 `.github/`，没有任何 CI 工作流。
- quirel_web 工作区存在大量未提交修改；其中 `src/main/java/com/quirel/quireld/`、`src/main/java/com/quirel/release/Versions.java` 等是**未跟踪文件**。也就是说，下文提到的 quireld 原子发布 API **尚未进入任何提交**，不能当作已固化的仓库能力。
- quirel_web 没有 `.github/`，也没有其他 CI 配置。
- quirel_web 的 `install.sh` 只检查 `java` 是否存在并打印版本，提示“JDK 21+”；而 `pom.xml` 的 `<java.version>` 是 **25**。服务器运行时 JDK 版本是部署自动化的前置条件。

### 1.3 “自动发布”的判定口径

本文把自动发布拆成 7 个可独立自动化的动作：

1. 版本冻结（改 4 个版本文件 + lock）
2. 预检（分支、tag、版本一致性、测试）
3. 客户端构建（Windows NSIS）
4. quireld 构建（Linux amd64/arm64）
5. 产物校验与清单（SHA-256、latest.json）
6. 产物上传与线上发布
7. 发布说明与记录

“一键”指上述 1–7 中，除 AGENTS 明确保留给用户的动作外，其余由一次触发完成。

---

## 2. 自动化成熟度矩阵

| 环节 | 现有实现 | 自动化成熟度 | 主要缺口 |
|---|---|---|---|
| 版本冻结 | [release.mjs](file:///e:/MyWork/Quirel/scripts/release.mjs) | 部分：脚本化、有守卫，但不可重入 | 必须在 main、工作区干净；失败留半成品；tag 在构建前产生 |
| 发布预检 | [verify.mjs](file:///e:/MyWork/Quirel/scripts/verify.mjs)（仅 dev 系） | 部分 | main 无该脚本；无 tag 祖先/版本一致性/产物冒烟检查 |
| 客户端构建 | `npm run tauri build`；[tauri.conf.json](file:///e:/MyWork/Quirel/src-tauri/tauri.conf.json) | 高：单命令 | 仅 Windows 本地；无 CI；无签名 |
| quireld 构建 | [agent/build.sh](file:///e:/MyWork/Quirel/agent/build.sh) | 高：单命令/架构 | 依赖 WSL/Linux + arm64 交叉工具链；无 CI |
| 校验和/清单 | build.sh 重建 `SHA256SUMS` | 高 | 只覆盖 tar.gz，不覆盖客户端安装包 |
| quireld 官网发布 | quirel_web 原子 API（未提交） | 中高：后端已实现暂存→发布 | 未提交；认证只有管理员密码 Cookie；无 CI token |
| 客户端安装包发布 | quirel_web `/api/upload` + `/api/release` | 中：后台可手动完成 | 无自动发布说明；Release JSON 需整体构造；无 CI token |
| quirel-web 自身部署 | `pack-deploy.sh` + `install.sh` + 手工 scp | 中：步骤固定、可脚本化 | 无 CI；需 SSH/sudo；Gitee-only；JDK 版本需确认 |
| 发布说明 | 无 | 低 | 无 CHANGELOG/生成器；quirel_web 发布校验强制 notes 非空 |
| 回滚 | quirel_web 版本历史/回滚、RUNBOOK | 中 | 发布流水线无自动回滚；tag 回滚靠人工 |

---

## 3. 硬缺口逐条

### 3.1 构建环境

#### 客户端（Windows NSIS）

- 仓库只配置了 NSIS 目标，没有 CI（[tauri.conf.json](file:///e:/MyWork/Quirel/src-tauri/tauri.conf.json#L24-L24)）。
- 构建需要 Windows + Node + Rust（MSVC）+ Tauri CLI（[package.json](file:///e:/MyWork/Quirel/package.json#L56)）+ NSIS 打包器。GitHub 托管 `windows-latest` 具备主体能力，NSIS 通常由 Tauri 构建期下载（推断，依赖网络）。
- 当前没有构建缓存、没有可复现环境锁定、没有产物签名。
- **结论**：不是硬阻塞，但必须新增 Windows runner job；首次搭建需要验证 NSIS 下载、WebView2 依赖与长路径问题。

#### quireld（Linux 双架构）

- `build.sh` 明确要求 arm64 预装 `aarch64-unknown-linux-gnu` target 与 `aarch64-linux-gnu-gcc`，缺失会直接报错退出（[build.sh](file:///e:/MyWork/Quirel/agent/build.sh#L22-L23)、[build.sh](file:///e:/MyWork/Quirel/agent/build.sh#L177-L186)）。
- amd64 本机构建，arm64 交叉编译（[build.sh](file:///e:/MyWork/Quirel/agent/build.sh#L193-L211)）。
- **结论**：Linux runner 可完成；需要 `rustup target add` 与 `apt install gcc-aarch64-linux-gnu`。若未来引入原生依赖，交叉编译复杂度会上升。

#### quirel-web

- 构建需要 JDK 25 + Maven Wrapper（[pom.xml](file:///E:/MyWork/Web/quirel_web/pom.xml#L9-L21)）。
- 部署包由 [pack-deploy.sh](file:///E:/MyWork/Web/quirel_web/deploy/pack-deploy.sh#L1-L40) 生成，ZIP 或 tar.gz 二选一。
- `install.sh` 只要求 `java` 存在并提示 JDK 21+（[install.sh](file:///E:/MyWork/Web/quirel_web/deploy/install.sh#L61-L66)），与 pom 的 Java 25 不一致。服务器 JDK 不足时，CI 构建成功但部署启动失败。
- **结论**：构建可自动化；部署需要 systemd + sudo + 正确 JDK，属于基础设施前置条件。

### 3.2 代码签名与凭据

- 客户端没有 Authenticode 证书配置：`tauri.conf.json` 的 bundle 段无签名项，仓库无证书/签名命令。当前产物默认未签名（推断），会触发 SmartScreen。
- 若要做 Tauri 自动更新，还需要 updater 签名密钥；当前完全没有。
- quirel_web 的后台认证是**单一管理员密码 + Cookie**：`POST /api/login` 只接收 password（[AuthController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/auth/AuthController.java#L23-L36)），除登录/检查/健康外的 `/api/**` 全部需要登录（[WebConfig.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/config/WebConfig.java#L65-L66)）。
  - CI 可以用 `QUIREL_WEB_ADMIN_PASSWORD` 登录并保存 Cookie，技术上可行。
  - 但这等于把人类管理员口令放进 CI secret，权限过大、无法审计区分“人”和“流水线”。**建议新增最小权限的发布 token**（只允许上传/发布，不允许改密码/删除记录）。
- quirel-web 部署需要服务器 SSH 凭据与 sudo 权限（`install.sh` 要求 root 或 sudo，[install.sh](file:///E:/MyWork/Web/quirel_web/deploy/install.sh#L50-L50)）。当前没有部署密钥管理方案。
- **结论**：不签名也能自动发布，但“可信分发”不成立；CI token 与 SSH/sudo 凭据是自动发布的硬前置。

### 3.3 自动更新通道

- 客户端没有 Tauri updater：`plugins` 为空（[tauri.conf.json](file:///e:/MyWork/Quirel/src-tauri/tauri.conf.json#L38-L38)），无 `tauri-plugin-updater` 依赖，也无更新检查代码。自动发布只解决“下载页有新包”，不解决“老客户端升级”。
- quireld 有独立更新通道：客户端按自身版本精确取包（[provision.rs](file:///e:/MyWork/Quirel/src-tauri/src/provision.rs#L37-L41)、[provision.rs](file:///e:/MyWork/Quirel/src-tauri/src/provision.rs#L823-L830)），官网主源/备源/GitHub 兜底，SHA-256 不匹配即中断（[provision.rs](file:///e:/MyWork/Quirel/src-tauri/src/provision.rs#L1062-L1074)）。
- quirel_web 已实现 quireld 的**原子发布顺序**：先写稳定别名与双份 `SHA256SUMS`，最后原子替换 `latest.json`（[QuireldService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/quireld/QuireldService.java#L174-L204)）。这比 Quirel 仓库里的手工上传脚本更安全。
- 但该模块当前**未提交**；Quirel 仓库里的 [publish-quireld.mjs](file:///e:/MyWork/Quirel/scripts/publish-quireld.mjs) 在 dev 系还有 semver 正则缺陷（前置研究 §8.1 R2）。
- **结论**：quireld 的更新通道已具备自动化条件（提交并部署后）；客户端自动更新是完全缺失的产品能力，不属于发布流水线能补齐的部分。

### 3.4 产物上传目标

| 产物 | 目标 | 现状 | 自动化判断 |
|---|---|---|---|
| quireld tar.gz + 引导脚本 | quirel_web `/downloads/quireld/` | 后台 API 已实现：`POST /api/admin/quireld/upload?version=&arch=`、`POST /api/admin/quireld/script`、`POST /api/admin/quireld/publish?version=`（[QuireldController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/quireld/QuireldController.java#L33-L64)） | 可自动化；需 token/密码与“模块已提交部署” |
| 客户端 NSIS | quirel_web `/downloads/` + 发布记录 | `POST /api/upload?kind=installer&name=...`（[FileController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/file/FileController.java#L69-L95)），再 `PUT /api/release` 或 `POST /api/release/publish`（[ReleaseController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/ReleaseController.java#L59-L100)） | 可自动化但耦合 Release JSON；必须先有发布说明 |
| quirel-web 程序 | 服务器 `/opt/quirel-web` | 手工 scp + `install.sh`，见 [RUNBOOK.md](file:///E:/MyWork/Web/quirel_web/deploy/RUNBOOK.md#L92-L133) | 可脚本化；需 SSH/sudo、备份与回滚 |
| GitHub Releases | GitHub | 仅 quireld 可选 `--github`（[publish-quireld.mjs](file:///e:/MyWork/Quirel/scripts/publish-quireld.mjs#L190-L233)） | 可自动化；客户端无 GitHub Release 流程 |

客户端发布记录不是“上传文件即完成”，还要通过发布校验：版本规范、通道合法、发布说明非空、至少一个平台绑定可下载且已登记 SHA-256（[ReleaseService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/ReleaseService.java#L241-L282)）。这意味着**自动发布说明生成器是客户端自动发布的前置条件**。

### 3.5 版本号与 tag 的产生时机

- `release.mjs` 的顺序是：改版本 → commit → **打 tag** → 尝试快进 dev（[release.mjs](file:///e:/MyWork/Quirel/scripts/release.mjs#L101-L161)）。
- 构建是 tag 之后的独立步骤。若 CI 采用“tag push 触发构建”，构建失败时 tag 已经存在，且指向一个未成功产出安装包的提交。
- 现有脚本没有 tag 存在性预检；`git tag` 失败会留下无 tag 的 release 提交。
- 版本号决策与 tag 创建按 AGENTS.md 必须由用户在 main 上完成（[AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L45-L54)）。
- **结论**：CI 不适合创建版本号或 tag；自动化应从“tag 已由用户推送”开始。若要避免“tag 先于构建成功”，可选：
  - 在 release 前用 `workflow_dispatch` 跑一次候选构建（不发布）；
  - 或给 release 脚本增加“只 bump 不 tag / 构建成功后补 tag”的两阶段模式（需改脚本与 AGENTS 表述）；
  - 或接受 tag 失败后人工删除重打（不推荐，容易违反“版本不复用”）。

### 3.6 发布说明与校验

- 仓库无 CHANGELOG、无 release notes 生成器。
- quirel_web 的客户端发布强制 `notes` 非空（[ReleaseService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/ReleaseService.java#L258-L260)）。
- 现有 `verify.mjs` 只做 lockstep 与编译/测试，不做“安装包能否启动/解压后版本是否正确”的产物冒烟测试。
- **结论**：自动发布必须补三样：发布说明生成、产物冒烟、发布后验证（`/release.json`、`/downloads/...`、SHA-256）。

### 3.7 CI 与仓库基础设施

- Quirel 有 GitHub origin，可跑 GitHub Actions；但仓库没有 Actions 配置，是否启用、是否私有仓库计费、是否有分支保护均未知。
- quirel_web **只有 Gitee 远端**，不能直接使用 GitHub Actions。可选：
  - Gitee Go（需确认套餐/runner 能力）；
  - 自建 runner；
  - 将 quirel_web 镜像到 GitHub 后跑 Actions（需接受双远端同步）。
- quirel_web 当前有大量未提交改动，其中 quireld 发布模块未跟踪。自动化不能建立在未提交代码上。
- **结论**：CI 基础设施与仓库同步策略是组织级前置，不是写一个 YAML 就能解决。

---

## 4. 与 AGENTS.md 的边界：哪些能自动，哪些必须人工

### 4.1 必须保留人工

| 动作 | 依据 | 原因 |
|---|---|---|
| 决定版本号与发版时机 | AGENTS.md §4（L48、L51） | 版本语义与发布节奏是产品决策 |
| 在 main 上执行 `npm run release` | AGENTS.md §4（L48） | 唯一版本写入口 |
| squash 合并任务分支到 dev | AGENTS.md §3（L40-L42） | 合并策略与提交历史由用户负责 |
| dev 合入 main | AGENTS.md §4（L49） | 发布基线由用户确认 |
| 推送 main/dev/tag | AGENTS.md §3（L40-L42）、§4（L48） | 远端写操作必须用户同意/执行 |
| 生产发布审批 | 建议新增 | 发布是对外动作，建议人工 gate |
| 客户端自动更新策略 | 产品决策 | 涉及签名、兼容与回滚 |

### 4.2 可以自动化

| 动作 | 说明 |
|---|---|
| 预检 | tag 格式、版本文件与 tag 一致、tag 提交在 main、`npm run verify`、`wire_compat` |
| 客户端构建 | Windows runner 执行 `npm ci` + `npm run tauri build` |
| quireld 构建 | Linux runner 安装 arm64 target/交叉编译器后执行 `build.sh` 两次 |
| 校验和 | 对安装包与 tar.gz 生成 SHA-256，校验 `SHA256SUMS` |
| 发布说明草稿 | 从 `git log v上一版..HEAD` 生成结构化 notes，允许人工修改 |
| 产物上传 | quirel_web 暂存上传，不切换线上 |
| 线上切换 | 审批后调用 quirel_web publish / `PUT /api/release` |
| GitHub Release | `gh release create/upload --clobber` |
| quirel-web 部署 | 审批后 SSH 备份、替换程序、健康检查、失败回滚软链 |
| 发布后验证 | 下载两个域名、校验 SHA、检查 `latest.json` 与 `/release.json` |

关键原则：**自动化只消费“用户已经产生的 tag”，不生产版本号或 tag**。这样不越界。

---

## 5. CI 方案（GitHub Actions / 自建 runner）

### 5.1 触发方式

推荐双触发：

- `push: tags: ['v*']`：正式发布主路径，tag 由用户本地脚本产生并推送。
- `workflow_dispatch`：仅用于“候选构建/重跑失败 job”，默认不发布；输入必须指定已存在的 tag。

不建议 `workflow_dispatch` 输入版本号并让 CI bump/tag，那会直接违反 AGENTS.md §4。

### 5.2 Job 设计

| Job | Runner | 内容 | 产物 |
|---|---|---|---|
| `preflight` | ubuntu-latest | 校验 tag 格式、4 个版本文件等于 tag、tag 提交在 main、`verify`、`wire_compat`、标签未复用 | 通过/失败 |
| `build-client` | windows-latest | Node + Rust MSVC + `npm ci` + `npm run tauri build`；计算安装包 SHA-256 | NSIS exe + sha256 |
| `build-quireld` | ubuntu-latest | `rustup target add aarch64-unknown-linux-gnu`、`apt install gcc-aarch64-linux-gnu`、`build.sh release amd64/arm64` | 双 tar.gz + SHA256SUMS + install-quireld.sh |
| `publish-quireld` | ubuntu-latest | 下载双架构产物；登录 quirel_web；上传两架构与脚本；publish；校验 `latest.json` | 线上 quireld 版本 |
| `publish-client` | ubuntu-latest | 上传安装包；构造 Release JSON（版本/notes/平台 URL）；发布；校验 `/release.json` | 线上客户端版本 |
| `deploy-quirel-web` | 自建/Gitee runner | `./mvnw -B clean package`、`pack-deploy.sh`、备份、SCP、`install.sh`、健康检查 | 官网新程序 |

`publish-*` 放在 GitHub Environment（如 `production`）下，配置 Required reviewers；审批前只做构建与暂存，不切换线上。

### 5.3 Runner 能力

| 能力 | 用途 | 说明 |
|---|---|---|
| Windows Server + MSVC Build Tools | Tauri 编译 | `windows-latest` 自带 |
| Node LTS + npm | 前端与 Tauri CLI | 建议锁定 Node 版本 |
| Rust stable | 两端 | 2024 edition，需 1.85+ |
| NSIS | 客户端打包 | Tauri 构建期获取（需网络） |
| `gcc-aarch64-linux-gnu` | arm64 交叉链接 | Ubuntu apt |
| `sha256sum` / `tar` / `zip` / `curl` / `jq` | 校验、打包、API 调用 | Ubuntu 自带 |
| JDK 25 + Maven Wrapper | quirel_web 构建 | 与 pom 一致 |
| SSH + sudo | quirel_web 部署 | 需 NOPASSWD 或受控 sudo 规则 |

### 5.4 需要的 Secrets / 仓库设置

| 名称（示例） | 用途 | 备注 |
|---|---|---|
| `QUIREL_WEB_ADMIN_PASSWORD` | 登录 quirel_web 后台 | 过渡方案；权限过大，建议后续换 token |
| `QUIREL_WEB_DEPLOY_TOKEN` | 上传/发布专用 token | 需在 quirel_web 新增最小权限认证 |
| `QUIREL_WEB_SSH_KEY` / `HOST` / `USER` | 部署 quirel-web | 需 sudo 权限与备份目录 |
| `WINDOWS_CERT_PFX_BASE64` / `PASSWORD` | 客户端签名（可选） | 当前无证书 |
| `TAURI_SIGNING_PRIVATE_KEY` | 未来 Tauri updater | 当前无 updater |
| `GITHUB_TOKEN` | GitHub Release | 默认 token，需 `contents: write` |
| Gitee 凭据 | quirel_web 镜像/Gitee Go | 若采用 Gitee CI |

仓库设置：

- Actions 启用；tag 推送不被分支保护阻断；`main` 保护策略允许发布提交（由用户推送）。
- 私有仓库需确认 Actions 额度。
- GitHub Environment `production` 配置审批人。
- quirel_web 若走 GitHub Actions，需要镜像仓库与同步策略；否则用 Gitee Go/自建 runner。

### 5.5 必须改动的仓库内容

| 改动 | 仓库 | 规模 | 说明 |
|---|---|---|---|
| 提交 quireld 发布模块与 `Versions.java` | quirel_web | S | 当前未跟踪，自动化前提 |
| 合并 semver 正则修复或改用 quirel_web API | Quirel | S | 旧 `publish:quireld` 主链路可被 API 替代 |
| 新增 release workflow | Quirel | M | 预检 + 两平台构建 + 审批发布 |
| 新增发布说明生成脚本 | Quirel | S-M | 从 git log 生成 notes，供 quirel_web 校验 |
| 新增 CI 发布 token（可选但推荐） | quirel_web | S | 避免在 CI 存管理员密码 |
| 新增 quirel-web 部署 workflow + 远程升级脚本 | quirel_web | M | 备份、替换、健康检查、回滚 |
| 统一 JDK 25 前置 | quirel_web | S | install.sh 检查与文档对齐 |

---

## 6. 分阶段方案

### 阶段 0：补齐自动化前提（不做自动发布）

目标：把“可自动化能力”变成可依赖的仓库事实。

- 提交 quirel_web 的 quireld 发布模块与 `Versions.java`，确认已部署并可用。
- 合并 `fix/publish-semver-regex`，或明确废弃 `publish:quireld` 的主链路上传职能，仅保留 GitHub 兜底。
- 对齐 main 与 dev 的 `verify.mjs` / `publish-quireld.mjs` / 文档差异。
- 决定客户端签名与 updater 是否在本轮范围内。
- 确定客户端发布说明来源与格式。
- 明确 quirel-web 部署的 SSH 用户、sudo 策略、备份目录与 JDK 25。

改动量：小（以提交/合并/文档为主）。
风险：低；不改变现有发布方式。

### 阶段 1：最小可用半自动（一次触发构建，人工发布）

目标：消除本地双环境构建，保留人工上传。

流程：

1. 用户合并 dev→main，执行 `npm run release <版本号>`，推送 main 与 tag。
2. CI 的 tag workflow 完成预检、Windows 客户端构建、Linux 双架构 quireld 构建，上传为 Actions Artifacts / 临时 Release。
3. 用户从 Artifacts 下载，按现有 quirel_web 后台上传并发布；或继续使用现有手工上传。

改动量：中（1 个 workflow + 预检脚本）。
前置：GitHub Actions 可用；tag 推到 GitHub。
风险：

- 构建成功但发布仍靠人工，未真正“一键”。
- tag 先于构建，失败需删 tag/重打。
- 无签名，安装包仍会被 SmartScreen 提示。

### 阶段 2：进阶自动发布（tag + 一次审批，其余自动）

目标：真正的“一次触发、一次审批”。

流程：

1. 用户仍执行 release 并推送 tag。
2. CI 预检 → 双平台构建 → 产物校验 → 暂存上传（quireld 两架构 + 客户端安装包）→ 等待 `production` 环境审批。
3. 审批通过：quirel_web publish quireld（原子切换 `latest.json`）→ 发布客户端 Release 记录 → 可选 GitHub Release → 双域名下载与 SHA 校验。
4. 任一步失败：不上线；已暂存文件保留但不生效；告警。

前置：

- quirel_web quireld 模块已提交部署。
- 最小权限发布 token（推荐）或管理员密码 secret。
- 发布说明生成器。
- 生产环境审批人。
- 客户端安装包上传/发布 API 的稳定约定（建议新增“只更新客户端版本”的专用端点，避免整体 Release JSON 覆盖截图/平台配置）。

改动量：中（workflow 2–3 个、脚本 2–3 个、quirel_web 小改）。
风险：

- 管理员密码进 CI 的权限风险；token 未落地前不建议长期使用。
- Release JSON 整体 PUT 可能覆盖人工维护的官网内容；需“读—改—写”或专用端点。
- 发布后客户端无自动更新，用户仍需手动下载。

### 阶段 3：一键（含版本与 tag）——需改治理，不建议默认实施

目标：一个按钮完成 bump、tag、构建、发布。

- 需要 `workflow_dispatch` 输入版本号，CI 在 main 上执行 release 脚本并推送 tag。
- 这与 AGENTS.md §4“版本只能由用户在 main 上跑 `npm run release`”“push 由用户执行”直接冲突。
- 若坚持，需要用户明确修改 AGENTS.md，并接受 CI 持有 main 写权限与 tag 权限。
- 即使做了，客户端自动更新仍需单独的 Tauri updater + 签名工程。

改动量：中到大（治理 + 脚本 + 权限）。
风险：高；发布权从人转移到流水线，审计与回滚要求更高。

---

## 7. 主要风险与应对

| 风险 | 影响 | 应对 |
|---|---|---|
| tag 先于构建 | 构建失败留下“已发布但无产物”的 tag | 候选构建预演；或两阶段 release（bump→构建→tag） |
| 无签名 | SmartScreen 警告、下载信任低 | 采购证书 + Tauri 签名配置 + CI secret；或明确接受 |
| 无客户端 updater | 老客户端长期停留在旧版 | 单独立项：Tauri updater + 签名 + 更新源 |
| quirel_web 模块未提交 | 自动化建立在未跟踪代码上 | 先提交、部署、验证 API |
| 管理员密码进 CI | 凭据权限过大、无法区分人/机器 | 新增最小权限发布 token |
| quirel_web Gitee-only | 无法直接用 GitHub Actions | Gitee Go/自建 runner/镜像仓库 |
| JDK 版本不一致 | 构建成功但服务器启动失败 | 统一 JDK 25；install.sh 增加版本硬校验 |
| Release JSON 覆盖 | 自动发布覆盖人工维护的官网内容 | 读—改—写；或新增专用端点 |
| 版本号复用/异常 tag | 客户端更新判断混乱 | 预检拒绝已存在 tag；清理 `v0.3.4` |
| 发布后无法回滚客户端 | 用户已下载坏包 | 保留版本历史；官网回滚到上一版；安装包不可覆盖同名旧版（或明确覆盖策略） |
| Actions 私有仓库额度 | 流水线不可用或额外成本 | 自建 runner/确认套餐 |

---

## 8. 明确回答

### 8.1 当前项目是否支持“一键自动发布”？

**不支持。**

准确表述是：

- **构建层面**：已高度脚本化，但需要 Windows 与 Linux 两套环境，当前没有 CI 编排。
- **发布层面**：quirel_web 已具备原子发布能力（quireld），客户端安装包也可通过后台发布，但都需要人工操作与凭据。
- **治理层面**：版本号、release 脚本、合并、tag、push 按规定必须由用户执行，不能被 CI 全自动替代。
- **更新层面**：客户端没有自动更新通道，发布自动化不等于用户自动升级。

### 8.2 最短路径

按优先级排序：

1. **提交并部署 quirel_web 的 quireld 发布模块**，确认 `/api/admin/quireld/*` 在线上可用；合并 semver 正则修复（或不再依赖旧上传脚本）。
2. **新增 Quirel 的 tag 触发 GitHub Actions**：`preflight` + `build-client`（Windows）+ `build-quireld`（Linux 双架构）。
3. **给 quirel_web 增加最小权限发布 token**（或在过渡期使用管理员密码 secret），让 CI 能上传/发布但不掌握全权。
4. **新增发布说明生成器**：`git log v上一版..HEAD` → notes；满足 quirel_web 的发布校验。
5. **加 `production` 环境保护与审批**，审批后自动发布 quireld + 客户端安装包，并做双域名/SHA 校验。
6. （可选，独立立项）**客户端代码签名与 Tauri updater**；不与发布流水线捆绑。

做到第 1–5 步后，发布体验是：

> 用户在 main 上执行一次 `npm run release`，推送 tag；CI 全自动构建并暂存；审批一次后自动上线。

这是当前治理规则下能达到的最接近“一键”的状态。要再进一步（连 release/tag 都自动），必须先修改 AGENTS.md §4，把发布权交给流水线。

---

## 9. 事实与推断分界

### 9.1 仓库中确实如此

- Quirel 无 CI；客户端仅 NSIS；无签名配置；无 Tauri updater。
- `release.mjs` 在构建前打 tag，且不推送、不构建。
- `build.sh` 的 arm64 交叉编译前置检查与双架构产物。
- quirel_web 有上传/发布 API、原子 `latest.json` 写入、发布前 notes/制品校验。
- quirel_web 仅有 Gitee 远端；工作区中 quireld 模块未跟踪。
- quirel_web 部署依赖 systemd + sudo，JDK 提示 21+ 而 pom 为 25。

### 9.2 推断

- GitHub 托管 Windows runner 可完成 Tauri NSIS 构建（需构建期下载 NSIS）。
- GitHub Actions 在私有仓库的可用性与额度取决于账户设置。
- quirel_web 线上是否已部署未提交的 quireld API，无法从仓库确认。
- Tauri 缺省使用 Cargo 版本、未配置签名时产物未签名，属于框架默认行为。
- 管理员密码 Cookie 登录在 CI 中可用，但没有 CSRF/短期令牌机制的证据；按现有代码推断可直接用 curl 维持 Cookie。

---

## 10. 关键证据索引

| 主题 | 文件 |
|---|---|
| 分支/发版规则 | [AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L45-L54) |
| 版本冻结 | [release.mjs](file:///e:/MyWork/Quirel/scripts/release.mjs) |
| 客户端打包 | [tauri.conf.json](file:///e:/MyWork/Quirel/src-tauri/tauri.conf.json) |
| quireld 构建 | [build.sh](file:///e:/MyWork/Quirel/agent/build.sh) |
| 旧官网发布脚本 | [publish-quireld.mjs](file:///e:/MyWork/Quirel/scripts/publish-quireld.mjs) |
| 客户端自动安装 | [provision.rs](file:///e:/MyWork/Quirel/src-tauri/src/provision.rs) |
| 官网发布 API | [QuireldController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/quireld/QuireldController.java) |
| quireld 原子发布 | [QuireldService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/quireld/QuireldService.java#L174-L204) |
| 客户端安装包上传/发布 | [FileController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/file/FileController.java#L69-L95)、[ReleaseController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/ReleaseController.java#L59-L100) |
| 发布校验 | [ReleaseService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/ReleaseService.java#L241-L282) |
| 认证 | [AuthController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/auth/AuthController.java#L23-L36) |
| quirel-web 部署/回滚 | [RUNBOOK.md](file:///E:/MyWork/Web/quirel_web/deploy/RUNBOOK.md#L92-L133) |
| quirel-web 打包 | [pack-deploy.sh](file:///E:/MyWork/Web/quirel_web/deploy/pack-deploy.sh) |
| quirel-web 运行时 | [pom.xml](file:///E:/MyWork/Web/quirel_web/pom.xml#L9-L21)、[install.sh](file:///E:/MyWork/Web/quirel_web/deploy/install.sh#L61-L66) |
