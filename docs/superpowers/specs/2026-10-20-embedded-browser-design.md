# 内置浏览器（嵌入 Quirel 桌面）设计

日期：2026-10-20
状态：已获用户批准（设计阶段）

## 1. 背景与目标

现方案（`BrowserApp`）是"会话控制面板"模式：Rust 端起 SOCKS5 监听 → QUIC 隧道转发到远程
服务器，浏览器本身是外部独立进程（Edge/Chrome/Firefox），流量通过
`--proxy-server=socks5://127.0.0.1:{port}` 进入隧道，实现"服务器视角浏览"。

本设计将外部浏览器替换为**嵌入 Quirel 桌面 shell 的内置浏览器**（WebView2），目标：

1. 浏览器成为 Quirel 窗口系统的一员（标签条/地址栏用 Quirel 组件绘制，Mica/Adwaita
   风格，TopBar 边界约束、拖拽缩放走现有 WindowShell）；
2. "服务器视角浏览"语义完整保留（流量必须走 SOCKS5 → QUIC 隧道）；
3. 性能与资源占用不劣于外部浏览器方案（用户验收线）。

## 2. 核心技术约束（决定架构的事实）

1. **代理参数是 WebView2 Environment 级**，创建后不可修改。主窗口（Quirel UI）的
   Environment 已创建且不能走代理 → 浏览器 webview 必须拥有**独立 Environment**。
   Tauri 的 unstable multiwebview（同窗口多 webview）共享 Environment，因此不可用。
2. 唯一可行路径：直接使用 `webview2-com` crate（wry 的 Windows 后端本身就是它实现的，
   已在依赖树中）创建独立 Environment 的 Controller，将 Controller 的 HWND 作为
   Quirel 主窗口的子窗口嵌入。
3. WebView2 Runtime 可用性：Quirel 主窗口 UI 本身就是 WebView2 渲染的——应用能启动
   即证明 Runtime 存在，**无需外部浏览器降级路径**。

## 3. 性能影响结论

| 维度 | 结论 |
|---|---|
| 主 UI 渲染 | 零影响：webview 是独立子 HWND + 独立进程，与 React 渲染互不干扰 |
| QUIC 转发路径 | 零影响：数据路径不变 |
| 常驻内存 | 与独立窗口方案完全相同（两套 process 树）；每标签 +80~150MB，睡眠策略兜底 |
| 二进制体积 | ≈ 0：`webview2-com 0.38.2` 已由 wry 0.55.1 引入，同版本复用；**实现时必须锁定同版本**（`webview2-com = "0.38"` + `cargo tree` 验证），版本错位会导致 windows crate 双份编译（+5~15MB） |
| 新增唯一热点 | 桌面内窗口移动/缩放时的 bounds 同步：rAF 节流（一帧最多一次）→ `put_Bounds`（DComp 位置更新，单次微秒级）。拖 Quirel 主窗口本身时 child HWND 由 OS 原子跟随，零成本 |

## 4. 总体架构

```
Quirel 主窗口
├─ Desktop shell（React，Mica）
│   └─ 浏览器 app 窗口（WindowShell 管理的一个窗口 = BrowserApp）
│       ├─ chrome 区（React 组件）：标签条 + 地址栏 + 导航按钮
│       └─ content 区：占位 div → 坐标上报 → Rust put_Bounds
│            └─ [child HWND] WebView2 Controller
│                （独立 Environment：user-data-dir 按 server_id 分离
│                  + --proxy-server=socks5://127.0.0.1:{port}）
└─ Rust
    ├─ proxy/mod.rs（会话状态机语义不变）
    ├─ proxy/socks5.rs（不动）
    ├─ proxy/webview_host.rs（新）：webview2-com FFI，Environment/Controller 生命周期
    └─ proxy/browser_window.rs（新）：TabManager（active/hidden/sleeping）+ 事件桥
```

### 组件职责

| 组件 | 职责 |
|---|---|
| `proxy/mod.rs`（改造） | `ProxySession.browser_child: Arc<Mutex<Option<Child>>>` → `host: Arc<BrowserWindowHost>`；状态机各路径语义不变 |
| `proxy/webview_host.rs`（新） | unsafe/FFI 唯一所在：Environment 创建（user-data-dir + additional args）、Controller 创建/_bounds/可见性/焦点、销毁；业务层不直接接触 COM |
| `proxy/browser_window.rs`（新） | TabManager（标签状态机）、导航命令（`eval` 实现）、页面事件 → 前端 events 桥接、睡眠任务 |
| `src/apps/BrowserApp.tsx`（改造） | 窗口升级为浏览器本体：chrome 区 + content 占位区 |
| `src/apps/browser/`（新，前端） | `BrowserToolbar`（标签条 + 地址栏 + 导航按钮，Symbolic 图标）、布局上报 hooks |
| `proxy/browser.rs`（删除） | 外部浏览器探测/spawn/残留自愈全部退役（无外部进程，自愈场景消失） |
| 设置页"浏览器"分区（改造） | 移除浏览器选择（Edge/Chrome/Firefox/自定义），改为内置浏览器设置（睡眠超时） |

### 与 Quirel 桌面 shell 的集成

- BrowserApp 窗口由现有 WindowShell 管理：拖拽、缩放、激活置顶、TopBar 边界约束全部
  继承现有行为。
- content 区为 React 布局内一个占位 div：`ResizeObserver` + rAF 节流 →
  `browser_set_bounds(x, y, w, h)`（窗口内坐标由 Rust 换算为屏幕坐标后 `put_Bounds`）。
- z-order：窗口激活/置顶时同步 webview HWND 层级（`SetWindowPos`）。
- 焦点：点击 content 区后键盘焦点进 Chromium（与真实浏览器一致）；chrome 区点击时
  React 正常接管。

## 5. 代理正确性不变量（关键设计）

1. **两个 Environment 天然隔离**：主窗口 Environment（无代理）与浏览器 Environment
   （独立 user-data-dir + 代理参数）互不影响，主 UI 流量永远不会被导向 SOCKS5。
2. **`additional_browser_args` 是替换不是追加**：必须显式补回 wry 默认的
   `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`，仅追加
   `--proxy-server=socks5://127.0.0.1:{port}`；不得添加 `--disable-gpu` 等损害渲染
   性能的参数。
3. **端口与 Environment 绑定**：Environment 创建后代理端口不可变。现有状态机语义
   （浏览器活着 → 沿用原端口重建监听）恰好保证 Environment 存活期内端口不变。
4. **user-data-dir 固定且按 server 隔离**：`{app_cache_dir}/browser-data/{server_id}/`。
   cookie/存储跨重启持久；不同服务器的浏览环境互不可见。
5. **需实测验证**：`socks5://` scheme 下 Chromium 的远程 DNS 解析行为（预期代理端解析）。

## 6. 会话生命周期

状态机各路径语义与现方案一致，仅更换"浏览器"载体：

| 路径 | 行为 |
|---|---|
| `proxy_start_session`（用户主动打开） | 绑 SOCKS5（现有逻辑）→ 创建/聚焦 BrowserWindow + Environment（per server_id）+ 首个 tab；已有窗口则聚焦 |
| 连接断开 `suspend_session` | 仅 abort 监听（现有）；窗口/webview 保留，页面自然显示连接错误 |
| 重连后 start（陈旧会话） | 原端口重建监听（现有）；窗口活着不重建（Environment 锁定原端口） |
| 用户关闭浏览器窗口 | WindowShell 关闭流程 → `browser_close_window` → destroy 全部 webview + drop Environment；监听保留（面板可再开，与现方案一致） |
| `proxy_stop_session` / App 退出 | 关监听 + destroy webview + drop Environment；无进程可 kill |
| `browser_alive` 判定 | host 存活（BrowserWindow 开着）；`proxy_session_status` 返回结构不变 |

## 7. 标签页（对齐浏览器行为）

| 浏览器行为 | 实现方式 |
|---|---|
| 标签切换即时、页内状态保留 | webview 常驻 hidden（`put_IsVisible(false)`），Chromium 自动做 page visibility 节流；DOM/JS 堆不动，切回即原样 |
| 后台标签睡眠省内存 | 后台超时（默认 2 小时，设置可调）销毁 webview 实例，Tab 条目保留 URL/标题；点击唤醒时重建并导航（对齐 Edge sleeping tabs） |
| 关闭标签 | destroy webview |
| `target=_blank` / `window.open` | content webview 的 `on_new_window` 请求 → 同窗口新建 tab |
| 新建标签 | 标签条 "+" 按钮 |

### TabManager 状态机

```
Tab: Active ──切换──▶ Hidden ──切换──▶ Active
        │              │
        │              └──后台超时──▶ Sleeping{url, title}
        └──后台超时──▶ Sleeping{url, title}
Sleeping ──点击唤醒──▶ Active（重建 webview + navigate）
任意态 ──关闭──▶ destroyed
```

- `tabs: HashMap<tab_id, Tab>`；`Tab { webview | sleeping{url, title}, last_active }`
- 睡眠扫描：tokio interval 任务检查 `last_active`
- active webview bounds = content 区；hidden 不占布局

## 8. 前端接口面

### Tauri commands

| 命令 | 说明 |
|---|---|
| `browser_open(server_id, url?)` | 打开/聚焦浏览器窗口（内部复用 proxy_start_session） |
| `browser_navigate(server_id, url, tab_id?)` | 当前/指定 tab 导航 |
| `browser_back` / `browser_forward` / `browser_reload` | `eval("history.back()")` 等实现 |
| `browser_close_tab(server_id, tab_id)` | 关闭标签 |
| `browser_focus_tab(server_id, tab_id)` | 切换标签 |
| `browser_set_bounds(server_id, x, y, w, h)` | content 区布局上报（rAF 节流） |
| `browser_close_window(server_id)` | 关闭浏览器窗口（WindowShell 关闭流程调用） |

### Rust → 前端 events

| 事件 | 载荷 |
|---|---|
| `browser://tabs` | tab 列表变更（id、url、title、loading、sleeping、active） |
| `browser://page-state` | 单 tab 状态变化（URL/标题/loading，带 tab_id） |

### MVP 明确不做

- favicon（先用首字母圆形图标；后续可接 `ICoreWebView2_15::GetFavicon`）
- 下载管理（Chromium 默认行为兜底）
- 书签/历史（YAGNI）
- 外部浏览器 fallback（Runtime 必在，见第 2 节第 3 条）

## 9. 错误处理

| 场景 | 处理 |
|---|---|
| Environment/Controller 创建失败 | AppError（词典渲染）→ BrowserApp 面板错误态；纯防御（Runtime 理论必在） |
| 网络断开 | 页面 Chromium 自带错误页；面板复用现有 `ConnectionState` 组件展示重连状态 |
| 睡眠唤醒重载失败 | Chromium 错误页，用户刷新重试 |
| `browser_set_bounds` 时序竞争 | Rust 端校验 tab/窗口存在性，不存在则静默忽略（幂等） |

错误信息遵循项目既有规范：用户视角结果 + 可执行动作，不暴露协议术语与内部机制。

## 10. 测试与验证

### Rust 单元测试

- TabManager 状态转换：active/hidden/sleeping/唤醒重建/关闭
- `proxy_session_status` 在新 backend 下的各分支（沿用现有测试模式）

### 手动验证清单（webview 无法无头测试）

1. 代理生效性：访问 IP 查询站，出口 IP = 服务器 IP
2. cookie 持久化：登录某站 → 重启 Quirel → 登录态保留
3. 断线恢复：断网 → 页面报错 → 重连 → 刷新即恢复（窗口不重建）
4. 多 server 隔离：两个 server 各开浏览器，cookie/存储互不可见
5. 标签全流程：新建/切换（状态保留）/关闭/睡眠唤醒（超时可调短验证）
6. 桌面内拖拽/缩放：content HWND 跟随无撕裂；拖 Quirel 主窗口时原子跟随
7. `target=_blank` 链接在同窗口开新 tab
8. 主 UI 流量不受代理影响（本地资源正常加载、IPC 正常）

## 11. 风险与缓解

| 风险 | 缓解 |
|---|---|
| `webview2-com` 与 wry 版本错位 → 双份编译、体积上涨 | 锁定 `webview2-com = "0.38"`，`cargo tree -d` 验证单版本 |
| HWND 与 React 布局不同步（桌面内拖动撕裂） | rAF 节流 + `put_Bounds`（DComp 位置更新）；主窗口整体拖动由 OS 原子跟随 |
| unstable 无（本方案不依赖 Tauri unstable feature） | 直接依赖 webview2-com，绕开 Tauri webview 管理层，无 unstable 风险 |
| 焦点/快捷键路由变化 | 与真实浏览器行为一致，可接受；chrome 区保持 React 焦点 |
| WebView2 API 细节（如 `socks5://` DNS 行为） | 第 5 节验证项 + 第 10 节清单实测兜底 |

## 12. 范围外（后续迭代候选）

- favicon、下载管理、书签/历史
- 独立浏览器窗口模式（脱离 Quirel 桌面的系统窗口）
- 外部浏览器选择能力回归
