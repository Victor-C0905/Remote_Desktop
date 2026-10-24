# 内置浏览器（WebView2 嵌入 Quirel 桌面）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将外部浏览器方案替换为嵌入 Quirel 桌面的内置浏览器（WebView2 独立 Environment + SOCKS5→QUIC 代理），支持标签页（后台状态保留 + 超时睡眠）。

**Architecture:** 每个 server 会话一个专用 STA 线程持有 WebView2 Environment/Controller（user-data-dir 按 server 隔离 + `--proxy-server`），Controller 的 HWND 嵌入 Quirel 主窗口；React 端绘制 chrome 区（标签条/地址栏）并上报 content 区 bounds（rAF 节流 → `put_Bounds`）。TabManager 状态机（active/hidden/sleeping）为纯逻辑可单测。

**Tech Stack:** webview2-com 0.38（锁定，wry 0.55.1 已引入）、windows 0.61、Tauri 2 commands/events、React + zustand + i18next。

**Spec:** `docs/superpowers/specs/2026-10-20-embedded-browser-design.md`（唯一权威设计）

**Git 约定（重要）:** 本项目规则——AI 永不执行 git 提交/回退。每个任务完成时只提示用户"建议提交"，由用户自己操作。计划中不含 git commit 步骤。

**参考源码（API 权威依据，已本地验证）:**
- `C:\Users\w8067\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\wry-0.55.1\src\webview2\mod.rs`（Environment/Controller 创建、事件订阅模式）
- `C:\Users\w8067\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\webview2-com-0.38.2\src\{callback,options,pwstr}.rs`
- 若编译报 API 签名错误，以 wry 源码为准修正（本计划代码已按 wry 0.55.1 / webview2-com 0.38.2 实际签名编写）。

---

## 文件结构

| 文件 | 动作 | 职责 |
|---|---|---|
| `src-tauri/Cargo.toml` | 修改 | 添加 webview2-com/windows 依赖（仅 windows target） |
| `src-tauri/src/proxy/webview_host.rs` | 新建 | FFI 唯一所在：Environment/Controller 创建、bounds/可见性/导航/脚本、消息泵 |
| `src-tauri/src/proxy/browser_window.rs` | 新建 | TabBook（纯状态机，可单测）+ Runtime 主循环 + BrowserWindowHost + tauri commands |
| `src-tauri/src/proxy/mod.rs` | 修改 | 会话持 `host: Mutex<Option<Arc<BrowserWindowHost>>>`；状态机改造；删外部浏览器逻辑 |
| `src-tauri/src/proxy/browser.rs` | 删除 | 外部浏览器探测/spawn 全部退役 |
| `src-tauri/src/errors.rs` | 修改 | 追加 3 个错误码 |
| `src-tauri/src/lib.rs` | 修改 | 注册新 commands |
| `src/components/symbolic.tsx` | 修改 | 新增 go-previous/go-next/tab-new 图标 |
| `src/apps/browser/useBrowserTabs.ts` | 新建 | 事件监听 + tab 状态合并 |
| `src/apps/browser/useBrowserBounds.ts` | 新建 | content 区 bounds 上报（rAF 节流） |
| `src/apps/browser/BrowserNavbar.tsx` | 新建 | 导航行（后退/前进/刷新 + 地址栏）；标签条复用 `src/components/ChromeTabStrip/`（Edge/Chrome 布局，portal 进窗口标题栏） |
| `src/apps/BrowserApp.tsx` | 重写 | 浏览器本体（ChromeTabStrip 进标题栏 + 导航行 + content 占位区，Edge/Chrome 布局） |
| `src/apps/BrowserApp.css` | 重写 | 新布局样式 |
| `src/apps/browser/useBrowserTabs.test.ts` | 新建 | 合并逻辑单测 |
| `src/stores/settingsStore.ts` | 修改 | browserId → browserSleepMinutes |
| `src/apps/Settings.tsx` | 修改 | 浏览器分区改造 |
| `src/window-system/init.ts` | 修改 | browser 窗口默认尺寸 1000×700 |
| `src/i18n/locales/zh-CN.ts` / `en-US.ts` | 修改 | browser.*/settings/errors 词条 |
| `src/types/errors.ts` | 修改 | 新错误码映射 |

## 关键设计决定（实现时不可偏离）

1. **线程模型**：每会话一个 std 专用线程（STA）持有全部 COM 对象；命令走 `std::sync::mpsc`，主循环 `recv_timeout(50ms)` + `PeekMessageW` 泵消息（STA 事件派发依赖消息泵）。COM 事件闭包把事件作为命令发回同一 channel，主循环统一处理——避免闭包借用 runtime 状态。
2. **Tauri command ↔ runtime**：`std::sync::mpsc::Sender` 同步发送（非阻塞）+ `tokio::sync::oneshot` 异步等回复；runtime 线程回复时 `reply.send(...)` 同步调用（tokio oneshot 的 send 是同步方法，非 async 上下文可用）。
3. **状态归属**：TabBook（tabs 元数据 + active 指针）只在 runtime 线程内；变更后立即 emit 事件到前端。前端是纯展示。
4. **bounds**：前端报 CSS 像素（`getBoundingClientRect`，相对主窗口客户区）；tauri command 里乘 `scale_factor()` 转物理像素；`put_Bounds` 的 RECT 直接相对主窗口客户区。
5. **窗口遮挡**：child HWND 在主 webview 之上，无法被 React z-index 遮挡 → BrowserApp 非激活时 `browser_set_visible(false)`（webview 隐藏露出 React 占位），激活时 `(true)` + 主循环内对 active webview `SetIsVisible(true)`。这是 spec 第 4 节 z-order 要求的实现。
6. **会话与窗口解耦**：关浏览器窗口（`browser_close_window`）只销毁 webview+Environment，监听保留；重开窗口走 `browser_open` → start_session 的"监听活 + host 无"路径重建 host（同端口）。
7. **事件**：`browser://tabs`（全量列表，结构变更时发）+ `browser://page-state`（单 tab 增量，title/url/loading 变化时发），载荷均带 `server_id`。
8. **标签睡眠**：任何 tab `last_active` 超过睡眠超时（默认 120 分钟，0=永不）即睡眠（销毁 webview 保留元数据）；唤醒=重建 webview + navigate。睡眠扫描任务每 60s 发一次 `ScanSleep` 命令。
9. **新建 tab 命令**：spec 接口面漏列，补 `browser_create_tab(server_id, url?)`（标签条 "+" 按钮使用）。

---

### Task 1: 添加依赖并验证单版本

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: 在 `[dependencies]` 段末尾追加 windows-only 依赖**

在 `src-tauri/Cargo.toml` 的 `[dependencies]` 段末尾（`sha2 = "0.10"` 之后、`[profile.release]` 之前）追加：

```toml
# 内置浏览器：WebView2 独立 Environment（嵌入 Quirel 桌面）
# 版本锁定说明：wry 0.55.1 引入 webview2-com 0.38，必须复用同版本，
# 版本错位会导致 windows crate 双份编译（二进制 +5~15MB）
[target.'cfg(windows)'.dependencies]
webview2-com = "0.38"
windows = "0.61"
```

- [ ] **Step 2: 验证依赖树中 webview2-com 只有单版本**

Run（在 `e:\MyWork\gnome-remote\src-tauri` 目录）：`cargo tree -d -p webview2-com`
Expected: 只出现 `webview2-com 0.38.2` 一行（无 0.3x 其他版本）。

Run：`cargo tree -d -p windows | Select-String "windows v"`
Expected: 只有 `windows v0.61.3`（无 0.6x 双份）。

- [ ] **Step 3: 确认编译不受影响**

Run：`cargo check`
Expected: 编译通过（或仅原有 warning）。

> 此时提示用户：Task 1 完成，建议 git 提交（`git add src-tauri/Cargo.toml src-tauri/Cargo.lock`）。

---

### Task 2: TabBook 纯逻辑状态机（TDD）

**Files:**
- Create: `src-tauri/src/proxy/browser_window.rs`（本任务只写 TabBook + tests 部分，runtime 部分留到 Task 4）

TabBook 是 TabManager 的纯数据核心：tab 元数据、active 指针、last_active 时间戳、状态转换决策。COM 执行由 runtime 层根据返回的动作执行。**不引用任何 COM/FFI 类型，可独立单测。**

- [ ] **Step 1: 创建 `src-tauri/src/proxy/browser_window.rs`，先写测试**

```rust
//! 浏览器窗口宿主：标签状态机 + runtime 线程 + 跨线程句柄
//!
//! 分层：
//! - TabBook：纯数据状态机（本文件可单测的部分，无 COM）
//! - Runtime：STA 线程主循环（执行 TabBook 决策 + COM 操作）
//! - BrowserWindowHost：跨线程句柄（tauri command 的调用入口）
//!
//! 事件流：COM 回调 → 命令 channel 回流 → 主循环统一处理 → emit 前端

/// 单个 tab 的展示元数据（emit 给前端的形态）
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TabInfo {
    pub id: u64,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub sleeping: bool,
}

/// runtime 需要对某 tab 执行的动作（TabBook 决策输出，runtime 执行）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TabAction {
    /// 重建 webview 并导航到 url
    Wake(u64),
    /// 销毁 webview（睡眠/关闭前清理）
    Destroy(u64),
}

#[cfg(test)]
mod tab_book_tests {
    use super::*;

    // ── TabBook 在 Task 2 Step 2 实现；这里先写测试 ──

    #[test]
    fn 新标签默认激活并可查询() {
        let mut book = TabBook::new();
        let id = book.create_tab("https://a.example", true);
        assert_eq!(book.active_id(), Some(id));
        let meta = book.meta(id).unwrap();
        assert_eq!(meta.url, "https://a.example");
        assert_eq!(meta.title, "");
        assert!(!meta.loading);
        assert!(!meta.sleeping);
        assert_eq!(book.list().len(), 1);
    }

    #[test]
    fn 后台创建的标签不改变激活() {
        let mut book = TabBook::new();
        let first = book.create_tab("https://a.example", true);
        let second = book.create_tab("https://b.example", false);
        assert_eq!(book.active_id(), Some(first));
        assert_ne!(first, second);
    }

    #[test]
    fn 切换激活返回旧激活() {
        let mut book = TabBook::new();
        let a = book.create_tab("https://a.example", true);
        let b = book.create_tab("https://b.example", true);
        assert_eq!(book.set_active(b), Some(a));
        assert_eq!(book.active_id(), Some(b));
    }

    #[test]
    fn 切换不存在的标签返回None且无副作用() {
        let mut book = TabBook::new();
        let a = book.create_tab("https://a.example", true);
        assert_eq!(book.set_active(999), None);
        assert_eq!(book.active_id(), Some(a));
    }

    #[test]
    fn 关闭激活标签时激活转移到相邻() {
        let mut book = TabBook::new();
        let a = book.create_tab("https://a.example", true);
        let b = book.create_tab("https://b.example", true);
        let c = book.create_tab("https://c.example", true);
        // 当前激活 c，关闭 c → 转移到 b（相邻前一个）
        assert_eq!(book.close_tab(c), Some(TabAction::Destroy(c)));
        assert_eq!(book.active_id(), Some(b));
        // 关闭 b → 转移到 a
        assert_eq!(book.close_tab(b), Some(TabAction::Destroy(b)));
        assert_eq!(book.active_id(), Some(a));
        // 关闭最后一个 → 无激活
        assert_eq!(book.close_tab(a), Some(TabAction::Destroy(a)));
        assert_eq!(book.active_id(), None);
        assert_eq!(book.list().len(), 0);
    }

    #[test]
    fn 关闭后台标签不影响激活() {
        let mut book = TabBook::new();
        let a = book.create_tab("https://a.example", true);
        let b = book.create_tab("https://b.example", false);
        assert_eq!(book.close_tab(b), Some(TabAction::Destroy(b)));
        assert_eq!(book.active_id(), Some(a));
    }

    #[test]
    fn 关闭不存在的标签返回None() {
        let mut book = TabBook::new();
        assert_eq!(book.close_tab(42), None);
    }

    #[test]
    fn 睡眠标记与唤醒() {
        let mut book = TabBook::new();
        let a = book.create_tab("https://a.example", true);
        book.mark_sleeping(a);
        assert!(book.meta(a).unwrap().sleeping);
        book.mark_woken(a);
        assert!(!book.meta(a).unwrap().sleeping);
    }

    #[test]
    fn 睡眠扫描_超时返回Sleep动作_未超时不动() {
        let mut book = TabBook::new();
        let a = book.create_tab("https://a.example", true);
        let b = book.create_tab("https://b.example", false);
        let timeout = std::time::Duration::from_secs(120);
        let now = std::time::Instant::now();

        // 刚创建：都不超时
        assert!(book.scan_sleep(now, timeout).is_empty());

        // 121 秒后：a、b 都超时（含激活 tab，spec 状态机允许 Active → Sleeping）
        let later = now + std::time::Duration::from_secs(121);
        let actions = book.scan_sleep(later, timeout);
        assert_eq!(actions.len(), 2);
        assert!(actions.contains(&TabAction::Destroy(a)));
        assert!(actions.contains(&TabAction::Destroy(b)));

        // 睡眠后（last_active 不再推进的 tab 不重复睡）：标记睡眠后再扫，无新动作
        book.mark_sleeping(a);
        book.mark_sleeping(b);
        let even_later = later + std::time::Duration::from_secs(1000);
        assert!(book.scan_sleep(even_later, timeout).is_empty());
    }

    #[test]
    fn 睡眠扫描_超时为0表示永不睡眠() {
        let mut book = TabBook::new();
        book.create_tab("https://a.example", true);
        let later = std::time::Instant::now() + std::time::Duration::from_secs(100000);
        // timeout = 0 → 永不睡眠（扫描直接返回空）
        assert!(book.scan_sleep(later, std::time::Duration::ZERO).is_empty());
    }

    #[test]
    fn touch_重置活跃时间避免睡眠() {
        let mut book = TabBook::new();
        let a = book.create_tab("https://a.example", true);
        let timeout = std::time::Duration::from_secs(120);
        let now = std::time::Instant::now();
        let later = now + std::time::Duration::from_secs(121);
        // touch 后从 touch 时刻重新计时
        book.touch(a, later - std::time::Duration::from_secs(10));
        assert!(book.scan_sleep(later, timeout).is_empty());
        let much_later = later + std::time::Duration::from_secs(20);
        assert_eq!(book.scan_sleep(much_later, timeout), vec![TabAction::Destroy(a)]);
    }

    #[test]
    fn 元数据更新_title_url_loading() {
        let mut book = TabBook::new();
        let a = book.create_tab("https://a.example", true);
        book.set_title(a, "Example".into());
        book.set_url(a, "https://a.example/x".into());
        book.set_loading(a, true);
        let meta = book.meta(a).unwrap();
        assert_eq!(meta.title, "Example");
        assert_eq!(meta.url, "https://a.example/x");
        assert!(meta.loading);
        // 不存在的 id 静默忽略（幂等）
        book.set_title(999, "ghost".into());
    }

    #[test]
    fn list_按创建顺序返回() {
        let mut book = TabBook::new();
        let a = book.create_tab("https://a.example", true);
        let b = book.create_tab("https://b.example", true);
        let c = book.create_tab("https://c.example", true);
        let ids: Vec<u64> = book.list().iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![a, b, c]);
    }

    #[test]
    fn 拖拽重排_前进与后退() {
        let mut book = TabBook::new();
        let a = book.create_tab("https://a.example", true);
        let b = book.create_tab("https://b.example", true);
        let c = book.create_tab("https://c.example", true);
        // 前进：a 从 0 移到 2 → b a c
        assert!(book.move_tab(0, 2));
        let ids: Vec<u64> = book.list().iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![b, a, c]);
        // 后退：c 从 2 移到 0 → c b a
        assert!(book.move_tab(2, 0));
        let ids: Vec<u64> = book.list().iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![c, b, a]);
        // 非法索引静默返回 false（ChromeTabStrip 的 from/to 恒在界内，防御）
        assert!(!book.move_tab(3, 0));
        assert!(!book.move_tab(0, 3));
        // 无位移返回 false（避免无意义 emit）
        assert!(!book.move_tab(1, 1));
    }
}
```

- [ ] **Step 2: 实现 TabBook（同文件，tests 模块之上）**

```rust
/// 标签簿：tab 元数据 + 激活指针 + 活跃时间（纯数据，无 COM）
///
/// 状态转换决策集中于此；COM 执行（建/销 webview）由 runtime 按
/// 返回的 TabAction 执行。所有方法对不存在的 id 静默忽略（幂等）。
pub(crate) struct TabBook {
    /// 按 id 保序（list 的展示顺序 = 创建顺序）
    order: Vec<u64>,
    tabs: std::collections::HashMap<u64, TabInfo>,
    /// 活跃时间（睡眠判定依据；睡眠后不再推进，mark_sleeping 时清空）
    last_active: std::collections::HashMap<u64, std::time::Instant>,
    active: Option<u64>,
    next_id: u64,
}

impl TabBook {
    pub(crate) fn new() -> Self {
        Self {
            order: Vec::new(),
            tabs: std::collections::HashMap::new(),
            last_active: std::collections::HashMap::new(),
            active: None,
            next_id: 1,
        }
    }

    /// 新建 tab；activate 时成为激活 tab。返回新 id。
    pub(crate) fn create_tab(&mut self, url: &str, activate: bool) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.order.push(id);
        self.tabs.insert(id, TabInfo {
            id, url: url.to_string(), title: String::new(), loading: false, sleeping: false,
        });
        self.last_active.insert(id, std::time::Instant::now());
        if activate {
            self.active = Some(id);
        }
        id
    }

    pub(crate) fn active_id(&self) -> Option<u64> {
        self.active
    }

    /// 切换激活 tab，返回旧激活 id（目标不存在时 None 且无副作用）
    pub(crate) fn set_active(&mut self, id: u64) -> Option<u64> {
        if !self.tabs.contains_key(&id) {
            return None;
        }
        let old = self.active;
        self.active = Some(id);
        self.touch(id, std::time::Instant::now());
        old
    }

    /// 关闭 tab，返回需销毁 webview 的动作；激活转移规则：关闭激活 tab
    /// 时转移到相邻前一个，无前一个则后一个，都没有则 None。
    pub(crate) fn close_tab(&mut self, id: u64) -> Option<TabAction> {
        if self.tabs.remove(&id).is_none() {
            return None;
        }
        self.last_active.remove(&id);
        self.order.retain(|&x| x != id);

        if self.active == Some(id) {
            let pos = self.order.iter().position(|&x| x == id);
            // order 已移除 id；原激活位置的前一个 = position-1（若存在），否则最后一个
            self.active = if pos > Some(0) {
                self.order.get(pos.unwrap() - 1).copied().or_else(|| self.order.last().copied())
            } else {
                self.order.last().copied()
            };
        }
        Some(TabAction::Destroy(id))
    }

    pub(crate) fn meta(&self, id: u64) -> Option<&TabInfo> {
        self.tabs.get(&id)
    }

    pub(crate) fn meta_mut(&mut self, id: u64) -> Option<&mut TabInfo> {
        self.tabs.get_mut(&id)
    }

    /// 按创建顺序返回全部 tab 元数据
    pub(crate) fn list(&self) -> Vec<TabInfo> {
        self.order.iter().filter_map(|id| self.tabs.get(id).cloned()).collect()
    }

    /// 重置 tab 活跃时间
    pub(crate) fn touch(&mut self, id: u64, now: std::time::Instant) {
        if self.tabs.contains_key(&id) {
            self.last_active.insert(id, now);
        }
    }

    pub(crate) fn mark_sleeping(&mut self, id: u64) {
        if let Some(meta) = self.meta_mut(id) {
            meta.sleeping = true;
            meta.loading = false;
        }
        self.last_active.remove(&id);
    }

    pub(crate) fn mark_woken(&mut self, id: u64) {
        if let Some(meta) = self.meta_mut(id) {
            meta.sleeping = false;
        }
        self.touch(id, std::time::Instant::now());
    }

    pub(crate) fn set_title(&mut self, id: u64, title: String) {
        if let Some(meta) = self.meta_mut(id) { meta.title = title; }
    }

    pub(crate) fn set_url(&mut self, id: u64, url: String) {
        if let Some(meta) = self.meta_mut(id) { meta.url = url; }
    }

    pub(crate) fn set_loading(&mut self, id: u64, loading: bool) {
        if let Some(meta) = self.meta_mut(id) { meta.loading = loading; }
    }

    /// 睡眠扫描：返回需销毁 webview 的 tab 列表。timeout=0 表示永不睡眠。
    /// 已睡眠的 tab（last_active 已清空）不会重复返回。
    pub(crate) fn scan_sleep(
        &mut self,
        now: std::time::Instant,
        timeout: std::time::Duration,
    ) -> Vec<TabAction> {
        if timeout.is_zero() {
            return Vec::new();
        }
        let expired: Vec<u64> = self.last_active.iter()
            .filter(|(_, t)| now.duration_since(**t) > timeout)
            .map(|(id, _)| *id)
            .collect();
        expired.into_iter()
            .map(|id| { self.mark_sleeping(id); TabAction::Destroy(id) })
            .collect()
    }

    /// 拖拽重排（ChromeTabStrip onReorder 的 from/to 语义）。返回是否发生位移。
    pub(crate) fn move_tab(&mut self, from: usize, to: usize) -> bool {
        if from == to || from >= self.order.len() || to >= self.order.len() {
            return false;
        }
        let moved = self.order.remove(from);
        self.order.insert(to, moved);
        true
    }
}
```

- [ ] **Step 3: 注册模块并运行测试（先确认失败存在、再确认全过）**

在 `src-tauri/src/proxy/mod.rs` 顶部模块声明区（`pub mod browser;` 旁）临时加：

```rust
pub mod browser_window;
```

Run（在 `e:\MyWork\gnome-remote\src-tauri`）：`cargo test --lib tab_book_tests`
Expected: 14 个测试全部 PASS。

- [ ] **Step 4: 若有失败，修正实现直到全绿**

Run 同上，Expected: PASS。

> 提示用户：Task 2 完成，建议 git 提交。

---

### Task 3: webview_host.rs — FFI 层（COM 唯一所在）

**Files:**
- Create: `src-tauri/src/proxy/webview_host.rs`

本文件是项目内 `unsafe`/COM 唯一所在（spec 第 4 节组件职责）。只提供原子操作，不做业务决策。所有 API 已对照 wry 0.55.1 实际签名。

- [ ] **Step 1: 写完整文件**

```rust
//! WebView2 FFI 原子操作（项目内 unsafe/COM 唯一所在）
//!
//! 线程模型：每个浏览器会话一个专用 STA 线程，Environment/Controller
//! 全部在该线程创建与操作。创建期间的异步完成（Environment/Controller
//! handler）用 webview2_com::wait_with_pump 泵消息等待。
//!
//! 业务决策（标签状态机、事件路由）在 browser_window.rs；
//! 本文件只做：创建环境 / 创建控制器 / bounds / 可见性 / 导航 / 脚本 / 取 URL / 取标题。

use std::path::Path;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::pwstr::take_pwstr;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{HWND, RECT, MSG};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, PM_REMOVE, TranslateMessage,
};

/// WebView2 控制器 + 页面接口句柄
pub(crate) struct WebviewHandle {
    pub controller: ICoreWebView2Controller,
    pub webview: ICoreWebView2,
}

/// 初始化当前线程为 STA（COM 单线程套间；重复调用返回 S_FALSE，忽略）
pub(crate) fn init_sta() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

/// 泵一次 Windows 消息队列（STA 事件派发依赖；主循环每轮调用）
pub(crate) fn pump_messages() {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// 创建 WebView2 Environment（per server：独立 user-data-dir + SOCKS5 代理参数）
///
/// 不变量（spec 第 5 节）：
/// - additional_browser_args 是替换不是追加 → 显式补回 wry 默认的
///   `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`
/// - 代理仅 `--proxy-server=socks5://127.0.0.1:{port}`；不得加 --disable-gpu 等损害渲染的参数
/// - Environment 创建后代理端口不可变（会话状态机保证存活期内端口不变）
pub(crate) fn create_environment(
    user_data_dir: &Path,
    socks_port: u16,
) -> Result<ICoreWebView2Environment, String> {
    let args = format!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --proxy-server=socks5://127.0.0.1:{}",
        socks_port
    );
    let user_data = HSTRING::from(user_data_dir.as_os_str());

    let options = CoreWebView2EnvironmentOptions::default();
    unsafe {
        options.set_additional_browser_arguments(args);
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |error_code, environment| {
            let result = (|| {
                error_code?;
                environment.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))
            })();
            tx.send(result).map_err(|_| windows::core::Error::from(windows::Win32::Foundation::E_UNEXPECTED))
        },
    ));

    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            windows::core::PCWSTR::null(),
            &user_data,
            &ICoreWebView2EnvironmentOptions::from(options),
            &handler,
        )
        .map_err(|e| format!("创建 WebView2 环境失败: {e}"))?;
    }

    webview2_com::wait_with_pump(rx).map_err(|e| format!("等待 WebView2 环境就绪失败: {e}"))
}

/// 创建 Controller（parent = Quirel 主窗口 HWND）并完成初始配置
///
/// bounds 为物理像素、相对主窗口客户区；visible=false 时仅创建不显示。
pub(crate) fn create_controller(
    parent: HWND,
    env: &ICoreWebView2Environment,
    bounds: RECT,
    visible: bool,
) -> Result<WebviewHandle, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |error_code, controller| {
            let result = (|| {
                error_code?;
                controller.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))
            })();
            tx.send(result).map_err(|_| windows::core::Error::from(windows::Win32::Foundation::E_UNEXPECTED))
        },
    ));

    unsafe {
        env.CreateCoreWebView2Controller(parent, &handler)
            .map_err(|e| format!("创建 WebView2 控制器失败: {e}"))?;
    }
    let controller = webview2_com::wait_with_pump(rx)
        .map_err(|e| format!("等待 WebView2 控制器就绪失败: {e}"))?;

    let webview = unsafe { controller.CoreWebView2() }
        .map_err(|e| format!("获取 WebView2 页面接口失败: {e}"))?;

    unsafe {
        controller.SetBounds(bounds).map_err(|e| format!("设置 bounds 失败: {e}"))?;
        controller.SetIsVisible(visible).map_err(|e| format!("设置可见性失败: {e}"))?;
    }

    Ok(WebviewHandle { controller, webview })
}

/// 应用 bounds（物理像素，相对主窗口客户区）
pub(crate) fn set_bounds(wv: &WebviewHandle, bounds: RECT) {
    unsafe {
        let _ = wv.controller.SetBounds(bounds);
    }
}

/// 设置可见性（切 tab / 窗口激活态同步）
pub(crate) fn set_visible(wv: &WebviewHandle, visible: bool) {
    unsafe {
        let _ = wv.controller.SetIsVisible(visible);
    }
}

/// 将键盘焦点移入页面（切 tab 时）
pub(crate) fn move_focus(wv: &WebviewHandle) {
    unsafe {
        let _ = wv.controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
    }
}

/// 导航到 URL
pub(crate) fn navigate(wv: &WebviewHandle, url: &str) -> Result<(), String> {
    unsafe {
        wv.webview.Navigate(&HSTRING::from(url))
            .map_err(|e| format!("导航失败: {e}"))
    }
}

/// 执行 JS（导航历史回退/前进/刷新的实现路径）
pub(crate) fn execute_script(wv: &WebviewHandle, script: &str) {
    unsafe {
        let handler = ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(())));
        let _ = wv.webview.ExecuteScript(&HSTRING::from(script), &handler);
    }
}

/// 读取当前 URL
pub(crate) fn current_url(wv: &WebviewHandle) -> Option<String> {
    unsafe {
        let mut uri = PWSTR::null();
        wv.webview.Source(&mut uri).ok()?;
        Some(take_pwstr(uri))
    }
}

/// 销毁 webview（controller.Close 会关闭其内部 HWND 并释放 Chromium 页面）
pub(crate) fn destroy(wv: &WebviewHandle) {
    unsafe {
        let _ = wv.controller.Close();
    }
}
```

- [ ] **Step 2: 注册模块**

在 `src-tauri/src/proxy/mod.rs` 的模块声明区追加：

```rust
pub mod webview_host;
```

- [ ] **Step 3: 编译验证**

Run：`cargo check`
Expected: 编译通过。若 webview2 API 签名有出入，对照 wry 源码（路径见计划头部）修正；`CoreWebView2EnvironmentOptions::default()` 的 `set_additional_browser_arguments` 方法名以 `options.rs` 为准。

> 提示用户：Task 3 完成，建议 git 提交。

---

### Task 4: BrowserWindowHost + Runtime 主循环（browser_window.rs 后半）

**Files:**
- Modify: `src-tauri/src/proxy/browser_window.rs`（在 TabBook/tests 之下追加 runtime 与 host）

- [ ] **Step 1: 追加命令枚举与 host/runtime 实现**

在 `browser_window.rs` 的 `mod tests` 之前插入以下代码（tests 模块保持在文件末尾）：

```rust
/* ── 跨线程命令与事件 ─────────────────────────────────── */

/// 单 tab 增量状态（emit browser://page-state）
#[derive(Debug, Clone, serde::Serialize)]
pub struct TabPageState {
    pub server_id: String,
    pub tab_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loading: Option<bool>,
}

/// tab 列表结构变更（emit browser://tabs，全量）
#[derive(Debug, Clone, serde::Serialize)]
pub struct TabsPayload {
    pub server_id: String,
    pub active: Option<u64>,
    pub tabs: Vec<TabInfo>,
}

type Reply<T> = tokio::sync::oneshot::Sender<Result<T, crate::errors::AppError>>;

/// runtime 线程命令（tauri command 调用 + COM 事件回流共用同一 channel）
pub(crate) enum HostCommand {
    Navigate { tab_id: Option<u64>, url: String, reply: Reply<()> },
    Back { tab_id: Option<u64>, reply: Reply<()> },
    Forward { tab_id: Option<u64>, reply: Reply<()> },
    Reload { tab_id: Option<u64>, reply: Reply<()> },
    CreateTab { url: String, activate: bool, reply: Reply<u64> },
    CloseTab { tab_id: u64, reply: Reply<()> },
    FocusTab { tab_id: u64, reply: Reply<()> },
    /// 拖拽重排（ChromeTabStrip onReorder 的 from/to）
    ReorderTab { from: usize, to: usize, reply: Reply<()> },
    /// 物理像素 bounds（tauri command 已按 scale_factor 换算）
    SetBounds { x: i32, y: i32, w: i32, h: i32 },
    /// 窗口激活态同步（spec 第 4 节 z-order 要求）
    SetVisible { visible: bool },
    ScanSleep,
    CloseAll { reply: Option<Reply<()>> },
    // ── COM 事件回流 ──
    EvNavStart { tab_id: u64 },
    EvNavDone { tab_id: u64 },
    EvTitle { tab_id: u64, title: String },
    EvSource { tab_id: u64, url: String },
    EvNewWindow { tab_id: u64, url: String },
}

/// 跨线程句柄：tauri command 的唯一入口
pub struct BrowserWindowHost {
    server_id: String,
    proxy_port: u16,
    cmd_tx: std::sync::mpsc::Sender<HostCommand>,
}

impl BrowserWindowHost {
    /// 请求窗口关闭（销毁全部 webview + drop Environment + 结束线程）。
    /// 监听不受影响（由 ProxySession 管理）。幂等：通道已断时静默。
    pub(crate) fn request_close(&self) {
        let _ = self.cmd_tx.send(HostCommand::CloseAll { reply: None });
    }

    /// 异步等待 CloseAll 完成（显式关闭路径用；Drop 路径用 fire-and-forget）
    pub async fn close_all(&self) -> Result<(), crate::errors::AppError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(HostCommand::CloseAll { reply: Some(tx) })
            .map_err(|_| crate::errors::AppError::new(crate::errors::codes::PROXY_BROWSER_WINDOW_CLOSED))?;
        rx.await
            .map_err(|_| crate::errors::AppError::new(crate::errors::codes::PROXY_BROWSER_WINDOW_CLOSED))?
    }

    pub(crate) fn server_id(&self) -> &str {
        &self.server_id
    }

    pub(crate) fn proxy_port(&self) -> u16 {
        self.proxy_port
    }
}

impl Drop for BrowserWindowHost {
    fn drop(&mut self) {
        // 兜底：任何路径（App 退出/会话移除）导致 host 被丢弃都通知线程退出
        self.request_close();
    }
}
```

- [ ] **Step 2: 追加 Runtime 状态与线程入口（继续在同文件追加）**

```rust
/* ── Runtime（STA 线程内状态与主循环）─────────────────── */

use super::webview_host::{self, WebviewHandle};
use crate::errors::{codes, AppError};

pub(crate) struct RuntimeCtx {
    pub server_id: String,
    pub app: tauri::AppHandle,
    pub user_data_dir: std::path::PathBuf,
    pub proxy_port: u16,
    pub parent_hwnd: windows::Win32::Foundation::HWND,
    pub sleep_timeout: std::time::Duration,
    pub first_url: String,
}

struct RuntimeState {
    ctx: RuntimeCtx,
    env: Option<ICoreWebView2Environment>,
    webviews: std::collections::HashMap<u64, WebviewHandle>,
    book: TabBook,
    /// content 区 bounds（物理像素，相对主窗口客户区）
    rect: windows::Win32::Foundation::RECT,
    /// 窗口激活态（false 时全部 webview 隐藏）
    window_visible: bool,
    cmd_tx: std::sync::mpsc::Sender<HostCommand>,
}

impl RuntimeState {
    fn emit_tabs(&self) {
        let payload = TabsPayload {
            server_id: self.ctx.server_id.clone(),
            active: self.book.active_id(),
            tabs: self.book.list(),
        };
        let _ = self.ctx.app.emit("browser://tabs", payload);
    }

    fn emit_page_state(&self, tab_id: u64) {
        if let Some(meta) = self.book.meta(tab_id) {
            let payload = TabPageState {
                server_id: self.ctx.server_id.clone(),
                tab_id,
                url: Some(meta.url.clone()),
                title: Some(meta.title.clone()),
                loading: Some(meta.loading),
            };
            let _ = self.ctx.app.emit("browser://page-state", payload);
        }
    }

    /// 创建 webview 并注册页面事件（新 tab / 唤醒共用）
    fn create_webview(&mut self, tab_id: u64, url: &str, activate: bool) -> Result<(), String> {
        let env = self.env.as_ref()
            .ok_or_else(|| "Environment 未就绪".to_string())?;
        let visible = activate && self.window_visible;
        let wv = webview_host::create_controller(self.ctx.parent_hwnd, env, self.rect, visible)?;
        webview_host::navigate(&wv, url)?;
        subscribe_page_events(&wv, tab_id, self.cmd_tx.clone())?;
        self.webviews.insert(tab_id, wv);
        Ok(())
    }
}

/// 订阅页面事件：COM 回调把事件作为命令发回主循环（避免闭包借用 runtime 状态）
unsafe fn subscribe_page_events(
    wv: &WebviewHandle,
    tab_id: u64,
    tx: std::sync::mpsc::Sender<HostCommand>,
) -> Result<(), String> {
    use windows::core::EventRegistrationToken as _; // 占位避免误导：token 实为 i64
    let _ = core::mem::drop(());
    let mut token = 0i64;

    wv.webview.add_NavigationStarting(
        &NavigationStartingEventHandler::create(Box::new({
            let tx = tx.clone();
            move |_, _| {
                let _ = tx.send(HostCommand::EvNavStart { tab_id });
                Ok(())
            }
        })),
        &mut token,
    ).map_err(|e| format!("订阅 NavigationStarting 失败: {e}"))?;

    wv.webview.add_NavigationCompleted(
        &NavigationCompletedEventHandler::create(Box::new({
            let tx = tx.clone();
            move |_, _| {
                let _ = tx.send(HostCommand::EvNavDone { tab_id });
                Ok(())
            }
        })),
        &mut token,
    ).map_err(|e| format!("订阅 NavigationCompleted 失败: {e}"))?;

    wv.webview.add_DocumentTitleChanged(
        &DocumentTitleChangedEventHandler::create(Box::new({
            let tx = tx.clone();
            move |webview, _| {
                let Some(webview) = webview else { return Ok(()) };
                let mut title = PWSTR::null();
                webview.DocumentTitle(&mut title)?;
                let _ = tx.send(HostCommand::EvTitle { tab_id, title: take_pwstr(title) });
                Ok(())
            }
        })),
        &mut token,
    ).map_err(|e| format!("订阅 DocumentTitleChanged 失败: {e}"))?;

    wv.webview.add_SourceChanged(
        &SourceChangedEventHandler::create(Box::new({
            let tx = tx.clone();
            move |webview, _| {
                let Some(webview) = webview else { return Ok(()) };
                let mut uri = PWSTR::null();
                webview.Source(&mut uri)?;
                let _ = tx.send(HostCommand::EvSource { tab_id, url: take_pwstr(uri) });
                Ok(())
            }
        })),
        &mut token,
    ).map_err(|e| format!("订阅 SourceChanged 失败: {e}"))?;

    // target=_blank / window.open → 同窗口新建 tab（spec 第 7 节）
    wv.webview.add_NewWindowRequested(
        &NewWindowRequestedEventHandler::create(Box::new({
            let tx = tx.clone();
            move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let url = take_pwstr(uri);
                args.SetHandled(true)?;
                let _ = tx.send(HostCommand::EvNewWindow { tab_id, url });
                Ok(())
            }
        })),
        &mut token,
    ).map_err(|e| format!("订阅 NewWindowRequested 失败: {e}"))?;

    Ok(())
}
```

注意：上面 `use windows::core::EventRegistrationToken as _;` 两行占位是错误的——直接删掉这两行（`let mut token = 0i64;` 即为正确写法，sys 签名 `token: *mut i64` 已验证）。**写入文件时不包含那 3 行占位。**

- [ ] **Step 3: 追加命令处理与主循环（继续在同文件追加）**

```rust
/// 地址规范化：无 scheme 的输入补 https://（地址栏直接输 "example.com" 可用）
fn normalize_url(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return "about:blank".into();
    }
    if t.starts_with("http://") || t.starts_with("https://") || t.starts_with("about:") || t.starts_with("file://") {
        return t.to_string();
    }
    format!("https://{t}")
}

impl RuntimeState {
    /// 定位目标 webview：指定 tab_id 或当前激活 tab；目标不可用返回 None
    fn target(&self, tab_id: Option<u64>) -> Option<(u64, &WebviewHandle)> {
        let id = tab_id.or(self.book.active_id())?;
        let wv = self.webviews.get(&id)?;
        Some((id, wv))
    }

    fn handle_command(&mut self, cmd: HostCommand) {
        match cmd {
            HostCommand::Navigate { tab_id, url, reply } => {
                let result = self.do_navigate(tab_id, &normalize_url(&url));
                let _ = reply.send(result);
            }
            HostCommand::Back { tab_id, reply } => {
                let result = self.do_history(tab_id, "history.back()");
                let _ = reply.send(result);
            }
            HostCommand::Forward { tab_id, reply } => {
                let result = self.do_history(tab_id, "history.forward()");
                let _ = reply.send(result);
            }
            HostCommand::Reload { tab_id, reply } => {
                let result = self.do_history(tab_id, "location.reload()");
                let _ = reply.send(result);
            }
            HostCommand::CreateTab { url, activate, reply } => {
                let result = self.do_create_tab(&normalize_url(&url), activate);
                let _ = reply.send(result);
            }
            HostCommand::CloseTab { tab_id, reply } => {
                self.do_close_tab(tab_id);
                let _ = reply.send(Ok(()));
            }
            HostCommand::FocusTab { tab_id, reply } => {
                self.do_focus_tab(tab_id);
                let _ = reply.send(Ok(()));
            }
            HostCommand::ReorderTab { from, to, reply } => {
                if self.book.move_tab(from, to) {
                    self.emit_tabs();
                }
                let _ = reply.send(Ok(()));
            }
            HostCommand::SetBounds { x, y, w, h } => {
                self.rect = RECT { left: x, top: y, right: x + w, bottom: y + h };
                // 立即应用到当前可见的激活 webview
                if let Some(id) = self.book.active_id() {
                    if let Some(wv) = self.webviews.get(&id) {
                        webview_host::set_bounds(wv, self.rect);
                    }
                }
            }
            HostCommand::SetVisible { visible } => {
                self.window_visible = visible;
                if let Some(id) = self.book.active_id() {
                    if let Some(wv) = self.webviews.get(&id) {
                        webview_host::set_visible(wv, visible);
                    }
                }
            }
            HostCommand::ScanSleep => {
                for action in self.book.scan_sleep(std::time::Instant::now(), self.ctx.sleep_timeout) {
                    self.apply_action(action);
                }
            }
            HostCommand::CloseAll { reply } => {
                self.do_close_all();
                if let Some(r) = reply {
                    let _ = r.send(Ok(()));
                }
            }
            // ── COM 事件回流 ──
            HostCommand::EvNavStart { tab_id } => {
                if let Some(meta) = self.book.meta_mut(tab_id) {
                    meta.loading = true;
                }
                // 导航即活跃（后台标签被 JS 重定向也算）
                self.book.touch(tab_id, std::time::Instant::now());
                self.emit_page_state(tab_id);
            }
            HostCommand::EvNavDone { tab_id } => {
                if let Some(meta) = self.book.meta_mut(tab_id) {
                    meta.loading = false;
                }
                self.emit_page_state(tab_id);
            }
            HostCommand::EvTitle { tab_id, title } => {
                self.book.set_title(tab_id, title);
                self.emit_page_state(tab_id);
            }
            HostCommand::EvSource { tab_id, url } => {
                self.book.set_url(tab_id, url);
                self.emit_page_state(tab_id);
            }
            HostCommand::EvNewWindow { tab_id: _, url } => {
                let _ = self.do_create_tab(&normalize_url(&url), true);
            }
        }
    }

    fn do_navigate(&mut self, tab_id: Option<u64>, url: &str) -> Result<(), AppError> {
        let (id, _) = self.target(tab_id)
            .ok_or_else(|| AppError::new(codes::PROXY_BROWSER_WINDOW_CLOSED))?;
        // 唤醒睡眠 tab：导航即唤醒
        if self.book.meta(id).map(|m| m.sleeping).unwrap_or(false) {
            return self.do_wake(id, url);
        }
        self.book.touch(id, std::time::Instant::now());
        if let Some(wv) = self.webviews.get(&id) {
            webview_host::navigate(wv, url)
                .map_err(|e| AppError::new(codes::PROXY_BROWSER_ENV_FAILED).with_detail(e))?;
        }
        self.book.set_url(id, url.to_string());
        self.emit_page_state(id);
        Ok(())
    }

    fn do_history(&mut self, tab_id: Option<u64>, script: &str) -> Result<(), AppError> {
        let (id, wv) = self.target(tab_id)
            .ok_or_else(|| AppError::new(codes::PROXY_BROWSER_WINDOW_CLOSED))?;
        self.book.touch(id, std::time::Instant::now());
        webview_host::execute_script(wv, script);
        Ok(())
    }

    fn do_create_tab(&mut self, url: &str, activate: bool) -> Result<u64, AppError> {
        self.env.as_ref().ok_or_else(|| AppError::new(codes::PROXY_BROWSER_ENV_FAILED))?;
        let id = self.book.create_tab(url, activate);
        if activate {
            // 旧激活 webview 隐藏
            if let Some(old) = self.book.active_id().filter(|old| *old != id) {
                if let Some(wv) = self.webviews.get(&old) {
                    webview_host::set_visible(wv, false);
                }
            }
        }
        let visible = activate && self.window_visible;
        if let Err(e) = self.create_webview(id, url, visible) {
            // 创建失败：回滚 tab 条目，报错
            self.book.close_tab(id);
            return Err(AppError::new(codes::PROXY_BROWSER_WINDOW_FAILED).with_detail(e));
        }
        self.emit_tabs();
        Ok(id)
    }

    fn do_wake(&mut self, id: u64, url: &str) -> Result<(), AppError> {
        self.env.as_ref().ok_or_else(|| AppError::new(codes::PROXY_BROWSER_ENV_FAILED))?;
        // 旧激活隐藏
        if let Some(old) = self.book.active_id().filter(|old| *old != id) {
            if let Some(wv) = self.webviews.get(&old) {
                webview_host::set_visible(wv, false);
            }
        }
        self.book.set_active(id);
        self.book.mark_woken(id);
        if let Err(e) = self.create_webview(id, url, true) {
            self.book.mark_sleeping(id);
            return Err(AppError::new(codes::PROXY_BROWSER_WINDOW_FAILED).with_detail(e));
        }
        self.emit_tabs();
        self.emit_page_state(id);
        Ok(())
    }

    fn do_focus_tab(&mut self, tab_id: u64) {
        if self.book.meta(tab_id).is_none() {
            return; // 幂等：不存在的 tab 静默忽略
        }
        // 睡眠 tab：点击唤醒（用记录的 url 重建）
        if self.book.meta(tab_id).map(|m| m.sleeping).unwrap_or(false) {
            let url = self.book.meta(tab_id).map(|m| m.url.clone()).unwrap_or_default();
            let _ = self.do_wake(tab_id, &url);
            return;
        }
        let old = self.book.set_active(tab_id);
        if let Some(old) = old.filter(|old| *old != tab_id) {
            if let Some(wv) = self.webviews.get(&old) {
                webview_host::set_visible(wv, false);
            }
        }
        if let Some(wv) = self.webviews.get(&tab_id) {
            webview_host::set_bounds(wv, self.rect);
            webview_host::set_visible(wv, self.window_visible);
            webview_host::move_focus(wv);
        }
        self.emit_tabs();
    }

    fn do_close_tab(&mut self, tab_id: u64) {
        if let Some(action) = self.book.close_tab(tab_id) {
            if let Some(wv) = self.webviews.remove(&tab_id) {
                webview_host::destroy(&wv);
            }
            let _ = action;
            // 激活转移后的新激活 webview 显示并铺满
            if let Some(id) = self.book.active_id() {
                if let Some(wv) = self.webviews.get(&id) {
                    webview_host::set_bounds(wv, self.rect);
                    webview_host::set_visible(wv, self.window_visible);
                }
            }
            self.emit_tabs();
        }
    }

    fn do_close_all(&mut self) {
        for (_, wv) in self.webviews.drain() {
            webview_host::destroy(&wv);
        }
        self.env = None; // drop Environment（user-data-dir 锁随进程引用释放）
        self.book = TabBook::new();
        self.emit_tabs();
    }

    fn apply_action(&mut self, action: TabAction) {
        match action {
            TabAction::Destroy(id) => {
                if let Some(wv) = self.webviews.remove(&id) {
                    webview_host::destroy(&wv);
                }
                self.book.mark_sleeping(id);
                // 若睡的是激活 tab，激活转移（无相邻则 None）
                if self.book.active_id() == Some(id) {
                    // close 后重建的语义太重：直接把激活转给相邻非睡眠 tab
                    // 用 close_tab 的相邻规则：临时关掉再…… 不可取。
                    // 简化：激活保留在睡眠 tab 上，前端点击时唤醒（do_focus_tab 已处理）
                }
                self.emit_tabs();
            }
            TabAction::Wake(id) => {
                if let Some(meta) = self.book.meta(id) {
                    let url = meta.url.clone();
                    let _ = self.do_wake(id, &url);
                }
            }
        }
    }
}

/// runtime 线程入口：初始化 STA → Environment → 首个 tab → 主循环
pub(crate) fn runtime_main(
    ctx: RuntimeCtx,
    cmd_rx: std::sync::mpsc::Receiver<HostCommand>,
    init_tx: tokio::sync::oneshot::Sender<Result<(), String>>,
) {
    webview_host::init_sta();

    let env = match webview_host::create_environment(&ctx.user_data_dir, ctx.proxy_port) {
        Ok(env) => env,
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };

    let mut state = RuntimeState {
        ctx,
        env: Some(env),
        webviews: std::collections::HashMap::new(),
        book: TabBook::new(),
        rect: RECT { left: 0, top: 0, right: 800, bottom: 600 },
        window_visible: true,
        cmd_tx: cmd_rx.clone_tx(),
    };

    // 首个 tab
    let first = state.do_create_tab(&state.ctx.first_url, true);
    if let Err(e) = first {
        let _ = init_tx.send(Err(format!("创建首个标签页失败: {}", e.detail.unwrap_or_default())));
        return;
    }

    let _ = init_tx.send(Ok(()));

    // 主循环：命令处理 + 消息泵（COM 事件依赖 STA 消息派发）
    loop {
        match cmd_rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(cmd) => {
                let is_close = matches!(cmd, HostCommand::CloseAll { .. });
                state.handle_command(cmd);
                webview_host::pump_messages();
                if is_close {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                webview_host::pump_messages();
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    tracing::info!("[Browser] runtime 线程退出: server_id={}", state.ctx.server_id);
}
```

注意 `cmd_rx.clone_tx()` 不存在——`cmd_tx` 需在外部传入。修正：`RuntimeCtx` 增加 `pub cmd_tx: std::sync::mpsc::Sender<HostCommand>`，`RuntimeState.cmd_tx` 从 ctx 取（`ctx.cmd_tx.clone()`）。**写入时按此修正**（state 构造处 `cmd_tx: ctx.cmd_tx.clone()`，删除 `cmd_rx.clone_tx()`）。

- [ ] **Step 4: 追加 spawn 入口（BrowserWindowHost::spawn）**

```rust
impl BrowserWindowHost {
    /// 启动浏览器 runtime 线程并等待初始化完成（Environment + 首个 tab）。
    /// 失败时线程已自行清理，返回 Err。
    pub async fn spawn(
        app: tauri::AppHandle,
        server_id: String,
        user_data_dir: std::path::PathBuf,
        proxy_port: u16,
        parent_hwnd: windows::Win32::Foundation::HWND,
        sleep_timeout: std::time::Duration,
        first_url: String,
    ) -> Result<Arc<BrowserWindowHost>, AppError> {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<HostCommand>();
        let (init_tx, init_rx) = tokio::sync::oneshot::channel();

        let ctx = RuntimeCtx {
            server_id: server_id.clone(),
            cmd_tx: cmd_tx.clone(),
            user_data_dir,
            proxy_port,
            parent_hwnd,
            sleep_timeout,
            first_url,
        };

        std::thread::Builder::new()
            .name(format!("browser-runtime-{server_id}"))
            .spawn(move || runtime_main(ctx, cmd_rx, init_tx))
            .map_err(|e| AppError::new(codes::PROXY_BROWSER_WINDOW_FAILED).with_detail(format!("spawn thread: {e}")))?;

        init_rx.await
            .map_err(|_| AppError::new(codes::PROXY_BROWSER_WINDOW_FAILED).with_detail("runtime init dropped"))?
            .map_err(|e| AppError::new(codes::PROXY_BROWSER_ENV_FAILED).with_detail(e))?;

        let host = Arc::new(BrowserWindowHost {
            server_id,
            proxy_port,
            cmd_tx,
        });

        // 睡眠扫描任务：每 60s 触发一次（host 死亡后 send 失败自动退出）
        let scan_tx = host.cmd_tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                if scan_tx.send(HostCommand::ScanSleep).is_err() {
                    break;
                }
            }
        });

        Ok(host)
    }

    /// 异步发送命令并等待回复（tauri command 的通用模式）
    pub(crate) async fn send_cmd<T>(
        &self,
        make: impl FnOnce(tokio::sync::oneshot::Sender<Result<T, AppError>>) -> HostCommand,
    ) -> Result<T, AppError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(make(tx))
            .map_err(|_| AppError::new(codes::PROXY_BROWSER_WINDOW_CLOSED))?;
        rx.await
            .map_err(|_| AppError::new(codes::PROXY_BROWSER_WINDOW_CLOSED))?
    }

    pub async fn navigate(&self, tab_id: Option<u64>, url: String) -> Result<(), AppError> {
        self.send_cmd(|reply| HostCommand::Navigate { tab_id, url, reply }).await
    }
    pub async fn back(&self, tab_id: Option<u64>) -> Result<(), AppError> {
        self.send_cmd(|reply| HostCommand::Back { tab_id, reply }).await
    }
    pub async fn forward(&self, tab_id: Option<u64>) -> Result<(), AppError> {
        self.send_cmd(|reply| HostCommand::Forward { tab_id, reply }).await
    }
    pub async fn reload(&self, tab_id: Option<u64>) -> Result<(), AppError> {
        self.send_cmd(|reply| HostCommand::Reload { tab_id, reply }).await
    }
    pub async fn create_tab(&self, url: String, activate: bool) -> Result<u64, AppError> {
        self.send_cmd(|reply| HostCommand::CreateTab { url, activate, reply }).await
    }
    pub async fn close_tab(&self, tab_id: u64) -> Result<(), AppError> {
        self.send_cmd(|reply| HostCommand::CloseTab { tab_id, reply }).await
    }
    pub async fn focus_tab(&self, tab_id: u64) -> Result<(), AppError> {
        self.send_cmd(|reply| HostCommand::FocusTab { tab_id, reply }).await
    }
    pub async fn reorder_tab(&self, from: usize, to: usize) -> Result<(), AppError> {
        self.send_cmd(|reply| HostCommand::ReorderTab { from, to, reply }).await
    }
    pub fn set_bounds(&self, x: i32, y: i32, w: i32, h: i32) {
        let _ = self.cmd_tx.send(HostCommand::SetBounds { x, y, w, h });
    }
    pub fn set_visible(&self, visible: bool) {
        let _ = self.cmd_tx.send(HostCommand::SetVisible { visible });
    }
}
```

- [ ] **Step 5: 编译修正**

Run：`cargo check`
Expected: 通过。已知需自查的点：
- `ICoreWebView2Environment` 等 COM 类型需 `use webview2_com::Microsoft::Web::WebView2::Win32::*;`（browser_window.rs 顶部加）
- `RECT/HWND/PWSTR` 需从 `windows::Win32::Foundation` 与 `windows::core` 引入
- `tauri::Manager`/`tauri::Emitter` trait 用于 `app.emit`（文件顶部 `use tauri::Emitter;`）
- `AppError.detail` 字段访问在 `runtime_main` 中用于拼错误消息——若 detail 为 Option，用 `.detail.as_deref().unwrap_or("")` 形式

> 提示用户：Task 4 完成，建议 git 提交。

---

### Task 5: proxy/mod.rs 会话改造 + 错误码 + 删除 browser.rs

**Files:**
- Modify: `src-tauri/src/proxy/mod.rs`
- Modify: `src-tauri/src/errors.rs`
- Delete: `src-tauri/src/proxy/browser.rs`

- [ ] **Step 1: errors.rs 追加错误码**

在 `errors.rs` 的 `codes` 模块 `PROXY_CLEANUP_FAILED` 之后追加：

```rust
    // ── 内置浏览器（proxy/browser_window.rs）──
    pub const PROXY_BROWSER_ENV_FAILED: &str = "proxy_browser_env_failed"; // WebView2 环境创建失败
    pub const PROXY_BROWSER_WINDOW_FAILED: &str = "proxy_browser_window_failed"; // 浏览器窗口/webview 创建失败
    pub const PROXY_BROWSER_WINDOW_CLOSED: &str = "proxy_browser_window_closed"; // 浏览器窗口已关闭
```

（旧的外部浏览器错误码常量保留——约束"code 语义一经发布不可变更"，前端词典不删。）

- [ ] **Step 2: 改写 mod.rs**

改动点（保留 SOCKS5 监听/QUIC 转发全部逻辑不动）：

1. 删除 `pub mod browser;`，保留 `pub mod socks5;`，新增：

```rust
pub mod browser_window;
pub mod socks5;
```

2. 删除 `use std::path::PathBuf; use std::process::Child;`，新增：

```rust
use std::sync::Arc;
```

3. 文件头注释改写为：

```rust
//! SOCKS5 over QUIC 代理会话管理
//!
//! 生命周期：
//! - proxy_start_session / browser_open：绑 SOCKS5 监听（支持固定端口）+ 启动内置浏览器
//!   （WebView2 独立 Environment，user-data-dir 按 server 隔离 + 代理参数锁定端口）
//! - QUIC 断开：suspend_session 仅停监听，浏览器窗口保留（页面显示连接错误）
//! - 重连后再次 start：检测到陈旧会话 → 沿用原端口重启监听，浏览器无感恢复
//! - browser_close_window（关浏览器窗口）：销毁 webview + Environment，监听保留
//! - proxy_stop_session / App 退出：停监听 + 销毁浏览器
```

4. `ProxySessionInfo` 改为：

```rust
/// 返回给前端的会话信息
#[derive(serde::Serialize)]
pub struct ProxySessionInfo {
    pub port: u16,
}
```

5. 删除 `BrowserInfo` 结构与 `proxy_list_browsers` command。

6. `ProxySession` 改为：

```rust
struct ProxySession {
    port: u16,
    /// 监听任务句柄（abort 即关闭监听）
    listener_task: tokio::task::JoinHandle<()>,
    /// 监听是否存活（suspend 时同步置 false，避免 is_finished 的微小竞态）
    listener_alive: bool,
    /// 内置浏览器宿主（None = 窗口已关但监听保留，可重建）
    host: std::sync::Mutex<Option<Arc<browser_window::BrowserWindowHost>>>,
}
```

7. `remove_and_terminate` 改为（监听 abort + host drop 触发 CloseAll）：

```rust
/// 移除会话并终止其监听任务与浏览器（幂等：会话不存在时静默）。
/// cleanup_all_sessions（App 退出）与 proxy_stop_session 的公共实现。
fn remove_and_terminate(app: &AppHandle, server_id: &str, reason: &str) {
    let sessions: State<ProxySessionManager> = app.state();
    let session = sessions.sessions.lock().unwrap().remove(server_id);
    if let Some(sess) = session {
        sess.listener_task.abort();
        // host drop（Arc 归零）→ BrowserWindowHost::Drop 通知 runtime 线程销毁全部 webview
        drop(sess.host.into_inner().unwrap());
        tracing::info!("[Proxy] {}: server_id={}, port={}", reason, server_id, sess.port);
    }
}
```

8. `suspend_session` 不变；`cleanup_all_sessions` 不变。

9. 删除以下函数（外部浏览器专用）：`other_active_session_uses_browser`、`exited_quickly`、`spawn_primary_browser`。

10. 新增辅助函数（放在 `port_conflict_with` 之后）：

```rust
/// 浏览器数据目录（user-data-dir），按 server 隔离（spec 第 5 节第 4 条）
fn browser_data_dir(app: &AppHandle, server_id: &str) -> Result<PathBuf, AppError> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("app cache dir: {e}")))?;
    // server_id 是 uuid，防御性净化（未来 id 来源变化也不产生路径穿越）
    let safe_id = server_id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>();
    let data = dir.join("browser-data").join(safe_id);
    std::fs::create_dir_all(&data)
        .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("create browser data dir: {e}")))?;
    Ok(data)
}

/// 取主窗口 HWND（Controller 的 parent）
fn main_window_hwnd(app: &AppHandle) -> Result<windows::Win32::Foundation::HWND, AppError> {
    use tauri::Manager;
    app.get_webview_window("main")
        .ok_or_else(|| AppError::new(codes::PROXY_BROWSER_WINDOW_FAILED).with_detail("main window not found"))?
        .hwnd()
        .map_err(|e| AppError::new(codes::PROXY_BROWSER_WINDOW_FAILED).with_detail(format!("hwnd: {e}")))
}
```

11. 新增内部启动函数 + 重写 `proxy_start_session`：

```rust
/// 为会话创建浏览器 host（Environment + 首个 tab；sleep_timeout 由前端设置传入）
async fn create_host(
    app: &AppHandle,
    server_id: &str,
    port: u16,
    first_url: &str,
) -> Result<Arc<browser_window::BrowserWindowHost>, AppError> {
    use crate::stores_bridge::browser_sleep_timeout; // 见 Task 5 Step 4 说明
    let data_dir = browser_data_dir(app, server_id)?;
    let hwnd = main_window_hwnd(app)?;
    browser_window::BrowserWindowHost::spawn(
        app.clone(),
        server_id.to_string(),
        data_dir,
        port,
        hwnd,
        browser_sleep_timeout(&app),
        first_url.to_string(),
    )
    .await
}

/// 启动代理会话：SOCKS5 监听（支持固定端口）+ 内置浏览器
///
/// 会话状态机：
/// - 无会话 → 全新启动（绑端口 + 创建 host）
/// - 监听活跃 + host 在 → no-op（返回现状，前端聚焦窗口）
/// - 监听活跃 + host 无（窗口刚关）→ 原端口重建 host（Environment 重新创建，登录态保留）
/// - 陈旧会话（断线挂起）→ 原端口（host 在时）或设置端口重启监听；host 无则重建
#[tauri::command]
pub async fn proxy_start_session(
    server_id: String,
    port: Option<u16>,
    app: AppHandle,
) -> Result<ProxySessionInfo, AppError> {
    start_session_inner(&server_id, port, "about:blank", &app).await
}

/// browser_open：browser_open(server_id, url?) 前端唯一入口（spec 第 8 节）
#[tauri::command]
pub async fn browser_open(
    server_id: String,
    url: Option<String>,
    app: AppHandle,
) -> Result<ProxySessionInfo, AppError> {
    start_session_inner(&server_id, None, url.as_deref().unwrap_or("about:blank"), &app).await
}

async fn start_session_inner(
    server_id: &str,
    requested_port: Option<u16>,
    first_url: &str,
    app: &AppHandle,
) -> Result<ProxySessionInfo, AppError> {
    // 串行化整个创建流程（跨 await 持锁）：并发调用排队执行，
    // 后到者会看到先到者刚注册的会话并走"已存在"路径，不再双重绑端口
    let sessions_state = app.state::<ProxySessionManager>();
    let _creation_guard = sessions_state.creation_lock.lock().await;

    let requested_port = requested_port.filter(|p| *p > 0);

    // 从 ConnectionManager 取该 server 的 QUIC 连接（代理流只能走 QUIC）
    let quic_conn = {
        let manager: State<ConnectionManager> = app.state();
        let conns = manager.connections.lock().unwrap();
        conns.get(server_id)
            .and_then(|c| c.quic_conn.clone())
            .ok_or_else(|| AppError::new(codes::CONNECTION_NOT_FOUND))?
    };

    // ── 会话已存在：按状态分发 ─────────────────────────────
    let existing = {
        let sessions: State<ProxySessionManager> = app.state();
        let guard = sessions.sessions.lock().unwrap();
        guard.get(server_id).map(|s| {
            (s.port, s.listener_alive, s.host.lock().unwrap().is_some())
        })
    };

    if let Some((sess_port, listener_alive, host_alive)) = existing {
        if listener_alive {
            if host_alive {
                // 活跃会话：no-op
                return Ok(ProxySessionInfo { port: sess_port });
            }
            // 窗口刚关（监听保留）：原端口重建 host（Environment 重新创建）
            let host = create_host(app, server_id, sess_port, first_url).await?;
            let sessions: State<ProxySessionManager> = app.state();
            if let Some(sess) = sessions.sessions.lock().unwrap().get_mut(server_id) {
                *sess.host.lock().unwrap() = Some(host);
            }
            tracing::info!("[Proxy] 浏览器窗口重建（监听未变）: server_id={}, port={}", server_id, sess_port);
            return Ok(ProxySessionInfo { port: sess_port });
        }

        // ── 陈旧会话（断线挂起）：重建监听 ──
        // host 还在 → 必须沿用原端口（Environment 代理参数指向它）；
        // host 已关 → 无端口约束，优先用新设置的固定端口
        let rebuild_port = if host_alive { sess_port } else { requested_port.unwrap_or(sess_port) };
        if let Some(holder) = port_conflict_with(app, server_id, rebuild_port) {
            return Err(
                AppError::new(codes::PROXY_PORT_CONFLICT)
                    .with_param("port", rebuild_port.to_string())
                    .with_param("holder", holder),
            );
        }
        let (actual_port, listener_task) = spawn_socks5_listener(quic_conn, rebuild_port).await?;

        let new_host = if host_alive {
            None
        } else {
            Some(create_host(app, server_id, actual_port, first_url).await?)
        };

        let sessions: State<ProxySessionManager> = app.state();
        let mut guard = sessions.sessions.lock().unwrap();
        if let Some(sess) = guard.get_mut(server_id) {
            sess.port = actual_port;
            sess.listener_task = listener_task;
            sess.listener_alive = true;
            if let Some(host) = new_host {
                *sess.host.lock().unwrap() = Some(host);
            }
        }
        tracing::info!(
            "[Proxy] 会话已恢复: server_id={}, socks_port={}, host_alive={}",
            server_id, actual_port, host_alive
        );
        return Ok(ProxySessionInfo { port: actual_port });
    }

    // ── 全新会话 ─────────────────────────────────────────
    let bind_port = requested_port.unwrap_or(0);
    if let Some(holder) = port_conflict_with(app, server_id, bind_port) {
        return Err(
            AppError::new(codes::PROXY_PORT_CONFLICT)
                .with_param("port", bind_port.to_string())
                .with_param("holder", holder),
        );
    }
    let (port, listener_task) = spawn_socks5_listener(quic_conn, bind_port).await?;

    // 创建 host；失败时回收监听任务（否则端口监听永久泄漏）
    let host = match create_host(app, server_id, port, first_url).await {
        Ok(h) => h,
        Err(e) => {
            listener_task.abort();
            return Err(e);
        }
    };

    let sessions: State<ProxySessionManager> = app.state();
    sessions.sessions.lock().unwrap().insert(
        server_id.to_string(),
        ProxySession {
            port,
            listener_task,
            listener_alive: true,
            host: std::sync::Mutex::new(Some(host)),
        },
    );

    tracing::info!("[Proxy] 会话已启动: server_id={}, socks_port={}", server_id, port);
    Ok(ProxySessionInfo { port })
}
```

12. `proxy_stop_session` 保留原样（remove_and_terminate 语义已改）。

13. `ProxySessionStatus` 保持字段不变；`proxy_session_status` 改为：

```rust
/// 查询代理会话状态（无会话返回 null）
///
/// browser_alive = 浏览器窗口开着（host 存在）。前端据此区分重连后的
/// 恢复策略：窗口开着 → 调 proxy_start_session 仅重建监听（无感恢复）；
/// 窗口已关或无会话 → 等待用户主动打开。
#[tauri::command]
pub fn proxy_session_status(server_id: String, app: AppHandle) -> Option<ProxySessionStatus> {
    let sessions: State<ProxySessionManager> = app.state();
    let guard = sessions.sessions.lock().unwrap();
    guard.get(&server_id).map(|s| ProxySessionStatus {
        port: s.port,
        listener_alive: s.listener_alive,
        browser_alive: s.host.lock().unwrap().is_some(),
    })
}
```

（`ProxySessionStatus` 结构删除 `browser` 字段，其余不变。）

14. 删除 `browser_profile_dir` 函数。

- [ ] **Step 3: 删除 browser.rs**

用 DeleteFile 删除 `src-tauri/src/proxy/browser.rs`。

- [ ] **Step 4: 睡眠超时读取（临时硬编码，Task 10 接前端设置）**

睡眠超时需要从前端设置读取。Rust 端没有 settingsStore——采用"前端传入"模式：`browser_open` 增加参数 `sleep_minutes: Option<u64>`。修改签名与传递：

`browser_open(server_id, url, sleep_minutes, app)`；`start_session_inner` 增加 `sleep_minutes: Option<u64>` 参数；`create_host` 改为：

```rust
async fn create_host(
    app: &AppHandle,
    server_id: &str,
    port: u16,
    first_url: &str,
    sleep_minutes: u64,
) -> Result<Arc<browser_window::BrowserWindowHost>, AppError> {
    let data_dir = browser_data_dir(app, server_id)?;
    let hwnd = main_window_hwnd(app)?;
    browser_window::BrowserWindowHost::spawn(
        app.clone(),
        server_id.to_string(),
        data_dir,
        port,
        hwnd,
        std::time::Duration::from_secs(sleep_minutes * 60),
        first_url.to_string(),
    )
    .await
}
```

`proxy_start_session` 恢复路径（无 sleep_minutes 入参）传 120（默认值）。注意：恢复路径 host 已存在时不需要 sleep_minutes；只有重建 host 的路径需要——统一传 120 即可（重建场景罕见，且设置变更主要影响新会话）。

> **检查点**：删除 `use crate::stores_bridge::browser_sleep_timeout;` 那行（Step 2 代码中的占位 import 不存在），以本 Step 为准。

- [ ] **Step 5: 编译验证**

Run：`cargo check`
Expected: 通过（`proxy_list_browsers` 未注册到 lib.rs 前可能有 dead_code warning——Task 6 会处理 lib.rs；若因 lib.rs generate_handler 引用已删函数报错，先临时跳过，Task 6 一并修复）。

Run：`cargo test --lib tab_book_tests`
Expected: 13 PASS（TabBook 不受影响）。

> 提示用户：Task 5 完成，建议 git 提交。

---

### Task 6: Tauri commands 注册 + lib.rs

**Files:**
- Modify: `src-tauri/src/proxy/browser_window.rs`（追加 7 个 command）
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: browser_window.rs 末尾（tests 模块之前）追加 tauri commands**

```rust
/* ── Tauri commands（前端接口面，spec 第 8 节）─────────── */

use tauri::{AppHandle, Manager, State};

/// 从会话表取 host；窗口未开报错（用户视角文案由前端词典渲染）
fn host_of(app: &AppHandle, server_id: &str) -> Result<Arc<BrowserWindowHost>, AppError> {
    let sessions: State<ProxySessionManager> = app.state();
    let guard = sessions.sessions.lock().unwrap();
    guard
        .get(server_id)
        .and_then(|s| s.host.lock().unwrap().clone())
        .ok_or_else(|| AppError::new(codes::PROXY_BROWSER_WINDOW_CLOSED))
}

/// 关闭浏览器窗口：销毁全部 webview + Environment；监听保留（面板可再开）
#[tauri::command]
pub async fn browser_close_window(server_id: String, app: AppHandle) -> Result<(), String> {
    let host = {
        let sessions: State<ProxySessionManager> = app.state();
        sessions
            .sessions
            .lock()
            .unwrap()
            .get(&server_id)
            .and_then(|s| s.host.lock().unwrap().take())
    };
    if let Some(host) = host {
        let _ = host.close_all().await;
    }
    Ok(())
}

/// 当前/指定 tab 导航
#[tauri::command]
pub async fn browser_navigate(
    server_id: String,
    url: String,
    tab_id: Option<u64>,
    app: AppHandle,
) -> Result<(), AppError> {
    host_of(&app, &server_id)?.navigate(tab_id, url).await
}

#[tauri::command]
pub async fn browser_back(server_id: String, tab_id: Option<u64>, app: AppHandle) -> Result<(), AppError> {
    host_of(&app, &server_id)?.back(tab_id).await
}

#[tauri::command]
pub async fn browser_forward(server_id: String, tab_id: Option<u64>, app: AppHandle) -> Result<(), AppError> {
    host_of(&app, &server_id)?.forward(tab_id).await
}

#[tauri::command]
pub async fn browser_reload(server_id: String, tab_id: Option<u64>, app: AppHandle) -> Result<(), AppError> {
    host_of(&app, &server_id)?.reload(tab_id).await
}

/// 新建标签页（标签条 "+" 按钮）
#[tauri::command]
pub async fn browser_create_tab(
    server_id: String,
    url: Option<String>,
    app: AppHandle,
) -> Result<u64, AppError> {
    host_of(&app, &server_id)?
        .create_tab(url.unwrap_or_else(|| "about:blank".into()), true)
        .await
}

#[tauri::command]
pub async fn browser_close_tab(server_id: String, tab_id: u64, app: AppHandle) -> Result<(), AppError> {
    host_of(&app, &server_id)?.close_tab(tab_id).await
}

#[tauri::command]
pub async fn browser_focus_tab(server_id: String, tab_id: u64, app: AppHandle) -> Result<(), AppError> {
    host_of(&app, &server_id)?.focus_tab(tab_id).await
}

/// 拖拽重排（ChromeTabStrip onReorder）
#[tauri::command]
pub async fn browser_reorder_tab(server_id: String, from: usize, to: usize, app: AppHandle) -> Result<(), AppError> {
    host_of(&app, &server_id)?.reorder_tab(from, to).await
}

/// content 区布局上报（CSS 像素 → 按 scale_factor 换算物理像素）。
/// 幂等：窗口/tab 不存在时静默忽略（spec 第 9 节）。
#[tauri::command]
pub async fn browser_set_bounds(
    server_id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    app: AppHandle,
) -> Result<(), String> {
    let Some(host) = host_of(&app, &server_id).ok() else {
        return Ok(()); // 窗口已关：布局事件迟到，静默忽略
    };
    let scale = {
        use tauri::Manager;
        app.get_webview_window("main")
            .and_then(|w| w.scale_factor().ok())
            .unwrap_or(1.0)
    };
    if width < 1.0 || height < 1.0 {
        return Ok(()); // 面积为 0（如最小化）不更新
    }
    host.set_bounds(
        (x * scale).round() as i32,
        (y * scale).round() as i32,
        (width * scale).round() as i32,
        (height * scale).round() as i32,
    );
    Ok(())
}

/// 窗口激活态同步（非激活时 webview 隐藏露出 React 占位，防 child HWND 穿透显示）
#[tauri::command]
pub async fn browser_set_visible(server_id: String, visible: bool, app: AppHandle) -> Result<(), String> {
    if let Some(host) = host_of(&app, &server_id).ok() {
        host.set_visible(visible);
    }
    Ok(())
}
```

注意：`ProxySessionManager` 与 `ProxySession` 的 `host` 字段可见性——`host_of` 在 browser_window.rs 中访问 `sessions.sessions` / `s.host`，需要 mod.rs 中这两个字段对本模块可见（同 crate，`struct ProxySession` 是 mod.rs 私有类型——把 `ProxySession` 字段声明为 `pub(crate)` 或将 host_of 移到 mod.rs。**选择**：在 mod.rs 中将 `ProxySession` 改为 `pub(crate) struct` 且字段 `pub(crate)`；`ProxySessionManager.sessions` 同理 `pub(crate)`）。执行时同步修改。

- [ ] **Step 2: lib.rs 注册命令**

`invoke_handler` 列表中将：

```rust
            proxy::proxy_start_session,
            proxy::proxy_stop_session,
            proxy::proxy_session_status,
            proxy::proxy_list_browsers,
```

改为：

```rust
            proxy::proxy_start_session,
            proxy::browser_open,
            proxy::proxy_stop_session,
            proxy::proxy_session_status,
            proxy::browser_window::browser_close_window,
            proxy::browser_window::browser_navigate,
            proxy::browser_window::browser_back,
            proxy::browser_window::browser_forward,
            proxy::browser_window::browser_reload,
            proxy::browser_window::browser_create_tab,
            proxy::browser_window::browser_close_tab,
            proxy::browser_window::browser_focus_tab,
            proxy::browser_window::browser_reorder_tab,
            proxy::browser_window::browser_set_bounds,
            proxy::browser_window::browser_set_visible,
```

- [ ] **Step 3: 编译验证**

Run：`cargo check`
Expected: 通过，无 dead_code warning（proxy_list_browsers/BrowserInfo 已随 Task 5 删除）。

Run：`cargo test --lib`
Expected: 全部 PASS。

> 提示用户：Task 6 完成，建议 git 提交。Rust 侧到此完成。

---

### Task 7: 前端 — 错误码映射 + symbolic 图标 + hooks

**Files:**
- Modify: `src/types/errors.ts`
- Modify: `src/i18n/locales/zh-CN.ts`、`en-US.ts`（错误词典部分）
- Modify: `src/components/symbolic.tsx`
- Create: `src/apps/browser/useBrowserTabs.ts`
- Create: `src/apps/browser/useBrowserBounds.ts`
- Create: `src/apps/browser/useBrowserTabs.test.ts`

- [ ] **Step 1: errors.ts 追加映射**

在 `src/types/errors.ts` 的 code→i18n 映射表（约 199 行 `proxy_browser_in_use` 之后）追加：

```ts
  proxy_browser_env_failed: "op.proxyBrowserEnvFailed",
  proxy_browser_window_failed: "op.proxyBrowserWindowFailed",
  proxy_browser_window_closed: "op.proxyBrowserWindowClosed",
```

- [ ] **Step 2: i18n 错误词典**

`zh-CN.ts` 的 `op` 段（`proxyBrowserInUse` 之后）追加：

```ts
    proxyBrowserEnvFailed: '浏览器环境初始化失败；请重试，若反复出现请重启应用',
    proxyBrowserWindowFailed: '浏览器窗口创建失败；请重试',
    proxyBrowserWindowClosed: '浏览器窗口已关闭；请重新打开远程浏览',
```

`en-US.ts` 对应位置追加：

```ts
    proxyBrowserEnvFailed: 'Failed to initialize the browser environment; try again, and restart the app if it persists',
    proxyBrowserWindowFailed: 'Failed to create the browser window; try again',
    proxyBrowserWindowClosed: 'The browser window is closed; reopen Remote Browser',
```

同时更新 `errors.test.ts` 中错误码全集测试（若存在全集断言，`src/types/errors.test.ts:207-209` 附近的列表追加三个新码）。

- [ ] **Step 3: symbolic.tsx 新增图标**

`SymbolicIconName` 类型追加 `"go-previous" | "go-next" | "tab-new"`；`ICONS` 追加（Adwaita 风格 16×16，currentColor）：

```tsx
  "go-previous": (
    <path d="M10.5 2.5 4 8l6.5 5.5V2.5Z" fill="currentColor" />
  ),
  "go-next": (
    <path d="M5.5 2.5 12 8l-6.5 5.5V2.5Z" fill="currentColor" />
  ),
  "tab-new": (
    <>
      <path d="M2 3.5C2 2.7 2.7 2 3.5 2h9c.8 0 1.5.7 1.5 1.5v9c0 .8-.7 1.5-1.5 1.5h-9C2.7 14 2 13.3 2 12.5v-9Z" fill="none" stroke="currentColor" strokeWidth="1.2" />
      <path d="M8 5v6M5 8h6" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    </>
  ),
```

同步更新 `src/components/symbolic.test.tsx` 的图标名清单（追加三个）。

- [ ] **Step 4: useBrowserTabs.ts**

```tsx
// src/apps/browser/useBrowserTabs.ts
// 浏览器标签状态：订阅 Rust 事件（browser://tabs 全量 / browser://page-state 增量）

import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** 单个标签元数据（Rust 端 TabInfo） */
export interface TabInfo {
  id: number;
  url: string;
  title: string;
  loading: boolean;
  sleeping: boolean;
}

/** tab 列表结构变更（Rust 端 TabsPayload，全量） */
interface TabsPayload {
  server_id: string;
  active: number | null;
  tabs: TabInfo[];
}

/** 单 tab 增量状态（Rust 端 TabPageState） */
interface PageStatePayload {
  server_id: string;
  tab_id: number;
  url?: string;
  title?: string;
  loading?: boolean;
}

/**
 * 增量合并 page-state 到列表（纯函数，可测）
 */
export function mergePageState(tabs: TabInfo[], ev: PageStatePayload): TabInfo[] {
  return tabs.map((t) =>
    t.id === ev.tab_id
      ? {
          ...t,
          url: ev.url ?? t.url,
          title: ev.title ?? t.title,
          loading: ev.loading ?? t.loading,
        }
      : t,
  );
}

/**
 * 订阅浏览器标签状态（按 server_id 过滤）
 */
export function useBrowserTabs(serverId: string | null) {
  const [tabs, setTabs] = useState<TabInfo[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);

  useEffect(() => {
    if (!serverId) {
      setTabs([]);
      setActiveId(null);
      return;
    }
    const unlisteners: Promise<UnlistenFn>[] = [
      listen<TabsPayload>("browser://tabs", (e) => {
        if (e.payload.server_id !== serverId) return;
        setTabs(e.payload.tabs);
        setActiveId(e.payload.active);
      }),
      listen<PageStatePayload>("browser://page-state", (e) => {
        if (e.payload.server_id !== serverId) return;
        setTabs((prev) => mergePageState(prev, e.payload));
      }),
    ];
    return () => {
      unlisteners.forEach((p) => p.then((un) => un()).catch(() => {}));
    };
  }, [serverId]);

  return { tabs, activeId };
}
```

- [ ] **Step 5: useBrowserBounds.ts**

```tsx
// src/apps/browser/useBrowserBounds.ts
// content 区布局上报：ResizeObserver + 窗口几何变化 → rAF 节流 → browser_set_bounds
// （spec 第 3 节：桌面内窗口移动/缩放时的唯一热点，rAF 保证一帧最多一次）

import { useEffect, useRef, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";

/** 窗口几何快照（引用值变化即触发重新上报；来自 useWindowState） */
export interface WindowGeometry {
  x: number;
  y: number;
  width: number;
  height: number;
}

export function useBrowserBounds(
  serverId: string | null,
  contentRef: RefObject<HTMLElement | null>,
  geometry: WindowGeometry,
  enabled: boolean,
) {
  const rafRef = useRef(0);

  useEffect(() => {
    const el = contentRef.current;
    if (!el || !serverId || !enabled) return;

    const report = () => {
      rafRef.current = 0;
      const r = el.getBoundingClientRect();
      // 面积为 0（最小化/隐藏）不上报，Rust 端保留旧 bounds
      if (r.width < 1 || r.height < 1) return;
      invoke("browser_set_bounds", {
        serverId,
        x: r.left,
        y: r.top,
        width: r.width,
        height: r.height,
      }).catch(() => {}); // 窗口刚关时布局事件迟到，静默
    };
    // rAF 节流：一帧最多一次
    const schedule = () => {
      if (!rafRef.current) rafRef.current = requestAnimationFrame(report);
    };

    schedule();
    const ro = new ResizeObserver(schedule);
    ro.observe(el);
    return () => {
      ro.disconnect();
      if (rafRef.current) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = 0;
      }
    };
  }, [serverId, enabled, geometry.x, geometry.y, geometry.width, geometry.height]);
}
```

- [ ] **Step 6: useBrowserTabs 合并逻辑测试**

```tsx
// src/apps/browser/useBrowserTabs.test.ts
import { describe, it, expect } from "vitest";
import { mergePageState, type TabInfo } from "./useBrowserTabs";

const base: TabInfo[] = [
  { id: 1, url: "https://a.example", title: "A", loading: false, sleeping: false },
  { id: 2, url: "https://b.example", title: "B", loading: true, sleeping: false },
];

describe("mergePageState", () => {
  it("合并指定 tab 的增量字段", () => {
    const merged = mergePageState(base, {
      server_id: "s1", tab_id: 2, title: "B Page", loading: false,
    });
    expect(merged[1]).toEqual({ id: 2, url: "https://b.example", title: "B Page", loading: false, sleeping: false });
    expect(merged[0]).toEqual(base[0]);
  });

  it("未提供字段保持原值", () => {
    const merged = mergePageState(base, { server_id: "s1", tab_id: 1, loading: true });
    expect(merged[0].url).toBe("https://a.example");
    expect(merged[0].title).toBe("A");
    expect(merged[0].loading).toBe(true);
  });

  it("不相关 tab 不受影响且返回新数组", () => {
    const merged = mergePageState(base, { server_id: "s1", tab_id: 999, title: "ghost" });
    expect(merged).not.toBe(base);
    expect(merged).toEqual(base);
  });
});
```

- [ ] **Step 7: 运行测试**

Run：`npx vitest run src/apps/browser/useBrowserTabs.test.ts src/components/symbolic.test.tsx src/types/errors.test.ts`
Expected: 全部 PASS。

> 提示用户：Task 7 完成，建议 git 提交。

---

### Task 8: BrowserNavbar + BrowserApp 改造（ChromeTabStrip + Edge/Chrome 布局）+ CSS + 窗口尺寸

**Files:**
- Create: `src/apps/browser/BrowserNavbar.tsx`
- Modify: `src/apps/BrowserApp.tsx`（重写）
- Modify: `src/apps/BrowserApp.css`（重写）
- Modify: `src/window-system/init.ts`（browser 注册项尺寸）

**布局对齐 Edge/Chrome（用户新增要求）：**
```
┌─ WindowShell HeaderBar（窗口标题栏）────────────────────┐
│ ChromeTabStrip（portal 进标题栏，variant="light"）       │ ← 标题栏即标签条
├─ BrowserNavbar（AppLayout toolbar 行）──────────────────┤
│ [←] [→] [⟳]   [🔒 地址栏]                               │
├─ content（webview child HWND）──────────────────────────┤
└─────────────────────────────────────────────────────────┘
```

复用项目的共享 ChromeTabStrip 组件（`src/components/ChromeTabStrip/`，TextEditor/Terminal 同款——Chrome 式页签几何、拖拽排序、关闭按钮、"+" 按钮全部由组件提供）：
- 与 TextEditor 完全相同的集成模式：`titlebarSlot`（`document.querySelector('[data-titlebar-slot="${windowId}"]')`）+ `createPortal` 进窗口标题栏，槽位缺失时回退应用内渲染；
- `variant="light"`：活动页签融合内容区浅色表面（--quirel-* 令牌，TextEditor 同款视觉）；
- `ChromeTabItem.id` 是 string，浏览器 tab_id 是 number → 映射 `String(id)` / `Number(id)`；
- `onReorder` 贯通到 Rust 端（Task 2 `move_tab` + Task 4 `ReorderTab` + Task 6 `browser_reorder_tab`），重排后由全量 `browser://tabs` 事件驱动 UI（引擎 DOM 顺序与 React 状态以 Rust order 为唯一事实源）。

- [ ] **Step 1: BrowserNavbar.tsx（导航按钮 + 地址栏；标签条由 ChromeTabStrip 承担）**

```tsx
// src/apps/browser/BrowserNavbar.tsx
// 浏览器导航行：后退/前进/刷新 + 地址栏（Edge/Chrome 布局的第二行；
// 标签条是窗口标题栏本体，见 BrowserApp 的 titlebarSlot portal）

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { SymbolicIcon } from "../../components/symbolic";
import type { TabInfo } from "./useBrowserTabs";

interface BrowserNavbarProps {
  activeTab: TabInfo | null;
  onNavigate: (url: string) => void;
  onBack: () => void;
  onForward: () => void;
  onReload: () => void;
}

export function BrowserNavbar({ activeTab, onNavigate, onBack, onForward, onReload }: BrowserNavbarProps) {
  const { t } = useTranslation();

  // 地址栏本地草稿：焦点编辑时不被页面事件覆盖；失焦/切 tab 时同步回实际 URL
  const [draft, setDraft] = useState(activeTab?.url ?? "");
  const [focused, setFocused] = useState(false);
  useEffect(() => {
    if (!focused) setDraft(activeTab?.url ?? "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTab?.id, activeTab?.url, focused]);

  const submit = () => {
    (document.activeElement as HTMLElement | null)?.blur();
    if (draft.trim()) onNavigate(draft.trim());
  };

  return (
    <div className="br-navbar">
      <button type="button" className="br-nav-btn" title={t("browser.back")} onClick={onBack}>
        <SymbolicIcon name="go-previous" size={14} />
      </button>
      <button type="button" className="br-nav-btn" title={t("browser.forward")} onClick={onForward}>
        <SymbolicIcon name="go-next" size={14} />
      </button>
      <button type="button" className="br-nav-btn" title={t("browser.reload")} onClick={onReload}>
        <SymbolicIcon name="view-refresh" size={14} />
      </button>
      <div className="br-addressbar">
        {activeTab?.loading && <span className="br-loading-dot" aria-hidden />}
        <input
          className="br-address"
          value={draft}
          placeholder={t("browser.addressPlaceholder")}
          onChange={(e) => setDraft(e.target.value)}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          onKeyDown={(e) => {
            if (e.key === "Enter") submit();
          }}
        />
      </div>
    </div>
  );
}
```

- [ ] **Step 2: BrowserApp.tsx 重写（ChromeTabStrip portal 进标题栏 + AppLayout）**

```tsx
// src/apps/BrowserApp.tsx
// 内置浏览器（WebView2 嵌入 Quirel 桌面）——Edge/Chrome 布局：
// 标题栏 = 标签条（ChromeTabStrip portal，TextEditor/Terminal 同款集成），
// 第二行 = 导航 + 地址栏（AppLayout toolbar），content = webview child HWND。
//
// 窗口非激活时 webview 隐藏（child HWND 无法被 React z-index 遮挡，spec 第 4 节）；
// 流量走 SOCKS5 → QUIC 隧道（服务器视角浏览，不变量见 spec 第 5 节）。

import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { useServerManager } from "../context/ServerManager";
import { useSettingsStore } from "../stores/settingsStore";
import { createLogger } from "../utils/logger";
import { SymbolicIcon } from "../components/symbolic";
import { ConnectionErrorState, ReconnectingState } from "../components/ConnectionState";
import { getServerErrorInfo } from "../stores/serversStore";
import { formatUnknownError } from "../types/errors";
import { AppLayout } from "../components/app-shell/AppLayout";
import { ChromeTabStrip } from "../components/ChromeTabStrip/ChromeTabStrip";
import { useOpenApp } from "../window-system/hooks/useOpenApp";
import { useWindowState } from "../window-system/WindowManagerContext";
import { useBrowserTabs } from "./browser/useBrowserTabs";
import { useBrowserBounds } from "./browser/useBrowserBounds";
import { BrowserNavbar } from "./browser/BrowserNavbar";
import "./BrowserApp.css";

const log = createLogger("BrowserApp");

/** 代理会话信息（Rust 端 ProxySessionInfo） */
interface ProxySessionInfo {
  port: number;
}

interface BrowserAppProps {
  windowId: string;
}

export function BrowserApp({ windowId }: BrowserAppProps) {
  const { t } = useTranslation();
  const { activeServer, servers, connectServer } = useServerManager();
  const openApp = useOpenApp();
  // 睡眠超时（分钟，0=永不；设置 → 浏览器）
  const sleepMinutes = useSettingsStore((s) => s.browserSleepMinutes);

  const [session, setSession] = useState<ProxySessionInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const activeServerIdRef = useRef<string | null>(null);

  const connected = activeServer?.status === "connected";
  const serverId = activeServer?.id ?? null;
  const { tabs, activeId } = useBrowserTabs(connected ? serverId : null);
  const activeTab = tabs.find((tab) => tab.id === activeId) || null;

  // 标签条挂载点：窗口标题栏槽位（Chrome 形态——标题栏即标签条，TextEditor 同款）
  const [titlebarSlot, setTitlebarSlot] = useState<HTMLElement | null>(null);
  useEffect(() => {
    setTitlebarSlot(document.querySelector<HTMLElement>(`[data-titlebar-slot="${windowId}"]`));
  }, [windowId]);

  // 窗口状态：激活态同步 webview 可见性；几何变化触发 bounds 上报
  const win = useWindowState(windowId);
  const contentRef = useRef<HTMLDivElement>(null);
  useBrowserBounds(
    connected ? serverId : null,
    contentRef,
    { x: win.position.x, y: win.position.y, width: win.size.width, height: win.size.height },
    connected && win.isActive,
  );

  // 打开/恢复会话（幂等）：挂载 + 断线重连后都会触发
  useEffect(() => {
    if (!connected || !serverId) return;
    if (serverId !== activeServerIdRef.current) {
      activeServerIdRef.current = serverId;
      setSession(null);
      setError(null);
    }
    let cancelled = false;
    (async () => {
      try {
        const info = await invoke<ProxySessionInfo>("browser_open", {
          serverId,
          url: null,
          sleepMinutes,
        });
        if (!cancelled) setSession(info);
      } catch (e) {
        if (!cancelled) setError(formatUnknownError(e));
        log.error("浏览会话打开失败:", e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [connected, serverId, sleepMinutes]);

  // 关闭窗口 → 只销毁 webview + Environment（监听保留，重开秒恢复）
  useEffect(() => {
    const sid = activeServerIdRef.current;
    return () => {
      if (sid) {
        invoke("browser_close_window", { serverId: sid })
          .then(() => log.info("浏览器窗口已关闭"))
          .catch((e) => log.error("关闭浏览器窗口失败:", e));
      }
    };
  }, []);

  // 窗口激活态 → webview 可见性（child HWND 无法被 React z-index 遮挡）
  useEffect(() => {
    if (!connected || !serverId) return;
    invoke("browser_set_visible", { serverId, visible: win.isActive }).catch(() => {});
  }, [connected, serverId, win.isActive]);

  // ── 操作 ──────────────────────────────────────────────
  const navigate = useCallback((url: string) => {
    if (serverId) invoke("browser_navigate", { serverId, url, tabId: activeId }).catch((e) => log.error("导航失败:", e));
  }, [serverId, activeId]);
  const back = useCallback(() => {
    if (serverId) invoke("browser_back", { serverId, tabId: activeId }).catch(() => {});
  }, [serverId, activeId]);
  const forward = useCallback(() => {
    if (serverId) invoke("browser_forward", { serverId, tabId: activeId }).catch(() => {});
  }, [serverId, activeId]);
  const reload = useCallback(() => {
    if (serverId) invoke("browser_reload", { serverId, tabId: activeId }).catch(() => {});
  }, [serverId, activeId]);
  const newTab = useCallback(() => {
    if (serverId) invoke("browser_create_tab", { serverId, url: null }).catch((e) => log.error("新建标签失败:", e));
  }, [serverId]);
  const closeTab = useCallback((id: string) => {
    if (serverId) invoke("browser_close_tab", { serverId, tabId: Number(id) }).catch(() => {});
  }, [serverId]);
  const focusTab = useCallback((id: string) => {
    if (serverId) invoke("browser_focus_tab", { serverId, tabId: Number(id) }).catch(() => {});
  }, [serverId]);
  const reorderTab = useCallback((from: number, to: number) => {
    if (serverId) invoke("browser_reorder_tab", { serverId, from, to }).catch(() => {});
  }, [serverId]);

  // ── 标签条（ChromeTabStrip；tab_id number → ChromeTabItem.id string）──
  const renderTabStrip = () => (
    <ChromeTabStrip
      variant="light"
      tabs={tabs.map((tab) => ({
        id: String(tab.id),
        label: tab.title || tab.url || t("browser.newTab"),
        dirty: false,
      }))}
      activeTabId={activeId != null ? String(activeId) : ""}
      onActivate={focusTab}
      onClose={closeTab}
      onAdd={newTab}
      onReorder={reorderTab}
    />
  );

  // ── 导航行 ────────────────────────────────────────────
  const renderNavbar = () => (
    <BrowserNavbar
      activeTab={activeTab}
      onNavigate={navigate}
      onBack={back}
      onForward={forward}
      onReload={reload}
    />
  );

  // ── 未连接：占位提示（复用现有 ConnectionState 组件）──
  if (!connected) {
    const troubledServer =
      servers.find((s) => s.status === "reconnecting") ||
      servers.find((s) => s.status === "error") ||
      null;

    if (troubledServer) {
      return (
        <div className="br-app">
          <div className="br-empty">
            {troubledServer.status === "reconnecting" ? (
              <ReconnectingState serverName={troubledServer.name || troubledServer.host} />
            ) : (
              <ConnectionErrorState
                {...getServerErrorInfo(troubledServer)}
                onRetry={() => connectServer(troubledServer.id)}
                onOpenSettings={() => openApp("settings")}
              />
            )}
          </div>
        </div>
      );
    }

    return (
      <div className="br-app">
        <div className="br-empty">
          <div className="br-empty-icon">
            <SymbolicIcon name="network-offline" size={32} />
          </div>
          <div className="br-empty-title">{t("browser.notConnectedTitle")}</div>
          <div className="br-empty-desc">{t("browser.notConnectedDesc")}</div>
        </div>
      </div>
    );
  }

  // ── 浏览器本体 ────────────────────────────────────────
  return (
    <div className="br-app">
      {/* 页签 portal 进窗口标题栏（Chrome 形态）；槽位缺失时回退应用内 */}
      {titlebarSlot ? createPortal(renderTabStrip(), titlebarSlot) : renderTabStrip()}
      <AppLayout toolbar={renderNavbar()}>
        {error && (
          <div className="br-error">
            <div className="br-error-title">{t("browser.errorTitle")}</div>
            <div className="br-error-desc">{error}</div>
          </div>
        )}
        <div className="br-content" ref={contentRef}>
          {/* webview child HWND 覆盖于此；本占位仅在 webview 隐藏时可见
              （窗口非激活 / 窗口刚开尚未上报 bounds / 出错） */}
          <div className="br-content-placeholder">
            <SymbolicIcon name="network-offline" size={24} />
            <span>{t("browser.contentPlaceholder")}</span>
          </div>
        </div>
      </AppLayout>
    </div>
  );
}
```

- [ ] **Step 3: BrowserApp.css 重写**

标签条样式由 ChromeTabStrip 自带（`--quirel-*` 令牌，variant="light"），本文件只需导航行 + content 占位 + 错误条：

```css
/* src/apps/BrowserApp.css — 内置浏览器（导航行 + content 占位区；
   标签条 = 窗口标题栏本体，样式由 ChromeTabStrip 提供） */

.br-app {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  background: var(--quirel-window-bg, #fafafa);
}

/* 槽位缺失时的应用内标签条回退：占满顶行（正常形态 portal 进标题栏） */
.br-app > .chrome-tabstrip-row {
  flex-shrink: 0;
}

/* ── 导航行（AppLayout toolbar；Edge/Chrome 第二行）────── */
.br-navbar {
  display: flex;
  align-items: center;
  gap: 4px;
  width: 100%;
  padding: 4px 8px;
}

.br-nav-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--fg-muted, #666);
  cursor: pointer;
  flex-shrink: 0;
}
.br-nav-btn:hover { background: var(--hover-bg, rgba(0, 0, 0, 0.06)); color: var(--fg, #222); }

.br-addressbar {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 10px;
  height: 28px;
  border-radius: 14px;
  background: var(--inset-bg, rgba(0, 0, 0, 0.05));
}

.br-loading-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--quirel-accent-bg, #15aa70);
  animation: br-pulse 1s ease-in-out infinite;
  flex-shrink: 0;
}
@keyframes br-pulse {
  0%, 100% { opacity: 0.3; }
  50% { opacity: 1; }
}

.br-address {
  flex: 1;
  border: none;
  outline: none;
  background: transparent;
  font-size: 12px;
  color: var(--fg, #222);
}
.br-address::placeholder { color: var(--fg-dim, #999); }

/* ── content 占位区（webview child HWND 覆盖其上）────── */
.br-content {
  position: relative;
  flex: 1;
  min-height: 0;
  background: var(--card-bg, #ffffff);
}

.br-content-placeholder {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--fg-dim, #999);
  font-size: 13px;
}

/* ── 错误条 ──────────────────────────────────────────── */
.br-error {
  flex-shrink: 0;
  padding: 8px 12px;
  background: var(--danger-bg, rgba(200, 40, 40, 0.08));
  color: var(--danger-fg, #a22);
  font-size: 12px;
}
.br-error-title { font-weight: 600; margin-bottom: 2px; }

/* ── 未连接占位 ──────────────────────────────────────── */
.br-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--fg-dim, #999);
}
.br-empty-icon { opacity: 0.6; }
.br-empty-title { font-size: 14px; font-weight: 600; color: var(--fg-muted, #666); }
.br-empty-desc { font-size: 12px; }
```

- [ ] **Step 4: init.ts 浏览器窗口尺寸**

```ts
  // Browser - 内置浏览器（WebView2 嵌入，chrome 区 + content 区）
  registry.register({
    id: 'browser',
    titleKey: 'apps.browser.title',
    icon: '🌐',
    defaultSize: { width: 1000, height: 700 },
    minSize: { width: 640, height: 440 },
    allowMultipleInstances: false,
    component: BrowserApp,
    desktopLabelKey: 'apps.browser.desktopLabel',
    dockLabelKey: 'apps.browser.dockLabel',
  });
```

- [ ] **Step 5: 类型检查**

Run：`npx tsc --noEmit`
Expected: 无错误。已知可能的编译点：
- `useWindowState` 返回的字段名（`position/size/isActive`——在 `WindowManagerContext.tsx` 快照结构中确认，若字段名不同以实际为准适配）
- `BrowserApp` 接收 `windowId` prop（`Desktop.tsx` 的 `MemoizedAppContent` 已传 `windowId`）
- `ChromeTabStrip` 的 props 名（`tabs/activeTabId/onActivate/onClose/onAdd/onReorder/variant`，见 `src/components/ChromeTabStrip/ChromeTabStrip.tsx:48-74`，本计划按实际签名编写）

> 提示用户：Task 8 完成，建议 git 提交。

---

### Task 9: settingsStore + 设置页 + i18n

**Files:**
- Modify: `src/stores/settingsStore.ts`
- Modify: `src/apps/Settings.tsx`
- Modify: `src/i18n/locales/zh-CN.ts`、`en-US.ts`

- [ ] **Step 1: settingsStore**

1. `SettingsState` 中 `browserId: string;` 替换为：

```ts
  // 内置浏览器：后台标签睡眠超时（分钟；0 = 永不睡眠）
  browserSleepMinutes: number;
```

2. `SettingsActions` 中 `setBrowserId` 替换为：

```ts
  setBrowserSleepMinutes: (minutes: number) => void;
```

3. `DEFAULT_SETTINGS` 中 `browserId: "auto",` 替换为：

```ts
  // 后台标签睡眠超时（对齐 Edge sleeping tabs 的默认量级）
  browserSleepMinutes: 120,
```

4. actions 中 `setBrowserId` 实现替换为：

```ts
      setBrowserSleepMinutes: (minutes) => {
        set({ browserSleepMinutes: minutes });
      },
```

- [ ] **Step 2: 设置页浏览器分区改造**

`src/apps/Settings.tsx`：

1. 删除 `BrowserInfo` 接口定义（约 27 行）与 browserList 相关代码（约 522-530 行的 state/effect）。
2. `browserIdPref`（约 533 行）改为 `const browserSleepMinutes = useSettingsStore((s) => s.browserSleepMinutes);`。
3. `case "browser":` 分支（约 1108 行）整体替换为：

```tsx
      case "browser": {
        const setBrowserSleepMinutes = useSettingsStore.getState().setBrowserSleepMinutes;
        const sleepOptions = [
          { value: 0, labelKey: "settings.browserSleepNever" },
          { value: 10, labelKey: "settings.browserSleepMinutes10" },
          { value: 30, labelKey: "settings.browserSleepMinutes30" },
          { value: 120, labelKey: "settings.browserSleepMinutes120" },
        ];
        return (
          <div className="st-section">
            <div className="st-section-title text-heading">{t("settings.browserTitle")}</div>

            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.builtInBrowserTitle")}</div>
              <div className="st-option-row">
                <span className="st-option-label text-body">{t("settings.socks5Port")}</span>
                <input
                  type="number"
                  className="st-input"
                  min={1024}
                  max={65535}
                  value={browserProxyPort}
                  onChange={(e) => {
                    const v = parseInt(e.target.value, 10);
                    if (!Number.isNaN(v) && v >= 1024 && v <= 65535) {
                      useSettingsStore.getState().setBrowserProxyPort(v);
                    }
                  }}
                  style={{ width: 100 }}
                />
              </div>
              <div className="st-option-row">
                <span className="st-option-label text-body">{t("settings.browserSleepTitle")}</span>
                <select
                  className="st-select"
                  value={sleepOptions.some((o) => o.value === browserSleepMinutes) ? browserSleepMinutes : 120}
                  onChange={(e) => setBrowserSleepMinutes(parseInt(e.target.value, 10))}
                >
                  {sleepOptions.map((o) => (
                    <option key={o.value} value={o.value}>{t(o.labelKey)}</option>
                  ))}
                </select>
              </div>
              <div className="st-hint">
                {t("settings.browserHint")}
              </div>
            </div>
          </div>
        );
      }
```

- [ ] **Step 3: i18n 词条**

`zh-CN.ts`：

1. `settings` 段：删除 `defaultBrowserTitle`/`browserAuto`/`noBrowsersHint`/`remoteBrowserUse`（若存在）/旧 `browserPortHint`，追加：

```ts
    builtInBrowserTitle: '内置浏览器',
    browserSleepTitle: '后台标签睡眠',
    browserSleepNever: '永不',
    browserSleepMinutes10: '10 分钟后',
    browserSleepMinutes30: '30 分钟后',
    browserSleepMinutes120: '2 小时后',
    browserHint: '网页流量经服务器网络转发（目标网站看到服务器 IP）。后台标签超时后睡眠以释放内存，点击标签即唤醒。\n固定端口使断线重连后自动恢复网络，无需重开（对已开始的会话在下一次启动时生效）。',
```

2. `browser` 段整体替换为：

```ts
  browser: {
    notConnectedTitle: '未连接服务器',
    notConnectedDesc: '连接服务器后，可在内置浏览器中通过服务器网络浏览网页',
    title: '远程浏览',
    errorTitle: '浏览器启动失败',
    newTab: '新建标签页',
    closeTab: '关闭标签页',
    back: '后退',
    forward: '前进',
    reload: '刷新',
    addressPlaceholder: '搜索或输入网址',
    contentPlaceholder: '连接后开始浏览',
  },
```

`en-US.ts` 对应：

```ts
    builtInBrowserTitle: 'Built-in browser',
    browserSleepTitle: 'Sleep background tabs',
    browserSleepNever: 'Never',
    browserSleepMinutes10: 'After 10 minutes',
    browserSleepMinutes30: 'After 30 minutes',
    browserSleepMinutes120: 'After 2 hours',
    browserHint: "Web traffic is forwarded through the server's network (websites see the server's IP). Background tabs sleep after a timeout to free memory; click a tab to wake it.\nA fixed port lets the browser restore its network automatically after a reconnection (takes effect on the next launch for sessions already started)",
```

```ts
  browser: {
    notConnectedTitle: 'Not connected',
    notConnectedDesc: 'Connect to a server to browse the web through its network in the built-in browser',
    title: 'Remote Browser',
    errorTitle: 'Failed to start the browser',
    newTab: 'New tab',
    closeTab: 'Close tab',
    back: 'Back',
    forward: 'Forward',
    reload: 'Reload',
    addressPlaceholder: 'Search or enter address',
    contentPlaceholder: 'Connect to start browsing',
  },
```

同时清理 zh-CN/en-US 中 `browser` 段的旧键（statusRunning/statusStarting/statusStopped/hintIdle/openBrowser/hintSession/openAnother/labelServer/labelBrowser/browserOpened/labelPort）——已无引用。

- [ ] **Step 4: 检查残留引用**

Run：`Grep "browserId|proxy_list_browsers|BrowserInfo" src/`
Expected: 无匹配（若 settingsStore 其他位置引用 browserId——`BrowserApp.tsx` 已在 Task 8 重写不再引用）。

Run：`npx tsc --noEmit`
Expected: 通过。

Run：`npx vitest run`
Expected: 全部 PASS（若 `errors.test.ts` 全集断言未更新会在此暴露）。

> 提示用户：Task 9 完成，建议 git 提交。

---

### Task 10: 全量构建验证 + 体积检查

**Files:** 无新改动（验证任务）

- [ ] **Step 1: Rust 全量测试**

Run（`src-tauri` 目录）：`cargo test`
Expected: 全部 PASS（含 TabBook 13 项）。

Run：`cargo clippy --all-targets`
Expected: 无新增 error（既有 warning 可忽略）。

- [ ] **Step 2: 依赖单版本复验（spec 第 11 节风险项）**

Run：`cargo tree -d -p webview2-com`
Expected: 单版本 0.38.2。

- [ ] **Step 3: 前端构建**

Run：`npm run build`
Expected: tsc + vite 构建成功。

- [ ] **Step 4: Tauri debug 构建（完整链接验证）**

Run（项目根）：`npm run tauri build -- --debug`（或 `cargo build` 在 src-tauri）
Expected: 编译链接成功。
记录 `target/debug/quirel.exe` 体积与 main 分支对比，验证二进制增量 ≈0（webview2-com 复用同版本）。

> 提示用户：代码全部完成，建议 git 提交。进入手动验证。

---

### Task 11: 手动验证清单（webview 无法无头测试，用户执行）

**Files:** 无（验证任务）

启动 `npm run tauri dev`，连接一台服务器后逐项验证（spec 第 10 节）：

- [ ] 1. **代理生效性**：地址栏输入 ip 查询站（如 `ipinfo.io`），出口 IP = 服务器 IP
- [ ] 2. **cookie 持久化**：登录某站点 → 重启 Quirel → 重新打开浏览器 → 登录态保留
- [ ] 3. **断线恢复**：断网 → 页面报错 → 恢复网络重连 → 浏览器窗口不重建，刷新即恢复
- [ ] 4. **多 server 隔离**：两台服务器各开浏览器，登录态互不可见
- [ ] 5. **标签全流程**：新建/切换（页面输入框内容保留）/关闭/睡眠唤醒（设置改 10 分钟验证）
- [ ] 6. **标签拖拽重排**：拖动页签换位后顺序保持（刷新事件后不打回原顺序），重排后激活页签不变
- [ ] 7. **桌面内拖拽/缩放**：BrowserApp 窗口拖动缩放时页面跟随无撕裂；拖 Quirel 主窗口时原子跟随
- [ ] 8. **target=_blank**：点击新窗口链接在同窗口开新 tab
- [ ] 9. **主 UI 不受代理影响**：桌面、其他应用、本地资源正常
- [ ] 10. **窗口遮挡**：BrowserApp 非激活时内容区显示占位（不穿透）；激活恢复
- [ ] 11. **关窗重开**：关闭浏览器窗口 → 重新打开 → 端口不变、登录态保留

---

## 自审记录（Self-Review）

**Spec 覆盖检查：**
- 第 4 节组件职责表 → Task 3/4/5/8 逐项落地 ✓
- 第 5 节不变量（args 替换补默认/user-data-dir per server/端口绑定）→ Task 3 `create_environment` + Task 5 `browser_data_dir`/状态机 ✓
- 第 6 节生命周期五条路径 → Task 5 状态机（start/suspend/陈旧恢复/close_window/stop+exit）✓
- 第 7 节标签行为（切换保留/睡眠/关闭/_blank/新建）→ Task 2 TabBook + Task 4 runtime + Task 8 ChromeTabStrip ✓
- 用户追加要求（2026-10-20）：项目风格 + ChromeTabStrip 标签页 + Edge/Chrome 布局 → Task 8 标题栏即标签条（portal，TextEditor 同款）+ BrowserNavbar 导航行 ✓
- 第 8 节接口面 → Task 6 全部 command + 事件；`browser_open` 补 `sleep_minutes` 参数（设置传入），新增 `browser_create_tab`/`browser_set_visible`（spec 第 4 节 z-order 要求的实现）/`browser_reorder_tab`（ChromeTabStrip 拖拽排序贯通 Rust 事实源）✓
- 第 9 节错误处理 → 新错误码 3 个 + 幂等忽略（set_bounds/close_tab 对不存在 id）✓
- 第 10 节测试 → TabBook 单测（Task 2）+ 手动清单（Task 11）✓

**类型一致性检查：**
- `TabInfo`（Rust serde）与前端 `TabInfo` 字段一致（id/url/title/loading/sleeping）✓
- `TabsPayload`/`TabPageState` 与前端 payload 接口一致 ✓
- `BrowserWindowHost` 方法名与 Task 6 commands 调用一致（navigate/back/forward/reload/create_tab/close_tab/focus_tab/reorder_tab/set_bounds/set_visible/close_all/request_close）✓
- `HostCommand` 变体与 handle_command 分支一一对应 ✓
- ChromeTabItem.id（string）↔ 浏览器 tab_id（number）映射仅发生在 BrowserApp 渲染层与回调（String/Number），hooks 与 Rust 之间始终是 number ✓
- reorder 链路：ChromeTabStrip onReorder(from,to) → browser_reorder_tab → HostCommand::ReorderTab → TabBook.move_tab → emit_tabs（全量事件回 UI）✓
- sleep_minutes 链路：settingsStore.browserSleepMinutes → browser_open 参数 → create_host → BrowserWindowHost::spawn ✓

**已知的实现期适配点（非占位符，均为 API 实测后的微调指引）：**
1. `useWindowState` 快照字段名以 `WindowManagerContext.tsx` 实际为准（Task 8 Step 5）。
2. `RuntimeCtx.cmd_tx` 传递（Task 4 Step 3 注明的修正）。
3. `ProxySession`/`ProxySessionManager` 字段可见性 `pub(crate)`（Task 6 Step 1 注明）。
4. webview2-com 编译期 API 签名若与本计划有出入，以本地 wry 0.55.1 源码为准微调（计划头部路径），改动仅限类型/方法名，不改架构。

**占位符扫描：** 无 TBD/TODO；所有代码步骤均含完整代码。Task 5 Step 4 的 `stores_bridge` 占位 import 已在该步骤内明确删除并以 sleep_minutes 参数链路替代。

**范围检查：** 单一实现计划可承载（Rust 侧 3 文件 + 前端 5 文件），无需拆分。

---

## 执行说明

- 按任务顺序执行；Task 1-6 是 Rust 侧（有依赖顺序），Task 7-9 是前端侧（有依赖顺序），Task 10/11 是验证。
- 每个任务完成后提示用户 git 提交（用户自己操作，AI 不执行 git 命令）。
- 任务 5 与 6 之间存在编译中间态（lib.rs 引用未删除的 `proxy_list_browsers` 时报错）——Task 5 Step 5 已注明，Task 6 一并修复。
