# Quirel × quirel_web 跨项目发布开发方案（RELEASE-CONTRACT v1）

> 日期：2026-11-24
> 状态：方案（只读产出，未改代码、未改 AGENTS.md、未提交、未发布）
> 契约版本：**RELEASE-CONTRACT v1**
> 权威版本：本文件位于 Quirel 仓库，是唯一权威版本。quirel_web 仓库中的副本为派生副本，本地化后需回指本文件。
> 前置研究：
> - [2026-11-24-release-process-research.md](file:///e:/MyWork/Quirel/docs/superpowers/specs/2026-11-24-release-process-research.md)
> - [2026-11-24-release-automation-assessment.md](file:///e:/MyWork/Quirel/docs/superpowers/specs/2026-11-24-release-automation-assessment.md)
> - quirel_web 侧评估：`E:\MyWork\Web\quirel_web\docs\2026-11-24-auto-release-assessment.md`

---

## 0. 一页结论

- 两端**都有能力**，但**没有契约**：Quirel 会构建、会算哈希；quirel_web 会收、会暂存、会原子发布；缺口在“谁在什么时机、用什么凭据、调用哪个端点、失败怎么退”。
- 本方案只定义**职责、接口、阶段和红线**，不实施。
- 自动化只能**消费**用户在 main 上产生的 `vX.Y.Z` tag；不得生产版本号、tag、合并或推送。
- 最短闭环仍是：**用户 release + 推 tag → CI 构建/校验/暂存 → 一次生产审批 → 调用 quirel_web API 上线 → 双域名回读校验**。
- 阶段 0 必须先清掉四个已确认的具体问题（quireld 模块未提交、semver 正则缺陷、main/dev 脚本漂移、RUNBOOK 升级删光旧 jar 导致回滚失效），否则自动化建立在流沙上。

---

## 1. 职责边界

### 1.1 Quirel 侧（`E:\MyWork\Quirel`）

| 职责 | 交付物 | 依据 |
|---|---|---|
| 版本冻结入口 | `npm run release <版本号>` 产出 4 个版本文件 + 3 个 lock + `chore(release)` 提交 + 轻量 tag `vX.Y.Z` | [release.mjs](file:///e:/MyWork/Quirel/scripts/release.mjs)、[AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L45-L54) |
| 版本一致性机检 | lockstep 校验（quirel / quireld / quirel-protocol / package.json） | [verify.mjs](file:///e:/MyWork/Quirel/scripts/verify.mjs#L75-L97)、[AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L62-L62) |
| 客户端构建 | Windows NSIS 安装包（x64） | [tauri.conf.json](file:///e:/MyWork/Quirel/src-tauri/tauri.conf.json#L24-L24)、[package.json](file:///e:/MyWork/Quirel/package.json#L56-L56) |
| quireld 构建 | Linux amd64 / arm64 两个 `tar.gz` + `SHA256SUMS` | [build.sh](file:///e:/MyWork/Quirel/agent/build.sh#L177-L186)、[build.sh](file:///e:/MyWork/Quirel/agent/build.sh#L238-L274) |
| 产物命名与校验值 | `quireld-{version}-release-linux-{arch}.tar.gz`、`SHA256SUMS`（`hash␠␠name` 两空格） | [build.sh](file:///e:/MyWork/Quirel/agent/build.sh#L284-L298)、[provision.rs](file:///e:/MyWork/Quirel/src-tauri/src/provision.rs#L823-L830) |
| 引导脚本 | `agent/deploy/install-quireld.sh` 原始文件 | [publish-quireld.mjs](file:///e:/MyWork/Quirel/scripts/publish-quireld.mjs) |
| tag 语义 | 轻量 tag `vX.Y.Z`，打在该次 `chore(release)` 提交上；不复用 | [AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L51-L51) |
| 发布预检 | tag 格式、4 个版本文件 == tag、tag 提交在 main、`npm run verify`、协议兼容回归 | 本方案新增（CI） |
| 发布说明草稿 | 从 `git log v上一版..HEAD` 生成结构化纯文本，供人工修改 | 本方案新增（CI） |
| 发布调用 | 用最小权限 token 调 quirel_web 的 API；发布后双域名回读校验 | 本方案新增（CI） |

**Quirel 明确不做：**

- 不决定版本号、不决定发版时机（用户决定）。
- 不创建 tag、不合并、不推送 main/dev（用户执行）。
- 不直接写 quirel_web 的磁盘文件或数据库（只走 HTTP API）。
- 不修改 `latest.json` / `/release.json` 的生成规则（那是 quirel_web 的实现）。
- 不删除 quirel_web 上的历史版本；不回滚线上版本（回滚是人工动作）。
- 不决定 stable/beta 通道切换。

### 1.2 quirel_web 侧（`E:\MyWork\Web\quirel_web`）

| 职责 | 实现 | 依据 |
|---|---|---|
| 制品接收 | 流式上传，服务端计算 SHA-256，2GB 单文件上限 | [FileController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/file/FileController.java#L69-L98)、[QuireldController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/quireld/QuireldController.java#L38-L57) |
| quireld 暂存 | 上传只写版本目录与该版本清单，不碰 `latest.json` / 稳定别名 | [QuireldService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/quireld/QuireldService.java#L116-L154) |
| quireld 原子发布 | 稳定别名 → 两份 `SHA256SUMS` → **最后**原子写 `latest.json` | [QuireldService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/quireld/QuireldService.java#L174-L204)、[QuireldService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/quireld/QuireldService.java#L357-L371) |
| 客户端发布记录 | 按版本号 upsert，导出 `release.json` 快照；`/release.json` 直出且 `no-store` | [ReleaseService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/ReleaseService.java#L171-L183)、[ReleaseController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/ReleaseController.java#L44-L55) |
| 发布前硬校验 | 版本规范、通道合法、`notes` 非空、至少一个平台绑定真实存在且已登记 SHA-256 | [ReleaseService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/ReleaseService.java#L241-L283) |
| 运行时状态 | `/release.json` 与后台列表回带平台 `available` / `sha256`（不落库） | [Platform.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/Platform.java#L15-L27)、[ReleaseService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/ReleaseService.java#L117-L138) |
| 双域名一致 | 两个域名指向同一实例、同一 `data/downloads`；产物只需上传一份 | [WebConfig.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/config/WebConfig.java#L38-L58) |
| 回滚 | 客户端：版本历史 / 软删 / 恢复 / rollback；quireld：重新发布旧版本（旧目录保留） | [ReleaseController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/ReleaseController.java#L126-L160)、[QuireldService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/quireld/QuireldService.java#L206-L231) |
| 制品保留 | quireld 版本目录不自动清理；当前生效版本禁删 | [QuireldService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/quireld/QuireldService.java#L206-L231) |
| 机器凭据 | RELEASE-CONTRACT v1 要求新增最小权限 Bearer token（当前只有管理员密码 Cookie） | 本方案新增 |

**quirel_web 明确不做：**

- 不构建客户端或 quireld（只接收成品）。
- 不决定或校验 tag 语义（只接收版本号字符串）。
- 不生成发布说明（只校验非空）。
- 不创建 Git tag、不合并分支、不触发 Quirel 构建。
- 不自动回滚线上版本；不替用户选通道。
- 不承担“同名同版本不同内容”的静默覆盖责任：接收端目前允许覆盖，**不可变由调用方在预检阶段保证**（阶段 2 建议服务端加护栏）。

### 1.3 “以为对方负责”清单（必须一次说清）

| 事项 | 唯一负责方 | 另一方做什么 |
|---|---|---|
| 发布说明内容 | Quirel 生成草稿 + 人工定稿 | quirel_web 只校验非空 |
| `latest.json` 写入顺序 | quirel_web `publish` 内部 | Quirel 不得绕过 API 直传静态文件 |
| 双域名可用性 | quirel_web（同一实例） | Quirel 发布后逐域回读校验 |
| 历史版本保留 | quirel_web | Quirel 不得请求删除当前版本 |
| 同版本不可变 | Quirel 预检拒绝 | quirel_web 记录 `contentChanged` 供审计 |
| stable/beta 选择 | 用户在审批时确认 | CI 透传，不自行决策 |
| 生产审批 | 用户 | CI 只等待 `production` 环境批准 |
| 版本号与 tag | 用户在 main 执行 release | CI 只消费 tag |

---

## 2. 接口契约（RELEASE-CONTRACT v1）

### 2.1 版本号与 tag 命名

| 对象 | 规则 | 示例 | 备注 |
|---|---|---|---|
| Quirel 客户端 / quireld / protocol | lockstep 语义化版本，可带预发布后缀 | `0.3.4`、`0.4.0-beta.1` | 三组件同版本；0.x 破坏性升次版本 |
| Quirel git tag | 轻量 tag `v{version}`，指向 `chore(release)` 提交 | `v0.3.4` | 永不复用；打在构建前 |
| quireld 版本目录 / 文件名 | **不带 v** 的 `{version}` | `0.3.4/quireld-0.3.4-release-linux-amd64.tar.gz` | [QuireldService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/quireld/QuireldService.java#L241-L243) |
| 客户端 Release 记录 | 服务端规范化为 **带 v** | `v0.3.4` | [Versions.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/Versions.java#L22-L41) |
| `latest.json.tag` | 带 v | `"v0.3.4"` | `{version:"0.3.4", tag:"v0.3.4", updated:"YYYY-MM-DD"}` |
| quirel_web 自身 tag | `web-v{X.Y.Z}`，仅用于本仓库部署 | `web-v1.0.7` | 与客户端 tag 命名空间隔离，禁止混用 |
| quirel_web 自身版本 | 待用户定（建议 `1.0.<build>` 或 `<yyyymmdd>-<短sha>`），由脚本注入，禁手改 | — | 现行 pom 固定 `1.0.0`，[pom.xml](file:///E:/MyWork/Web/quirel_web/pom.xml#L15-L16) |

约束：

- `v` 前缀方向相反：tag/客户端记录带 v，quireld 目录与文件名不带 v。CI 必须显式转换，不得字符串直传。
- 版本号只能来自 tag；CI 不接受命令行传入的版本参数（沿用零参数原则）。
- 预发布版本（`-beta.N`）必须走 `beta` 通道；稳定版走 `stable`。

### 2.2 制品命名与目录布局

quireld（`data/downloads/quireld/`，与客户端取包契约一一对应）：

```
downloads/quireld/latest.json                                      # 更新检查开关（最后写）
downloads/quireld/install-quireld.sh                               # 引导脚本
downloads/quireld/SHA256SUMS                                       # 当前版本条目 + 稳定别名条目
downloads/quireld/quireld-linux-amd64.tar.gz                       # 稳定别名（引导脚本用）
downloads/quireld/quireld-linux-arm64.tar.gz
downloads/quireld/{version}/quireld-{version}-release-linux-amd64.tar.gz
downloads/quireld/{version}/quireld-{version}-release-linux-arm64.tar.gz
downloads/quireld/{version}/SHA256SUMS                             # 客户端按版本精确匹配
```

客户端安装包（`data/downloads/`）：

- 文件名由 Tauri NSIS 构建产生，必须**包含版本号**；以实际产物名为准，不在此硬编码。
- 落地路径：`/downloads/<文件名>`；发布记录中的 `platforms.<key>.url` 使用站内相对路径（如 `/downloads/quirel_0.3.4_x64-setup.exe`）或外链。

清单格式（强制）：

```
<64位小写hex>␠␠<文件名>
```

- 两个空格分隔，客户端 `provision.rs` 严格解析；不得使用 `*` 前缀或单空格。
- `SHA256SUMS` 至少覆盖：两个版本 tarball + 两个稳定别名。

### 2.3 端点总表

| 方法 | 路径 | 用途 | 幂等 | 契约归属 |
|---|---|---|---|---|
| POST | `/api/login` | 密码换 Cookie（过渡方案） | 是 | quirel_web |
| POST | `/api/upload?kind=installer&name={file}` | 上传客户端安装包 | 是（同名覆盖） | quirel_web |
| PUT | `/api/release` | 直接发布客户端 Release | 是（同版本 upsert） | quirel_web |
| POST | `/api/release/publish` | 发布草稿内容 | 是 | quirel_web |
| GET | `/release.json?channel=stable\|beta` | 公开回读当前版本 | 是 | quirel_web |
| GET | `/api/release/history` | 历史版本（回滚依据） | 是 | quirel_web |
| POST | `/api/release/rollback` | 回滚（**仅人工**） | 否 | quirel_web |
| POST | `/api/admin/quireld/upload?version={v}&arch={amd64\|arm64}` | 上传 quireld（暂存） | 是（同名覆盖） | quirel_web |
| POST | `/api/admin/quireld/script` | 上传引导脚本 | 是（覆盖） | quirel_web |
| POST | `/api/admin/quireld/publish?version={v}` | 原子发布 quireld | 是（可重放） | quirel_web |
| GET | `/api/admin/quireld` | 总览/回读暂存状态 | 是 | quirel_web |
| DELETE | `/api/admin/quireld/{version}` | 删除非当前版本（**仅人工**） | 否 | quirel_web |

主机：

- 主源 `https://quireld.cn`，备源 `https://quireld.icu`；同一实例，同一数据目录。
- 应用只监听 `127.0.0.1:8080`，公网经 Nginx；**外部 CI 不能直连 8080**，部署健康检查必须在服务器本机执行。

### 2.4 鉴权：现状与目标

**现状（v1 兼容基线）**

- 唯一凭据：管理员密码 → `POST /api/login {"password":"..."}` → `Set-Cookie: quirel_admin=...; HttpOnly; SameSite=Lax; Max-Age=604800`（7 天）。
- 除 `/api/login`、`/api/check`、`/api/logout`、`/api/health` 外，`/api/**` 一律要求登录，未登录 `401 {"ok":false,"error":"未授权"}`。
- 依据：[AuthController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/auth/AuthController.java#L23-L36)、[AuthService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/auth/AuthService.java#L293-L305)、[AuthInterceptor.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/auth/AuthInterceptor.java#L30-L40)、[WebConfig.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/config/WebConfig.java#L60-L66)。

**目标（RELEASE-CONTRACT v1 要求，阶段 0/1 落地）**

- Header：`Authorization: Bearer <token>`。
- 判定顺序：Bearer token 优先于 Cookie；token 无效直接 401，不回退 Cookie。
- Token 只存**哈希**（SHA-256），不落明文；服务端通过环境变量注入，例如：
  `QUIREL_WEB_RELEASE_TOKENS='ci-release=<sha256hex>:artifact:write,release:publish,quireld:publish'`
- 令牌元数据（建议最小集）：`name`、`sha256`、`scopes`、`createdAt`、`expiresAt`（可选）、`lastUsedAt`（日志即可）。
- 作用域：

| Scope | 允许 | 明确禁止 |
|---|---|---|
| `artifact:write` | `/api/upload`、`/api/admin/quireld/upload`、`/api/admin/quireld/script` | 删除文件 |
| `release:publish` | `PUT /api/release`、`POST /api/release/publish` | rollback、restore、purge、delete |
| `quireld:publish` | `POST /api/admin/quireld/publish` | `DELETE /api/admin/quireld/{version}` |
| `release:read`（可选） | `GET /api/admin/quireld`、`GET /api/release/history`、`GET /api/admin/files` | 所有写操作 |
| （不授予） | — | `POST /api/admin/password`、`DELETE /api/file`、`DELETE /api/release/{id}`、`/purge`、`/restore`、`/rollback` |

- 审计：日志记录 `tokenName + method + path + 结果`，禁止记录 token 明文。
- 轮换：支持两个 token 并存；轮换窗口内新 token 生效后撤销旧 token。
- 越权返回 `403 {"ok":false,"error":"权限不足"}`；未认证仍是 `401`。
- 代码落点：`AuthInterceptor` 增加 Bearer 分支与 scope 判定；`WebConfig` 不变；新增配置读取（`@Value` 或 `ServerConfig`）。

**过渡期规则**：若阶段 0 未完成 token，CI 只能用管理员密码 Cookie，且必须在方案中标注为临时措施；不得长期保留。

### 2.5 请求 / 响应结构

#### 2.5.1 上传客户端安装包

```
POST /api/upload?kind=installer&name={URL 编码后的文件名}
Content-Type: application/octet-stream
Body: 原始二进制流
```

成功 `200`：

```json
{
  "ok": true,
  "url": "/downloads/quirel_0.3.4_x64-setup.exe",
  "name": "quirel_0.3.4_x64-setup.exe",
  "size": 12345678,
  "sha256": "…64hex…",
  "overwritten": true,
  "contentChanged": true
}
```

- `overwritten` / `contentChanged` 仅在覆盖同名文件时出现；`contentChanged=true` 意味着“同版本内容变了”，CI 必须视为**失败候选**（违反不可变），除非用户明确批准。
- 参数只从查询串解析；不要发 `multipart/form-data`。

#### 2.5.2 发布客户端 Release

```
PUT /api/release
Content-Type: application/json
```

请求体（只列契约字段；`screenshot` / `screenshots` / `featureImage` 会被忽略）：

```json
{
  "version": "v0.3.4",
  "updated": "2026-11-24",
  "githubUrl": "https://github.com/Victor-C0905/Remote_Desktop/releases/tag/v0.3.4",
  "notes": "## 新增\n- …\n\n## 修复\n- …",
  "channel": "stable",
  "platforms": {
    "windows": {
      "label": "Windows 10/11 (x64)",
      "req": "Windows 10 及以上",
      "size": "11.8 MB",
      "url": "/downloads/quirel_0.3.4_x64-setup.exe"
    }
  }
}
```

- `version` 可带或不带 `v`，服务端统一规范化成 `vX.Y.Z[-suffix]`。
- `updated` 建议**留空**由服务端签发当天日期；若传入，必须是 `YYYY-MM-DD`。
- `channel` 缺省 `stable`；预发布必须显式 `beta`。
- `notes` 纯文本，换行保留；非空。
- 至少一个平台的 `url` 非空且不是 `#`；站内文件必须真实存在且已登记 SHA-256。

成功 `200`：`{"ok":true}`。

失败 `400`：

```json
{
  "ok": false,
  "error": "发布前检查未通过",
  "issues": [
    "发布说明不能为空：写清本版新增、修复与升级注意事项",
    "Windows 的安装包缺少 SHA-256 校验和：quirel_0.3.4_x64-setup.exe，请重新上传"
  ]
}
```

#### 2.5.3 公开回读

```
GET /release.json?channel=stable
```

- `200` 直接返回 `Release` 对象（未包裹 `ok`），`Cache-Control: no-store`。
- 从未发布该通道 → `404`。
- 运行时字段 `platforms.*.available`（`true/false/null`）与 `sha256` 可能出现；**回读后若要重新 PUT，必须先剥掉这两个字段**（服务端会清，但契约要求调用方不依赖）。

#### 2.5.4 quireld 上传（暂存）

```
POST /api/admin/quireld/upload?version={不带v}&arch={amd64|arm64}
Body: 原始 tar.gz 流
```

成功 `200`：

```json
{
  "ok": true,
  "version": "0.3.4",
  "arch": "amd64",
  "name": "quireld-0.3.4-release-linux-amd64.tar.gz",
  "size": 12345678,
  "sha256": "…64hex…",
  "current": false,
  "overwritten": true,
  "contentChanged": false
}
```

- `current=true` 表示上传的是当前生效版本（同版本重传）；`contentChanged=true` 违反不可变，CI 必须失败。

#### 2.5.5 quireld 引导脚本

```
POST /api/admin/quireld/script
Body: 原始 shell 文本（≤1MB）
```

成功：`{"ok":true,"name":"install-quireld.sh","size":…,"sha256":"…"}`。

#### 2.5.6 quireld 发布

```
POST /api/admin/quireld/publish?version={不带v}
```

成功 `200`（实际返回 `latest.json` 内容 + `ok`）：

```json
{"ok": true, "version": "0.3.4", "tag": "v0.3.4", "updated": "2026-11-24"}
```

- 若该版本目录无任何架构产物 → `400 {"ok":false,"error":"该版本没有任何架构产物，先上传 amd64/arm64 的 tar.gz"}`。
- 发布顺序由服务端保证：别名 → 两份清单 → 最后 `latest.json`；调用方不得在 publish 之后再手写这些文件。

#### 2.5.7 总览（回读）

```
GET /api/admin/quireld
```

返回 `{current, versions:[{version, staged, updated, arches:[{arch,name,size,sha256}]}], script, aliases}`；未包裹 `ok`。

CI 用它做发布前/后断言：`current.version`、`versions[].arches[].sha256`、`aliases.*.matches`。

### 2.6 幂等性与重试语义

| 操作 | 幂等性 | 重试规则 |
|---|---|---|
| 上传安装包 / quireld / 脚本 | 按文件名覆盖，内容相同则结果相同 | 网络错误、`5xx`、`429` 可重试（指数退避，最多 3 次）；重试后必须比对返回 `sha256` 与本地一致 |
| quireld publish | 可重放：重写别名/清单/`latest.json` | 可重试；重试后 `GET /api/admin/quireld` 校验 |
| 客户端 `PUT /api/release` | 同版本 upsert；同 body 重放安全 | 可重试；但**整体替换**语义意味着并发人工编辑会被覆盖，阶段 2 改为读—改—写或专用端点 |
| 删除/回滚/purge | 非幂等 | CI 不调用；仅人工 |

统一规则：

1. 写后必读：每个写操作后立刻调用对应 GET 断言状态。
2. `4xx` 不重试；`200` 但 `ok:false` 不重试（见 2.7 特例）。
3. 无幂等键（Idempotency-Key）机制；重试安全依赖上表的“同名覆盖 + 同版本 upsert”。
4. 上传顺序固定：先产物、后清单、最后 `latest.json`（由 publish 封装）；客户端先安装包、后 Release。

### 2.7 失败返回与错误矩阵

| HTTP | 响应体 | 触发 | CI 动作 |
|---|---|---|---|
| `401` | `{"ok":false,"error":"未授权"}` | 未登录 / token 无效 | 立即失败，检查 secret |
| `403`（v1 新增） | `{"ok":false,"error":"权限不足"}` | token scope 不足 | 立即失败，检查 scope |
| `400` | `{"ok":false,"error":"<原因>"}` | 参数/版本/架构非法 | 立即失败，不重试 |
| `400` | `{"ok":false,"error":"发布前检查未通过","issues":[…]}` | Release 校验失败 | 失败并输出 issues |
| `400` | `{"ok":false,"error":"不能删除当前生效版本 …"}` | quireld 删除护栏 | 人工介入 |
| `413` | `{"ok":false,"error":"文件超过大小限制（2GB）"}` | 流式超限 | 失败 |
| `507` | `{"ok":false,"error":"磁盘剩余空间不足…"}` | 磁盘保护 | 失败并告警 |
| `500` | `{"ok":false,"error":"服务器内部错误，请稍后重试"}` | 服务端异常 | 可重试 1 次，仍失败则告警 |

**契约特例（必须知道）**：`POST /api/upload` 对“文件名为空”和“Content-Length 超限”返回 **HTTP 200 + `ok:false`**，不是 4xx（[FileController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/file/FileController.java#L69-L98)）。CI 必须**同时检查 HTTP 状态与 `ok` 字段**，不能只看状态码。

### 2.8 时序与顺序约束

```
[用户]  dev→main 合并（人工）
   │
   ├─ npm run release X.Y.Z（main，人工/经用户同意代跑）→ 提交 + tag vX.Y.Z
   ├─ 推送 main 与 tag（人工）
   ▼
[CI] preflight：tag 格式 / 4 版本文件==tag / tag 提交属于 main / verify / wire_compat / tag 未复用
   │
   ├─ build-client（Windows）→ NSIS + sha256
   └─ build-quireld（Linux）  → amd64/arm64 tar.gz + SHA256SUMS
   ▼
[CI] 暂存上传（不切换线上）
   ├─ quireld amd64 → arm64 → script
   └─ 客户端安装包
   ▼
[CI] 暂存回读断言（GET /api/admin/quireld、GET /api/admin/files）
   ▼
[用户] production 环境审批（一次）
   ▼
[CI] 发布
   ├─ POST /api/admin/quireld/publish?version=X.Y.Z   （内部：别名→清单→latest.json）
   └─ PUT /api/release（version=vX.Y.Z）
   ▼
[CI] 发布后校验
   ├─ quireld.cn / quireld.icu 的 {version}/SHA256SUMS 与 tar.gz SHA 一致
   ├─ 两个域名的 latest.json.version == X.Y.Z
   └─ /release.json?channel=… 的 version 与平台 available/sha256 正确
```

硬顺序：

1. 没有 tag 不构建，没有构建不暂存。
2. `latest.json` 永远最后；`/release.json` 在安装包登记之后。
3. 审批前不执行任何“生效”写操作（publish / PUT release）。
4. 同一时刻只允许一个发布源；人工后台上传与 CI 发布不得并发。

### 2.9 发布说明（notes）归属

- **生成方**：Quirel CI。来源 `git log --no-merges v上一版..v当前`，按 `feat/fix/perf/refactor/docs/chore` 分组，输出纯文本。
- **定稿方**：审批人（可在 Environment 审批前修改 workflow 输入，或改为“生成草稿 → 人工提交到发布”）。
- **校验方**：quirel_web，仅要求非空。
- 格式契约：UTF-8 纯文本，保留换行，1–8192 字符；不含 HTML 标签；不包含未公开的安全细节。
- 时间字段：`updated` 留空由服务端签发，避免 CI 时区（UTC）与服务器时区（Asia/Shanghai）跨日不一致。

### 2.10 契约版本与变更流程

- 契约标识：`RELEASE-CONTRACT v1`（本文件标题与本节为准）。
- 任何破坏性变更（端点、字段、错误码、命名、顺序、scope）必须：
  1. 在本文件升版并记录变更；
  2. 同步 quirel_web 副本；
  3. 在两端 PR 描述中互相引用；
  4. 更新 4.4 的“契约文件清单”。
- 兼容性窗口：新增字段/端点视为兼容；删除/重命名/改语义视为破坏性，需双端同版本窗口。

---

## 3. 分阶段计划

标注：`[Q]` = Quirel，`[W]` = quirel_web。规模 S/M/L 为改动量相对估计，不含等待与联调时间。

### 阶段 0：清障与契约冻结（不做自动发布）

| ID | 归属 | 改动 | 验收标准 | 依赖 |
|---|---|---|---|---|
| P0-1 | W | 提交并部署 quireld 暂存/发布模块与 `Versions.java`（当前未跟踪） | 线上 `GET /api/admin/quireld` 返回 200；测试版本可上传双架构并 publish；版本目录出现 `latest.json` 且顺序正确 | 无 |
| P0-2 | Q | 修复 `publish-quireld.mjs` 的 semver 正则（现为 `/^d+.d+.d+…/`，缺反斜杠，任何版本都会失败），或明确废弃其主链路上传职能，仅保留 GitHub 兜底 | `node scripts/publish-quireld.mjs --dry-run` 在有效 tag 下通过；非法版本被拒绝 | 无 |
| P0-3 | Q | 收敛 main 与 dev 的脚本漂移：`verify.mjs`、`publish-quireld.mjs`、`package.json` scripts 在 main 上缺失或版本不同 | `git diff main..dev -- scripts package.json` 为空或无未声明的差异；main 上 `npm run verify` 可执行 | 无 |
| P0-4 | W | 修 RUNBOOK 升级步骤：`sudo rm -rf quirel-backend*.jar` 会删光旧 jar，使“翻软链回滚”失效；改为白名单删除非 jar 文件 + 保留最近 N 个 jar | 预发环境模拟升级两次后，`ln -sfn` 回滚到上一版并健康检查通过 | 无 |
| P0-5 | W | 新增最小权限 Bearer token + scope 判定（2.4）；同时把 admin 密码从 CI 方案中移除 | token 可上传/发布；调用改密/删除/回滚返回 403；无 token 仍 401 | P0-1 |
| P0-6 | Q+W | 契约 v1 冻结：本文件定稿 + quirel_web 副本落盘 + 双方确认 notes 格式与域名清单 | 两份文档存在且内容对应；责任人确认 | 无 |
| P0-7 | W | 统一 JDK 前置：`install.sh` 现仅提示 21+，`pom.xml` 要求 25；增加 major≥25 硬校验或对齐文档 | 服务器 Java 版本低于 25 时安装脚本明确失败 | 无 |
| P0-8 | W | 明确部署通道：SSH 用户、受限 sudo 命令白名单、备份目录、回滚脚本位置（仅文档与一次性运维配置） | 部署机可用受限账号执行一次 dry-run | 无 |

阶段 0 完成定义：

- quirel_web 的发布 API 是**已提交、已部署、可回读**的事实；
- Quirel 的两个脚本缺陷不再阻塞；
- RUNBOOK 的回滚承诺与实际脚本一致；
- 机器凭据可用且权限最小；
- 契约 v1 双仓可查。

### 阶段 1：半自动（tag 触发构建，人工发布）

| ID | 归属 | 改动 | 验收标准 | 依赖 |
|---|---|---|---|---|
| P1-1 | Q | 新增 tag 触发 workflow 的 `preflight` job | 非法 tag/版本不一致/tag 不在 main/已存在 tag 全部失败；合法 tag 通过并输出版本 | P0-3 |
| P1-2 | Q | `build-client`（windows-latest）：Node + Rust MSVC + `npm ci` + `npm run tauri build`，产出 NSIS + sha256 | 安装包可安装启动；sha256 与 Actions 摘要一致 | P1-1 |
| P1-3 | Q | `build-quireld`（ubuntu-latest）：`rustup target add aarch64-unknown-linux-gnu` + `apt install gcc-aarch64-linux-gnu` + `build.sh release amd64/arm64` | 两个 tar.gz 架构正确（`file`/`uname -m` 冒烟）；`SHA256SUMS` 两空格格式 | P1-1 |
| P1-4 | Q | 产物上传为 Actions Artifacts（含 `SHA256SUMS` 与 `install-quireld.sh`），附构建元数据（tag、commit、sha256） | 一次性下载全部产物；与本地构建可复现对比 | P1-2, P1-3 |
| P1-5 | Q | `workflow_dispatch` 仅用于重跑/候选构建，强制指定已存在 tag，不发布 | 手动触发不产生任何线上写操作 | P1-4 |

阶段 1 完成定义：用户仍手工上传到 quirel_web，但不再需要本地 Windows + WSL 双环境构建；CI 产物可直接用于现有后台流程。

风险：tag 先于构建，构建失败会留下“有 tag 无产物”；无签名，SmartScreen 仍提示；仍非“一键”。

### 阶段 2：自动发布（tag + 一次审批）

| ID | 归属 | 改动 | 验收标准 | 依赖 |
|---|---|---|---|---|
| P2-1 | Q | 发布说明生成器：`git log` → 分组纯文本，输出到 workflow artifact/审批输入 | notes 非空、可读、无敏感信息；quirel_web 校验通过 | P0-6 |
| P2-2 | Q | `publish-quireld` job：token 登录 → 上传 amd64/arm64/script → `GET /api/admin/quireld` 断言 sha → `publish` → 双域名回读 | 两域名 `latest.json` 与 `{version}/SHA256SUMS` 一致；失败不切换线上 | P0-1, P0-5, P1-3 |
| P2-3 | Q | `publish-client` job：上传安装包 → 读取现有 `/release.json` → 合并 version/notes/platforms（剥掉运行时字段）→ `PUT /api/release` → 回读断言 | `/release.json` 版本、平台、sha256 正确；人工维护的其他平台/链接不丢失 | P0-5, P1-2, P2-1 |
| P2-4 | W | 为客户端发布提供“读—改—写”安全路径：现 `PUT /api/release` 是整体替换，建议新增专用端点（仅更新 version/notes/指定平台）或文档化合并规则 | 并发人工编辑不被静默覆盖；接口测试覆盖 | P2-3 |
| P2-5 | Q+W | `production` 环境保护 + Required reviewers；失败告警；回滚 runbook；发布后校验脚本 | 未审批不执行任何生效写；失败可一键回滚 | P2-2, P2-3 |
| P2-6 | W | （独立支线）quirel_web 自身部署自动化：`web-v*` tag → 构建/测试/打包 → SSH/受限 sudo → 备份/换链/健康检查/失败回滚 | 预发环境一条命令上线；健康检查失败自动翻回旧 jar | P0-4, P0-7, P0-8 |

阶段 2 完成定义：用户执行一次 `npm run release` 并推 tag，CI 完成构建与暂存，审批一次后自动上线，双域名校验通过；任何失败不改变线上状态。

依赖顺序：

```
P0-1 ─→ P0-5 ─→ P2-2
P0-3 ─→ P1-1 ─→ P1-2 ─┐
        └────→ P1-3 ─┴─→ P1-4 ─→ P2-3
P0-6 ─→ P2-1 ─────────────→ P2-3
P0-4/P0-7/P0-8 ─→ P2-6
P2-2/P2-3 ─→ P2-5
```

### 阶段 3（不建议默认实施）

一个按钮完成 bump + tag + 构建 + 发布。与 AGENTS.md §4 直接冲突（版本只能由用户在 main 上跑 `npm run release`，push 由用户执行）。若坚持，必须先由用户修改治理条款，并接受 CI 持有 main 写权限与 tag 权限；客户端自动更新仍需单独的 Tauri updater + 签名工程。

---

## 4. 落地位置与文档记录

### 4.1 Quirel 仓库（权威版本）

- 本文件：`docs/superpowers/plans/2026-11-24-cross-project-release-automation-plan.md`（符合 AGENTS.md §8 的 AI 产物白名单）。
- 不建议再复制到 `docs/` 根或仓库根，避免多份真相。

### 4.2 quirel_web 仓库（派生副本）

建议路径（二选一，推荐前者）：

1. `docs/release-contract.md`：单一契约副本，README 与 AGENTS.md 各加一行指向它。
2. `docs/superpowers/plans/2026-11-24-cross-project-release-automation-plan.md`：与 Quirel 保持同构路径，便于对照。

副本开头必须加：

```
> 本文件是 Quirel 仓库 RELEASE-CONTRACT v1 的本地化副本，权威版本位于
> Quirel 仓库 docs/superpowers/plans/2026-11-24-cross-project-release-automation-plan.md。
> 修改契约请先在权威版本提交，再同步本文件。
```

### 4.3 两个仓库的文档记录建议（只给措辞，不改 AGENTS.md）

Quirel `AGENTS.md` §4 末尾建议增加（需用户决定是否采纳）：

> - 发布自动化（CI）只消费用户在 main 上产生的 `vX.Y.Z` tag：CI 不得创建版本号、打 tag、squash 合并、合并 dev→main 或推送 main/dev；生产发布必须经 `production` 环境人工审批。跨仓库发布契约见 `docs/superpowers/plans/2026-11-24-cross-project-release-automation-plan.md`（RELEASE-CONTRACT v1）。

quirel_web `AGENTS.md` 建议增加：

> - 客户端制品发布契约见 `docs/release-contract.md`（源自 Quirel 仓库 RELEASE-CONTRACT v1）。发布 API 的机器凭据只能使用 `Authorization: Bearer` 最小权限 token，禁止在 CI 中使用管理员密码；`latest.json` 只能由 `QuireldService.publish` 写入；生产发布须人工审批，回滚与删除仅限人工会话。

注意：以上仅为建议措辞。本次未修改任何 AGENTS.md。

### 4.4 契约版本号与需同步维护的文件清单

契约标识：`RELEASE-CONTRACT v1`（变更时升为 v2，并在本节记录差异）。

Quirel：

- `AGENTS.md` §4（版本与发版）、§5 红线第 3 行
- `scripts/release.mjs`（版本、tag、提交顺序）
- `scripts/verify.mjs`（lockstep）
- `scripts/publish-quireld.mjs`（旧上传链路，可能废弃）
- `agent/build.sh`（产物命名、架构、清单格式）
- `agent/deploy/install-quireld.sh`（引导脚本）
- `src-tauri/src/provision.rs`（取包 URL、SHA 校验、版本精确匹配）
- `src-tauri/src/agent_update.rs`（`latest.json` 读取与版本比较）
- `src-tauri/tauri.conf.json`（targets、无 version 字段）
- `package.json`（`release` / `build:dev` / `verify` / Tauri CLI）

quirel_web：

- `src/main/java/com/quirel/quireld/QuireldController.java`
- `src/main/java/com/quirel/quireld/QuireldService.java`
- `src/main/java/com/quirel/release/ReleaseController.java`
- `src/main/java/com/quirel/release/ReleaseService.java`
- `src/main/java/com/quirel/release/Release.java`、`Platform.java`、`Versions.java`
- `src/main/java/com/quirel/file/FileController.java`
- `src/main/java/com/quirel/auth/AuthInterceptor.java`、`AuthService.java`
- `src/main/java/com/quirel/config/WebConfig.java`
- `src/main/java/com/quirel/web/GlobalExceptionHandler.java`
- `deploy/install.sh`、`deploy/pack-deploy.sh`、`deploy/RUNBOOK.md`
- `pom.xml`

同步要求：任一文件改动若影响端点、字段、错误码、命名、顺序、scope，必须更新本契约并升版。

### 4.5 对侧副本的本地化替换清单

将本文件复制到 quirel_web 时，逐项替换：

| 类别 | 本文件（Quirel 权威版） | quirel_web 副本应改为 |
|---|---|---|
| 仓库路径 | `E:\MyWork\Quirel` | `E:\MyWork\Web\quirel_web` |
| 文档相对链接 | `docs/superpowers/...`、`scripts/...`、`src-tauri/...`、`agent/...` | `../src/main/java/...`、`../deploy/...`、`../pom.xml`；Quirel 文件改为“远端仓库”说明或 GitHub/Gitee 链接 |
| 仓库名 | GitHub `Victor-C0905/Remote_Desktop` | Gitee `qiang-wei-chen/quirel_web` |
| 远端 | GitHub + Gitee 双远端 | 仅 Gitee；CI 选型改为 Gitee Go / 自建 runner |
| CI runner | `windows-latest` / `ubuntu-latest` | Gitee Go 的 runner 标签或自建 Linux runner；Windows 构建仍指向 Quirel 仓库 |
| 构建命令 | `npm ci` / `npm run tauri build` / `bash agent/build.sh` | `./mvnw -B clean package` / `bash deploy/pack-deploy.sh`（Quirel 命令标注为“客户端仓库执行”） |
| 版本入口 | `npm run release <版本号>` | 本仓库无此脚本；自身版本由 `mvnw`/脚本注入，客户端版本只接收 |
| 分支模型 | main/dev/feature/fix/archive | 按本仓库现状（`release/*`、`archive-*`）描述，或明确改为 main/dev 后另议 |
| tag 示例 | `v0.3.4`（客户端） | 保留客户端示例；新增 `web-v1.0.7` 作为本仓库部署示例 |
| JDK | 无（Rust/Node） | JDK 25 + Maven Wrapper |
| 文档引用 | `AGENTS.md` §4/§5/§8 | 本仓库 `AGENTS.md`（环境/命令/目录章节），不引用 Quirel 的 §4 |
| 域名 | quireld.cn / quireld.icu | 不变（由本仓库实例提供），但注明“本仓库负责实际伺服” |
| 凭据 | GitHub Secrets | Gitee Go 变量 / 自建 runner 环境变量；名称保持一致（`QUIREL_WEB_RELEASE_TOKEN` 等） |
| 审阅人 | Quirel 维护者 | quirel_web 维护者 + Quirel 发布负责人（双签） |

副本保留的公共内容：端点表、字段结构、错误矩阵、顺序约束、不可变要求、红线（仅调整“执行方”名称）。

---

## 5. 边界与红线

### 5.1 必须保留人工

| 动作 | 依据 | 说明 |
|---|---|---|
| 决定版本号与发版时机 | AGENTS.md §4（L48、L51） | 产品决策 |
| 在 main 上执行 `npm run release` | AGENTS.md §4（L48） | 唯一版本写入口 |
| squash 合并任务分支到 dev | AGENTS.md §3（L40-L42） | 合并策略与历史由用户负责 |
| dev 合入 main | AGENTS.md §4（L49） | 发布基线由用户确认 |
| 推送 main/dev/tag | AGENTS.md §3（L40-L42） | 远端写操作 |
| 生产发布审批 | 本方案新增 | 对外动作，需人工 gate |
| 回滚/删除版本 | AGENTS.md + quirel_web 护栏 | 影响线上，仅人工 |
| stable/beta 通道切换 | 产品决策 | CI 只透传 |
| 客户端自动更新策略 | 产品决策 | 涉及签名、兼容、回滚 |

### 5.2 自动化只消费 tag，不生产 tag

允许自动：预检、构建、SHA 计算、暂存上传、发布说明草稿、审批后 publish、发布后双域名校验、失败告警。

禁止自动：bump 版本、创建/删除 tag、squash、合并分支、推送、审批、回滚、删除版本、修改管理员密码。

### 5.3 越界后果（写清楚，避免“就这一次”）

| 越界行为 | 直接后果 | 修复成本 |
|---|---|---|
| CI 创建 tag | 与用户本地 tag 冲突；版本复用；客户端更新判断混乱 | 删 tag + 重新 release，可能违反“版本不复用” |
| CI 推送 main | 绕过 squash/评审；AGENTS.md §3 失效；审计断链 | 强制回退远端，需用户授权 |
| CI 自动 bump 版本 | 同一版本号多次构建，产物不可复现；lockstep 破坏 | 手工清理版本文件与 tag |
| CI 直接写 `latest.json` | 客户端在产物就绪前发现新版本，升级失败 | 回滚静态文件，客户端已受影响 |
| 在 CI 使用管理员密码 | 口令泄露面扩大；无法区分人机操作；改密即断流水线 | 轮换口令 + 重建 token |
| 自动回滚线上版本 | 用户已下载的包与官网版本错位；历史记录被改写 | 人工核对历史与文件，重建发布记录 |
| 同版本覆盖不同内容 | 不可变被破坏；用户下载到与校验值不符的包 | 重新发版，旧版本作废 |

---

## 6. 风险与回滚

### 6.1 双端回滚方式

Quirel（构建侧）：

- 版本不可回退复用：tag 一旦推送，不删除、不重打；坏包通过**新版本**修复。
- 构建产物可重跑：CI 保留 tag 对应的 workflow run 与 artifacts；必要时从同一 commit 重建。
- 客户端回滚 = 在 quirel_web 上把 Release 记录指回上一版本（人工），并把下载页指向旧安装包（旧文件仍在 `data/downloads`，前提是未被同名覆盖）。

quireld（发布侧）：

- 重新发布上一版本：`POST /api/admin/quireld/publish?version=上一版`（旧版本目录必须保留）。
- 当前生效版本禁删（服务端护栏）；删除非当前版本才允许。
- 顺序由服务端保证，回滚同样先别名/清单、最后 `latest.json`。

quirel_web（自身部署）：

- 保留最近 N 个版本 jar（P0-4 修复后）；`ln -sfn` 翻回旧 jar + restart。
- 升级前整目录备份（程序 + data）；整目录回滚用于怀疑数据损坏。
- 健康检查失败自动翻链（P2-6）。
- 数据库迁移只做加法（`ReleaseDbMigrator` 只增列）；回滚旧 jar 前确认新版本未依赖新列。

### 6.2 半途失败处置

| 失败点 | 线上状态 | 处置 |
|---|---|---|
| preflight 失败 | 未变 | 修复后重跑；不产生 tag 删除 |
| 客户端构建失败 | 未变 | 修代码/环境；tag 已存在但不发布，必要时发新补丁版本 |
| quireld 构建失败 | 未变 | 同上 |
| 暂存上传部分成功 | 未变（publish 未调用） | 重跑上传（同名覆盖幂等）；校验 sha 后再 publish |
| 暂存后审批拒绝 | 未变 | 保留暂存目录，人工决定清理或下次复用 |
| quireld publish 失败 | 可能已写别名但 `latest.json` 未变 | 客户端仍取旧版本；重放 publish 幂等修复 |
| 客户端 PUT 失败 | 未变（或旧记录仍在） | 不重试 4xx；核对 issues 后重发 |
| 发布后校验失败 | 已生效但可能不一致 | 立即人工回滚到上一版本；保留证据（响应体/日志） |

### 6.3 制品不可变的落地要求

1. 版本号/tag 永不复用（AGENTS.md §4）。
2. 同一版本 + 不同 SHA = 硬失败：CI 上传前查 `GET /api/admin/quireld` 与 `GET /api/admin/files`，发现 `contentChanged=true` 或历史 SHA 不一致即中止，需用户书面批准才可继续。
3. quireld 版本目录永久保留（至少保留所有已发布版本）；当前生效版本禁删。
4. 安装包文件名包含版本号；禁止 `latest.exe` 这类可变名作为发布记录目标。
5. 发布后校验两个域名的 SHA 与 `latest.json`/`/release.json` 一致；不一致视为发布失败。
6. 阶段 2 建议 quirel_web 对“同版本不同 SHA”增加服务端护栏（当前仅记录 `contentChanged`）。

---

## 7. 完成定义（验收总表）

阶段 0：

- [ ] quirel_web quireld 模块已提交、已部署、可回读
- [ ] `publish-quireld.mjs` 正则修复或废弃决策落地
- [ ] main/dev 脚本无未声明漂移
- [ ] RUNBOOK 升级不再删光旧 jar，回滚演练通过
- [ ] Bearer token 可用且越权返回 403
- [ ] 契约 v1 双仓可查
- [ ] JDK 25 前置明确

阶段 1：

- [ ] tag 触发 preflight + 双平台构建
- [ ] 产物含 SHA256SUMS 与引导脚本
- [ ] 手动重跑不产生线上写
- [ ] 下载产物可安装/解压，架构正确

阶段 2：

- [ ] 审批前无生效写
- [ ] quireld 与客户端发布均通过 API 完成
- [ ] 双域名/SHA/回读校验通过
- [ ] 失败注入（错误 SHA、缺 notes、403）均能阻断且不改变线上
- [ ] 回滚演练成功（quireld 回上一版、客户端记录回上一版、web jar 翻链）

---

## 8. 事实与推断

仓库中确实如此：

- 两端均有脚本/API 能力，但无 CI、无跨仓库契约、无机器凭据。
- quireld 发布为“别名 → 清单 → `latest.json` 最后”的原子顺序。
- 客户端发布强制 notes 非空、站内制品需登记 SHA-256。
- quirel_web 仅 Gitee 远端；quireld 模块当前未跟踪。
- RUNBOOK 升级步骤删除全部旧 jar，与回滚说明矛盾。
- 客户端无 Tauri updater、无签名配置。

推断（需联调确认）：

- GitHub 托管 Windows runner 可完成 Tauri NSIS 构建（构建期需下载 NSIS）。
- Gitee Go 的私有仓库 runner 能力与额度满足阶段 2。
- 服务器 JDK 实际版本 ≥25。
- 当前线上是否已部署未提交的 quireld 模块。

---

## 9. 关键证据索引

| 主题 | 文件 |
|---|---|
| 分支与发版规则 | [AGENTS.md](file:///e:/MyWork/Quirel/AGENTS.md#L33-L54) |
| 版本冻结 | [release.mjs](file:///e:/MyWork/Quirel/scripts/release.mjs) |
| lockstep 机检 | [verify.mjs](file:///e:/MyWork/Quirel/scripts/verify.mjs#L75-L97) |
| 客户端打包 | [tauri.conf.json](file:///e:/MyWork/Quirel/src-tauri/tauri.conf.json#L24-L38) |
| quireld 构建与清单 | [build.sh](file:///e:/MyWork/Quirel/agent/build.sh#L177-L186)、[build.sh](file:///e:/MyWork/Quirel/agent/build.sh#L238-L298) |
| 旧发布脚本 | [publish-quireld.mjs](file:///e:/MyWork/Quirel/scripts/publish-quireld.mjs) |
| 客户端取包契约 | [provision.rs](file:///e:/MyWork/Quirel/src-tauri/src/provision.rs#L37-L41)、[provision.rs](file:///e:/MyWork/Quirel/src-tauri/src/provision.rs#L823-L830) |
| 更新检查 | [agent_update.rs](file:///e:/MyWork/Quirel/src-tauri/src/agent_update.rs#L295-L337) |
| quireld 端点 | [QuireldController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/quireld/QuireldController.java#L32-L75) |
| quireld 暂存/发布 | [QuireldService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/quireld/QuireldService.java#L116-L204) |
| 客户端发布端点 | [ReleaseController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/ReleaseController.java#L44-L108) |
| 发布校验 | [ReleaseService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/ReleaseService.java#L241-L283) |
| 版本规范化 | [Versions.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/release/Versions.java#L15-L41)、[QuireldService.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/quireld/QuireldService.java#L254-L262) |
| 上传端点 | [FileController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/file/FileController.java#L69-L98) |
| 鉴权 | [AuthController.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/auth/AuthController.java#L23-L36)、[AuthInterceptor.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/auth/AuthInterceptor.java#L30-L40)、[WebConfig.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/config/WebConfig.java#L60-L66) |
| 错误语义 | [GlobalExceptionHandler.java](file:///E:/MyWork/Web/quirel_web/src/main/java/com/quirel/web/GlobalExceptionHandler.java#L29-L93) |
| 部署与回滚 | [RUNBOOK.md](file:///E:/MyWork/Web/quirel_web/deploy/RUNBOOK.md#L92-L133)、[install.sh](file:///E:/MyWork/Web/quirel_web/deploy/install.sh#L50-L152) |
| quirel_web 技术基线 | [pom.xml](file:///E:/MyWork/Web/quirel_web/pom.xml#L9-L21)、[pack-deploy.sh](file:///E:/MyWork/Web/quirel_web/deploy/pack-deploy.sh) |
