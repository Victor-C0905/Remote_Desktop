# 用户数据存储与清除策略

> 实测基准：Windows 11，Tauri 2（NSIS 安装）。最后核对：2026-10-25。

## 一、数据地图

### A. `%APPDATA%\com.quirel.app\`（Rust `app_data_dir`，结构化数据）

| 文件/目录 | 内容 | 敏感度 |
|---|---|---|
| `servers.bin` | 服务器列表：host/port/username/certFingerprint/状态 + **auth 凭据（私钥/密码，当前为明文 JSON）** | 🔴 高 |
| `settings.bin` | UI 设置：主题/强调色/字号/图标主题/壁纸配置/语言/浏览器选择 | 🟡 低 |
| `windows.bin` | 桌面会话恢复（窗口布局） | 🟡 低 |
| `transfers.bin` | 传输任务历史 | 🟡 低 |
| `wallpaper/` | 用户自定义壁纸缓存（单文件覆盖式） | 🟢 无 |
| `logs/` | 滚动日志（client.log.日期）——**含服务器域名/IP/用户名** | 🟠 中 |

### B. `%LOCALAPPDATA%\com.quirel.app\`（WebView2 + 内置浏览器，体积大头）

| 目录 | 内容 |
|---|---|
| `EBWebView/` | WebView2 主数据（localStorage/IndexedDB/缓存），运行时锁定 |
| `browser-profile/` | 内置浏览器用户资料（cookie/登录态/历史）——体积最大 |
| `browser-data/` | 内置浏览器运行数据 |
| `system-browser/` | 系统浏览器联动数据 |
| ~~`EBWebView.bak/`~~ | ~~WebView 升级残留死数据（2026-10-20 后无活动、无 lockfile）~~ 已于 2026-10-25 手动清除（547MB） |

### C. 其他

- **安装目录**（NSIS currentUser → `%LOCALAPPDATA%\Programs\`）：程序本体，卸载器负责删除
- **注册表**：NSIS 卸载键，卸载器负责删除
- **远程服务器端 quireld**：独立于桌面端（配置/审计日志/证书在服务器上），卸载走 `agent/deploy/uninstall.sh`（参数需与安装时一致）

## 二、卸载行为判定

`tauri.conf.json` 未设置 `bundle.windows.nsis.deleteAppDataOnUninstall`，Tauri 默认 **false**：

> **卸载只删安装目录与卸载键；A、B 两组数据全部保留（当前实测约 3.1GB）。**

手动完全清除 = 退出应用后删除上述 A、B 两个目录。

## 三、已实施的清除体系（2026-10-25）

1. **🔴→✅ 凭据安全化（成熟形态）**：SSH 私钥/密码/口令**唯一持久化形态 = OS 凭据管理器**
   (keyring crate：Windows Credential Manager / macOS Keychain / Linux keyutils，
   service=quirel-server-credentials，account=serverId)。完整防线：
   - **启动探测**：boot 时写/读/删探针验证链路；不可用立即通知（credential_probe）
   - **表单前置拦截**：凭据库不可用时，含密码/私钥的服务器表单阻止提交（不允许静默降级明文）
   - **强一致保存**：secretsSet 失败可见（通知），isKeyringReady 门控落盘剥离——
     写入确认成功前明文保留（防两头落空蒸发，曾出过事故已根治）
   - **启动装配**：keyring 凭据合并回内存 state（连接逻辑零改动）；旧明文自动迁移（成功即剥离）
   - 注意：**系统凭据库不随数据目录删除**，完全清除时须先 credential_delete_all

2. **🟠→✅ 应用内完全清除**：设置 → 关于 → 「数据与隐私」→ 红色危险按钮 → 确认对话框。
   执行序列：删除系统凭据库（按 serverId）→ 写 `%TEMP%\quirel-pending-wipe` 标记 →
   拉起新进程 → 旧进程退出 → 新进程启动早期（webview 创建前，文件无锁）删除：
   `%APPDATA%\{id}`（全部 bin/日志/壁纸缓存）、`%LOCALAPPDATA%\{id}`（WebView2/内置浏览器全部数据）、
   `%TEMP%\quirel-view`（下载残留）、`%TEMP%\quirel-test-browser-settings`，随后自动重启为初始状态。
3. **🟠→✅ 卸载清除**：`tauri.conf.json` → `bundle.windows.nsis.deleteAppDataOnUninstall: true`
   ——NSIS 卸载器删除应用数据目录。注意：卸载器不触及**系统凭据库**与 **%TEMP% 残留**，
   完全清除的权威路径是应用内入口（先删凭据再 wipe）；手动兜底路径见下。
4. **🟡→✅ WebView .bak 自动清理**：启动早期维护自动删除数据目录下 `*.bak` 残留
   （2026-10-25 手动清理过一次 547MB，此后自动化）。

## 四、残留清单（卸载后手动兜底）

NSIS 卸载 + 应用内清除双通道之后，理论上残留仅剩：

| 残留 | 位置 | 说明 |
|---|---|---|
| 系统凭据库条目 | Windows 凭据管理器（`quirel-server-credentials`） | 卸载器不清；应用内清除会删；手动：控制面板→凭据管理器→Windows 凭据中按前缀删除 |
| %TEMP% 偶发残留 | `%TEMP%\quirel-view` 等 | 应用内清除会删；OS 磁盘清理兜底 |
| WebView2 Runtime | 系统级组件 | 与其他应用共享，不属于本项目，不应卸载 |
| 远程 quireld | 服务器上 | 独立卸载：`agent/deploy/uninstall.sh`（参数对齐安装时） |
| 注册表卸载键/开始菜单 | — | NSIS 卸载器负责删除 ✓ |

**完全清除手动路径**（终极兜底）：卸载 → 删 `%APPDATA%\com.quirel.app`、`%LOCALAPPDATA%\com.quirel.app`、`%TEMP%\quirel-*` → 凭据管理器删 `quirel-server-credentials` 条目 → （远程）执行 quireld 卸载脚本。

## 五、历史记录

- 2026-10-25：手动清除 `EBWebView.bak`（547MB，验证依据：mtime 停在 10-20、无 lockfile、目录结构与现用目录同源）。
- 2026-10-25：落地凭据 keyring 化 + 应用内完全清除 + NSIS 卸载删除数据 + .bak 自动清理（feature/new_ui 分支）。
