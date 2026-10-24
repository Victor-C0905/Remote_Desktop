//! 浏览器窗口宿主：标签状态机 + runtime 线程 + 跨线程句柄
//!
//! 分层：
//! - TabBook：纯数据状态机（本文件可单测的部分，无 COM）
//! - Runtime：STA 线程主循环（执行 TabBook 决策 + COM 操作）
//! - BrowserWindowHost：跨线程句柄（tauri command 的调用入口）
//!
//! 事件流：COM 回调 → 命令 channel 回流 → 主循环统一处理 → emit 前端

use tauri::Emitter;
use std::sync::Arc;
use super::browser_settings::{BrowserSettings, SearchEngine};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::take_pwstr;
use webview2_com::{
    DocumentTitleChangedEventHandler, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler, SourceChangedEventHandler,
};
use windows::core::PWSTR;
use windows::Win32::Foundation::RECT;

/* ── 常量 ─────────────────────────────────────────────── */

/// 初始 webview 内容区尺寸（物理像素，宽x高；runtime 启动兜底值，
/// 首个 tab 创建后由前端 SetBounds 立即覆盖）
const INITIAL_TAB_SIZE_PX: (i32, i32) = (800, 600);
/// 主循环兜底轮询间隔（毫秒）：等待侧以 MsgWaitForMultipleObjectsEx 事件驱动
/// （命令 SetEvent / STA 消息即醒），此超时仅为活性兜底（防唤醒句柄异常睡死），
/// 不是常规唤醒路径
const MAIN_LOOP_FALLBACK_MS: u32 = 1000;
/// 睡眠扫描间隔（秒）：定期触发 ScanSleep，超时未活跃的后台 tab 销毁 webview 释放内存
const SLEEP_SCAN_INTERVAL_SECS: u64 = 60;

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
        // 修正：必须在 retain 移除之前捕获位置，否则恒为 None（原计划代码缺陷）
        let pos = self.order.iter().position(|&x| x == id);
        self.order.retain(|&x| x != id);

        if self.active == Some(id) {
            self.active = match pos {
                // 有前一个 → 相邻前一个（p-1 位置必然存在）
                Some(p) if p > 0 => self.order.get(p - 1).copied(),
                // 无前一个（原第 0 个）→ 后一个（元素已前移到 p 位置）
                Some(p) => self.order.get(p).copied(),
                None => None,
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
    ///
    /// 引擎（chrome-tabs.js animateTabMove）dragEnd 上报的 destinationIndex
    /// 是被拖页签的「最终索引」：左移插到目标位之前、右移插到目标位之后，
    /// 两个方向都等价于标准 arrayMove（先移除再插到 to）。
    pub(crate) fn move_tab(&mut self, from: usize, to: usize) -> bool {
        if from == to || from >= self.order.len() || to >= self.order.len() {
            return false;
        }
        let moved = self.order.remove(from);
        self.order.insert(to, moved);
        true
    }
}

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

/// 跨线程命令发送端：send 成功后 SetEvent 唤醒 runtime 主循环。
///
/// 原主循环以 recv_timeout(50ms) 超时驱动消息泵，COM 事件（NavigationCompleted/
/// DocumentTitleChanged 等）经 STA 消息派发要等下一次泵才回流，完成信号最坏
/// 晚 50ms（表现为页签转圈/地址栏更新慢一拍）。事件唤醒后命令即时到达，
/// 等待侧用 MsgWaitForMultipleObjectsEx（见 runtime_main）。
pub(crate) struct CmdSender {
    tx: std::sync::mpsc::Sender<HostCommand>,
    /// auto-reset 唤醒事件句柄（内核对象，句柄值进程内跨线程有效）
    wake: windows::Win32::Foundation::HANDLE,
}
// HANDLE 内含裸指针不满足 Send，但内核句柄值进程内跨线程使用合法（仅 SetEvent）
unsafe impl Send for CmdSender {}
// Arc<BrowserWindowHost> 要求 Sync：&CmdSender 的并发使用仅 tx.send（std 线程安全）
// 与 SetEvent（内核级线程安全调用），共享只读使用合法
unsafe impl Sync for CmdSender {}

impl Clone for CmdSender {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone(), wake: self.wake }
    }
}

impl CmdSender {
    pub(crate) fn send(
        &self,
        cmd: HostCommand,
    ) -> Result<(), std::sync::mpsc::SendError<HostCommand>> {
        let r = self.tx.send(cmd);
        if r.is_ok() {
            // SetEvent 失败（句柄已关/通道将断）无碍：主循环有兜底超时收割，
            // 静默即可，不制造噪音
            unsafe {
                let _ = windows::Win32::System::Threading::SetEvent(self.wake);
            }
        }
        r
    }
}

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
    /// 重发全量标签列表（browser://tabs，无回复）。会话恢复后前端 tabs 已被
    /// 清空，且此后的增量 page-state 事件在空列表上合并不生效，需全量回灌
    RefreshTabs,
    /// 默认搜索引擎变更（设置持久化后由 browser_set_settings 转发，运行中即时生效）
    SetSearchEngine { engine: SearchEngine },
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
    cmd_tx: CmdSender,
}

impl BrowserWindowHost {
    /// 请求窗口关闭（销毁全部 webview + drop Environment + 结束线程）。
    /// 监听不受影响（由 ProxySession 管理）。幂等：通道已断时静默。
    pub(crate) fn request_close(&self) {
        let _ = self.cmd_tx.send(HostCommand::CloseAll { reply: None });
    }

    /// 请求重发全量标签列表（browser://tabs）。fire-and-forget：通道已断时静默。
    pub(crate) fn refresh_tabs(&self) {
        let _ = self.cmd_tx.send(HostCommand::RefreshTabs);
    }

    /// 默认搜索引擎变更（browser_set_settings 持久化后转发，运行中即时生效）。
    /// fire-and-forget：通道已断时静默。
    pub(crate) fn set_search_engine(&self, engine: SearchEngine) {
        if let Err(e) = self.cmd_tx.send(HostCommand::SetSearchEngine { engine }) {
            tracing::warn!("[Browser] SetSearchEngine 指令发送失败（runtime 已退出）: engine={engine:?} err={e}");
        }
    }

    /// 异步等待 CloseAll 完成（显式关闭路径用；Drop 路径用 fire-and-forget）
    pub async fn close_all(&self) -> Result<(), crate::errors::AppError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(HostCommand::CloseAll { reply: Some(tx) })
            .map_err(|_| AppError::new(codes::PROXY_BROWSER_WINDOW_CLOSED))?;
        // 3s 超时兜底：销毁大量 webview 可能较慢，但不应无限挂起调用方；
        // 超时只代表"未等到确认"，runtime 仍会自行完成关闭
        match tokio::time::timeout(std::time::Duration::from_secs(3), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AppError::new(codes::PROXY_BROWSER_WINDOW_CLOSED)),
            Err(_) => {
                tracing::warn!("[Browser] close_all 等待超时(3s)，runtime 可能仍在销毁 webview");
                Err(crate::errors::AppError::new(crate::errors::codes::OPERATION_FAILED)
                    .with_detail("close_all timeout"))
            }
        }
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

/* ── Runtime（STA 线程内状态与主循环）─────────────────── */

use super::webview_host::{self, WebviewHandle};
use crate::errors::{codes, AppError};

/// 跨线程传递的 HWND 包装：HWND 内含裸指针不满足 Send，但句柄值本身只是
/// 进程内的整数标识；此处仅跨线程移交该值，窗口创建仍发生在 runtime STA 线程
#[derive(Debug, Clone, Copy)]
pub(crate) struct SendHwnd(windows::Win32::Foundation::HWND);
unsafe impl Send for SendHwnd {}

pub(crate) struct RuntimeCtx {
    pub server_id: String,
    pub app: tauri::AppHandle,
    pub user_data_dir: std::path::PathBuf,
    pub proxy_port: u16,
    /// 主窗口句柄（经 SendHwnd 包装跨线程移交）
    pub parent_hwnd: SendHwnd,
    pub sleep_timeout: std::time::Duration,
    pub first_url: String,
    /// 浏览器设置快照（spawn 时刻）：搜索引擎参与地址栏导航判定；
    /// incognito 仅用于退出时清理即弃目录（目录本身由 mod.rs 选定）
    pub search_engine: SearchEngine,
    pub incognito: bool,
    /// 命令发送端（RuntimeState 与 COM 事件订阅闭包共用；plan 勘误：
    /// Receiver 无 clone_tx，改由 ctx 携带 cmd_tx 传入线程内）
    pub cmd_tx: CmdSender,
}

struct RuntimeState {
    ctx: RuntimeCtx,
    env: Option<ICoreWebView2Environment>,
    webviews: std::collections::HashMap<u64, WebviewHandle>,
    book: TabBook,
    /// 地址栏搜索词使用的默认搜索引擎（可经 SetSearchEngine 运行中切换）
    search_engine: SearchEngine,
    /// content 区 bounds（物理像素，相对主窗口客户区）
    rect: RECT,
    /// 窗口激活态（false 时全部 webview 隐藏）
    window_visible: bool,
    cmd_tx: CmdSender,
}

impl RuntimeState {
    fn emit_tabs(&self) {
        let payload = TabsPayload {
            server_id: self.ctx.server_id.clone(),
            active: self.book.active_id(),
            tabs: self.book.list(),
        };
        // emit 失败可安全忽略：窗口刚建/刚关时前端可能尚未监听，且 tabs 为
        // 全量事件，后续任何结构变更都会补发，warn 只会制造噪音
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
            // 同上：page-state 为增量事件，前端未就绪时丢一帧无害（下次事件补齐）
            let _ = self.ctx.app.emit("browser://page-state", payload);
        }
    }

    /// 创建 webview 并注册页面事件（新 tab / 唤醒共用）
    fn create_webview(&mut self, tab_id: u64, url: &str, activate: bool) -> Result<(), String> {
        let env = self.env.as_ref()
            .ok_or_else(|| "Environment 未就绪".to_string())?;
        let visible = activate && self.window_visible;
        let wv = webview_host::create_controller(self.ctx.parent_hwnd.0, env, self.rect, visible)?;
        // 先订阅页面事件再导航（对齐 wry 的 init_webview 顺序）：WebView2 事件
        // 经 STA 消息泵派发，若首个导航的 NavigationStarting/SourceChanged 在
        // Navigate 调用内被同步派发，后注册将整体丢失——表现为首个页面
        // loading/标题/URL 永不更新（地址栏与标签条停滞在 about:blank）
        unsafe { subscribe_page_events(&wv, tab_id, self.cmd_tx.clone())?; }
        webview_host::navigate(&wv, url)?;
        self.webviews.insert(tab_id, wv);
        Ok(())
    }
}

/// 订阅页面事件：COM 回调把事件作为命令发回主循环（避免闭包借用 runtime 状态）。
/// 事件订阅 token 无需 remove_*：controller/webview 释放时订阅随 COM 对象销毁。
unsafe fn subscribe_page_events(
    wv: &WebviewHandle,
    tab_id: u64,
    tx: CmdSender,
) -> Result<(), String> {
    wv.webview.add_NavigationStarting(
        &NavigationStartingEventHandler::create(Box::new({
            let tx = tx.clone();
            move |_, _| {
                let _ = tx.send(HostCommand::EvNavStart { tab_id });
                Ok(())
            }
        })),
        &mut 0i64,
    ).map_err(|e| format!("订阅 NavigationStarting 失败: {e}"))?;

    wv.webview.add_NavigationCompleted(
        &NavigationCompletedEventHandler::create(Box::new({
            let tx = tx.clone();
            move |_, _| {
                let _ = tx.send(HostCommand::EvNavDone { tab_id });
                Ok(())
            }
        })),
        &mut 0i64,
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
        &mut 0i64,
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
        &mut 0i64,
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
        &mut 0i64,
    ).map_err(|e| format!("订阅 NewWindowRequested 失败: {e}"))?;

    Ok(())
}

/// 地址规范化：URL 直达 / 域名形态补 https:// / 其余作为搜索词交给默认搜索引擎
fn normalize_url(raw: &str, engine: SearchEngine) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return "about:blank".into();
    }
    if t.starts_with("http://") || t.starts_with("https://") || t.starts_with("about:") || t.starts_with("file://") {
        return t.to_string();
    }
    // 域名形态（example.com / localhost:8080）→ 直达；含空格或无点 → 搜索词
    if !t.contains(char::is_whitespace) && looks_like_host(t) {
        return format!("https://{t}");
    }
    engine.search_url(t)
}

/// 粗判域名形态：localhost（可带 :port）直达；其余含点且不以点开头/结尾视为
/// 域名（防御 "..." 类退化输入）。单词新 gTLD（如 "example"）无法离线判定，
/// 从简按搜索词处理。
fn looks_like_host(t: &str) -> bool {
    let lower = t.to_ascii_lowercase();
    if lower == "localhost" || lower.starts_with("localhost:") {
        return true;
    }
    t.contains('.') && !t.starts_with('.') && !t.ends_with('.')
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
                let result = self.do_navigate(tab_id, &normalize_url(&url, self.search_engine));
                if let Err(ref e) = result {
                    tracing::warn!("[Browser] Navigate 执行失败: tab_id={tab_id:?} url={url} err={e}");
                }
                let _ = reply.send(result);
            }
            HostCommand::Back { tab_id, reply } => {
                let result = self.do_history(tab_id, "history.back()");
                if let Err(ref e) = result {
                    tracing::warn!("[Browser] Back 执行失败: tab_id={tab_id:?} err={e}");
                }
                let _ = reply.send(result);
            }
            HostCommand::Forward { tab_id, reply } => {
                let result = self.do_history(tab_id, "history.forward()");
                if let Err(ref e) = result {
                    tracing::warn!("[Browser] Forward 执行失败: tab_id={tab_id:?} err={e}");
                }
                let _ = reply.send(result);
            }
            HostCommand::Reload { tab_id, reply } => {
                let result = self.do_history(tab_id, "location.reload()");
                if let Err(ref e) = result {
                    tracing::warn!("[Browser] Reload 执行失败: tab_id={tab_id:?} err={e}");
                }
                let _ = reply.send(result);
            }
            HostCommand::CreateTab { url, activate, reply } => {
                let result = self.do_create_tab(&normalize_url(&url, self.search_engine), activate);
                if let Err(ref e) = result {
                    tracing::warn!("[Browser] CreateTab 执行失败: url={url} activate={activate} err={e}");
                }
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
                // info 级：bounds 是否到达并应用是"webview 不铺满/占位常显"
                // 问题的关键证据链一环（与 SetVisible 同理，此前不可观测）
                tracing::info!(
                    "[Browser] SetBounds 应用: rect=({x},{y},{w},{h}) active_tab={:?} webviews={}",
                    self.book.active_id(),
                    self.webviews.len()
                );
                // 立即应用到当前可见的激活 webview
                match self.book.active_id().and_then(|id| self.webviews.get(&id)) {
                    Some(wv) => {
                        webview_host::set_bounds(wv, self.rect);
                        // 诊断：枚举主窗口 child HWND 实证 webview 真实落点
                        // （限前 3 次，闭环后随其他诊断日志一并移除）
                        webview_host::dump_child_windows(self.ctx.parent_hwnd.0);
                    }
                    // 无可应用对象：bounds 已记录（后续创建的 webview 会用它），
                    // 但"指令到达 runtime 却无处安放"是排查不铺满问题的关键信号
                    None => tracing::warn!(
                        "[Browser] SetBounds 无可应用 webview（激活 tab 不存在或 webview 未建）: rect=({x},{y},{w},{h})"
                    ),
                }
            }
            HostCommand::SetVisible { visible } => {
                // info 级：webview 忽隐忽现/占位常显问题的关键证据链一环
                // （此前 debug 级被运行日志吞掉，无法判断隐藏/显示是否到达）
                tracing::info!(
                    "[Browser] SetVisible: visible={visible} active_tab={:?} webviews={}",
                    self.book.active_id(),
                    self.webviews.len()
                );
                self.window_visible = visible;
                if let Some(id) = self.book.active_id() {
                    if let Some(wv) = self.webviews.get(&id) {
                        // 显示前先按最近记录的 rect 归位：bounds 与可见性是两条
                        // IPC，可见性先到时 webview 会在旧位置闪现再跳到新位置
                        // （拖拽松手"画面原地闪过再补偿"的根因）；先归位再显示，
                        // 保证出现即最新 bounds。SetBounds 幂等，重复应用无害
                        if visible {
                            webview_host::set_bounds(wv, self.rect);
                        }
                        webview_host::set_visible(wv, visible);
                    }
                }
            }
            HostCommand::RefreshTabs => {
                self.emit_tabs();
            }
            HostCommand::SetSearchEngine { engine } => {
                tracing::info!("[Browser] 默认搜索引擎切换: {:?} → {:?}", self.search_engine, engine);
                self.search_engine = engine;
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
                let _ = self.do_create_tab(&normalize_url(&url, self.search_engine), true);
            }
        }
    }

    fn do_navigate(&mut self, tab_id: Option<u64>, url: &str) -> Result<(), AppError> {
        // 目标缺失是"地址栏回车无反应"的最可疑静默点（此前仅回错误码，
        // runtime 侧零日志），显式区分无激活 tab 与 webview 不在册
        let Some((id, _)) = self.target(tab_id) else {
            tracing::warn!(
                "[Browser] 导航目标缺失（拒绝导航）: tab_id={tab_id:?} active={:?} webviews={} url={url}",
                self.book.active_id(),
                self.webviews.len()
            );
            return Err(AppError::new(codes::PROXY_BROWSER_WINDOW_CLOSED));
        };
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
        let Some((id, wv)) = self.target(tab_id) else {
            tracing::warn!(
                "[Browser] 历史操作目标缺失: tab_id={tab_id:?} active={:?} webviews={} script={script}",
                self.book.active_id(),
                self.webviews.len()
            );
            return Err(AppError::new(codes::PROXY_BROWSER_WINDOW_CLOSED));
        };
        // 修正：wv 借用源自 target 对 self 的整体共享借用，须先用完 wv
        // 再动 book——若先 book.touch 再 execute_script 会与可变借用重叠（E0502）
        webview_host::execute_script(wv, script);
        self.book.touch(id, std::time::Instant::now());
        Ok(())
    }

    fn do_create_tab(&mut self, url: &str, activate: bool) -> Result<u64, AppError> {
        self.env.as_ref().ok_or_else(|| AppError::new(codes::PROXY_BROWSER_ENV_FAILED))?;
        // 修正：须在 create_tab 之前捕获旧激活 id——create_tab(url, true) 已把
        // active 指向新 tab，之后再取 active_id() 只会拿到新 id，
        // 旧 webview 将永远可见（计划原文的隐藏逻辑成了死代码）
        let old = if activate { self.book.active_id() } else { None };
        let id = self.book.create_tab(url, activate);
        if activate {
            // 旧激活 webview 隐藏
            if let Some(old) = old.filter(|old| *old != id) {
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
            if let Err(e) = self.do_wake(tab_id, &url) {
                tracing::warn!("[Browser] 点击睡眠 tab 唤醒失败: tab_id={tab_id} err={e}");
            }
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
                // 若睡的是激活 tab：激活转移语义太重（close 后重建不可取），
                // 简化为激活保留在睡眠 tab 上，前端点击时唤醒（do_focus_tab 已处理）
                self.emit_tabs();
            }
            TabAction::Wake(id) => {
                if let Some(meta) = self.book.meta(id) {
                    let url = meta.url.clone();
                    if let Err(e) = self.do_wake(id, &url) {
                        tracing::warn!("[Browser] 扫描自动唤醒失败: tab_id={id} err={e}");
                    }
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

    // ctx 即将 move 进 state：无痕清理所需字段先行克隆
    let incognito = ctx.incognito;
    let profile_dir = ctx.user_data_dir.clone();

    let env = match webview_host::create_environment(&ctx.user_data_dir, ctx.proxy_port) {
        Ok(env) => env,
        Err(e) => {
            // 此前仅回传错误（Rust 端零日志），用户只能靠前端 console 定位
            tracing::error!(
                "[Browser] WebView2 Environment 创建失败（runtime 未启动）: server_id={} user_data_dir={} port={} err={e}",
                ctx.server_id, ctx.user_data_dir.display(), ctx.proxy_port
            );
            let _ = init_tx.send(Err(e));
            return;
        }
    };

    let mut state = RuntimeState {
        // plan 勘误：Receiver 无 clone_tx，cmd_tx 由 ctx 携带（须先 clone 再 move ctx）
        cmd_tx: ctx.cmd_tx.clone(),
        // ctx 按值收尾：Copy 字段必须先于 move 读取
        search_engine: ctx.search_engine,
        ctx,
        env: Some(env),
        webviews: std::collections::HashMap::new(),
        book: TabBook::new(),
        rect: RECT {
            left: 0,
            top: 0,
            right: INITIAL_TAB_SIZE_PX.0,
            bottom: INITIAL_TAB_SIZE_PX.1,
        },
        window_visible: true,
    };

    // 首个 tab（先克隆 URL：&mut self 调用不能同时借用 self.ctx.first_url）
    let first_url = state.ctx.first_url.clone();
    let first = state.do_create_tab(&first_url, true);
    if let Err(e) = first {
        let detail = e.detail.unwrap_or_default();
        tracing::error!(
            "[Browser] 首个标签页创建失败（runtime 未启动，疑似 Controller 链路故障）: server_id={} rect={:?} err={detail}",
            state.ctx.server_id, state.rect
        );
        let _ = init_tx.send(Err(format!("创建首个标签页失败: {detail}")));
        return;
    }

    let _ = init_tx.send(Ok(()));
    tracing::info!(
        "[Browser] runtime 就绪: server_id={} port={} 首tab_url={} rect={:?} 窗口可见={}",
        state.ctx.server_id, state.ctx.proxy_port, first_url, state.rect, state.window_visible
    );

    // 唤醒事件句柄（spawn 侧创建、随 CmdSender 分发）：主循环等待与收尾关闭用
    let wake = state.ctx.cmd_tx.wake;

    // 主循环：事件驱动 + 消息泵。顺序 = 收割命令 → 泵消息 → 阻塞等待。
    // 等待侧 MsgWaitForMultipleObjectsEx：命令 SetEvent / 任何 STA 消息即醒，
    // 消除原 recv_timeout(50ms) tick 粒度（COM 事件回流最坏晚 50ms，表现为
    // 页签转圈/地址栏更新慢一拍）；空闲时不轮询，MAIN_LOOP_FALLBACK_MS 仅为
    // 活性兜底
    loop {
        // 1) 收割全部积压命令（CloseAll 处理后退出；通道断开即退出）
        let mut exit = false;
        loop {
            match cmd_rx.try_recv() {
                Ok(cmd) => {
                    let is_close = matches!(cmd, HostCommand::CloseAll { .. });
                    state.handle_command(cmd);
                    if is_close {
                        // 对齐原语义：CloseAll 后泵一次再退（controller.Close
                        // 派发的收尾消息有机会走完）
                        webview_host::pump_messages();
                        exit = true;
                        break;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    exit = true;
                    break;
                }
            }
        }
        if exit {
            break;
        }
        // 2) 泵消息：COM 事件派发（handler 内 send 命令 + SetEvent 即时唤醒下轮）
        webview_host::pump_messages();
        // 3) 阻塞等待：唤醒事件 / STA 消息到达 / 兜底超时。QS_SENDMESSAGE 显式
        //    并入（跨线程 SendMessage 派发须能在等待中被唤醒，QS_ALLINPUT 不含它）；
        //    MWMO_INPUTAVAILABLE 保证队列已有消息时立即返回而非等新消息。
        //    auto-reset 事件在返回 WAIT_OBJECT_0 时自动复位，多命令批量到达只
        //    唤醒一轮、由收割循环一次吃尽
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                MsgWaitForMultipleObjectsEx, MWMO_INPUTAVAILABLE, QS_ALLINPUT, QS_SENDMESSAGE,
            };
            let _ = MsgWaitForMultipleObjectsEx(
                Some(&[wake]),
                MAIN_LOOP_FALLBACK_MS,
                QS_ALLINPUT | QS_SENDMESSAGE,
                MWMO_INPUTAVAILABLE,
            );
        }
    }
    tracing::info!("[Browser] runtime 线程退出: server_id={}", state.ctx.server_id);

    // 显式 drop：无痕目录必须在 Environment（随 state 释放）之后才能删除
    drop(state);

    // 收尾顺序关键：先断命令通道（此后任何 send 必失败，CmdSender 不再 SetEvent），
    // 再关唤醒句柄——消除「send 成功但句柄已关、句柄值被系统复用误触他对象」的竞态
    drop(cmd_rx);
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(wake);
    }

    // 无痕目录即弃即焚：WebView2 子进程可能短暂持有目录锁，重试数次尽力删除；
    // 失败留待下次开启前 create_host 兜底清理
    if incognito {
        for attempt in 0..5u32 {
            match std::fs::remove_dir_all(&profile_dir) {
                Ok(()) => {
                    tracing::info!("[Browser] 无痕数据目录已清理: {}", profile_dir.display());
                    break;
                }
                Err(e) if attempt < 4 => {
                    tracing::debug!("[Browser] 无痕目录清理重试 {}: {e}", attempt + 1);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                Err(e) => {
                    tracing::warn!("[Browser] 无痕目录清理失败（下次开启前兜底清理）: {e}");
                }
            }
        }
    }
}

impl BrowserWindowHost {
    /// 启动浏览器 runtime 线程并等待初始化完成（Environment + 首个 tab）。
    /// 失败时线程已自行清理，返回 Err。
    ///
    /// parent_hwnd 为主窗口句柄整数值（而非 HWND）：HWND 含裸指针不满足
    /// Send，作为 async fn 参数会驻留生成器状态机跨 await，使调用链上的
    /// tauri command future 非 Send；线程内还原为 HWND（SendHwnd 包装）。
    pub async fn spawn(
        app: tauri::AppHandle,
        server_id: String,
        user_data_dir: std::path::PathBuf,
        proxy_port: u16,
        parent_hwnd: isize,
        sleep_timeout: std::time::Duration,
        first_url: String,
        settings: BrowserSettings,
    ) -> Result<Arc<BrowserWindowHost>, AppError> {
        let (cmd_tx_raw, cmd_rx) = std::sync::mpsc::channel::<HostCommand>();
        let (init_tx, init_rx) = tokio::sync::oneshot::channel();

        // auto-reset 唤醒事件：命令 send 成功后 SetEvent 即时唤醒 runtime 主循环
        // （CreateEventW 无 COM 依赖，调用线程创建即可）。thread::spawn 失败路径
        // 需自行关闭句柄；线程成功启动后由 runtime_main 收尾关闭
        let wake = unsafe { windows::Win32::System::Threading::CreateEventW(None, false, false, None) }
            .map_err(|e| AppError::new(codes::PROXY_BROWSER_WINDOW_FAILED)
                .with_detail(format!("创建主循环唤醒事件失败: {e}")))?;
        let cmd_tx = CmdSender { tx: cmd_tx_raw, wake };

        let ctx = RuntimeCtx {
            server_id: server_id.clone(),
            app,
            cmd_tx: cmd_tx.clone(),
            user_data_dir,
            proxy_port,
            parent_hwnd: SendHwnd(windows::Win32::Foundation::HWND(parent_hwnd as *mut _)),
            sleep_timeout,
            first_url,
            search_engine: settings.search_engine,
            incognito: settings.incognito,
        };

        if std::thread::Builder::new()
            .name(format!("browser-runtime-{server_id}"))
            .spawn(move || runtime_main(ctx, cmd_rx, init_tx))
            .is_err()
        {
            // 线程未启动：无人收尾，关闭唤醒事件防句柄泄漏
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(wake);
            }
            return Err(AppError::new(codes::PROXY_BROWSER_WINDOW_FAILED)
                .with_detail("spawn thread: browser runtime"));
        }

        init_rx.await
            .map_err(|_| AppError::new(codes::PROXY_BROWSER_WINDOW_FAILED).with_detail("runtime init dropped"))?
            .map_err(|e| AppError::new(codes::PROXY_BROWSER_ENV_FAILED).with_detail(e))?;

        let host = Arc::new(BrowserWindowHost {
            server_id,
            proxy_port,
            cmd_tx,
        });

        // 睡眠扫描任务：定期触发（host 死亡后 send 失败自动退出）
        let scan_tx = host.cmd_tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(SLEEP_SCAN_INTERVAL_SECS));
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
        if let Err(e) = self.cmd_tx.send(HostCommand::SetBounds { x, y, w, h }) {
            // runtime 已退出：指令无人消费，前端表现为"上报了但不铺满"
            tracing::warn!("[Browser] SetBounds 指令发送失败（runtime 已退出）: rect=({x},{y},{w},{h}) err={e}");
        }
    }
    pub fn set_visible(&self, visible: bool) {
        if let Err(e) = self.cmd_tx.send(HostCommand::SetVisible { visible }) {
            tracing::warn!("[Browser] SetVisible 指令发送失败（runtime 已退出）: visible={visible} err={e}");
        }
    }
}

/* ── Tauri commands（前端接口面，spec 第 8 节）─────────── */

use super::ProxySessionManager;
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
    // guard 须绑定为局部变量：链式调用作为块尾表达式时临时 MutexGuard
    // 存活到外层 let 语句结束，晚于 sessions 局部变量的块尾 drop（E0597）
    let host = {
        let sessions: State<ProxySessionManager> = app.state();
        let guard = sessions.sessions.lock().unwrap();
        guard
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
///
/// url 未指定 → 默认首页 = 当前设置搜索引擎的官网（settings.json）
#[tauri::command]
pub async fn browser_create_tab(
    server_id: String,
    url: Option<String>,
    app: AppHandle,
) -> Result<u64, AppError> {
    let url = match url {
        Some(u) => u,
        None => {
            let dir = super::browser_data_dir(&app, &server_id)?;
            super::browser_settings::load(&dir).search_engine.home_url()
        }
    };
    host_of(&app, &server_id)?.create_tab(url, true).await
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
        // host 未入册（browser_open 未返回/已关窗口）：指令被静默丢弃，
        // 此日志是判断"bounds 指令根本没到 runtime"的关键证据
        tracing::debug!("[Browser] browser_set_bounds 丢弃（host 未就绪或已关闭）: server_id={server_id}");
        return Ok(()); // 窗口已关：布局事件迟到，静默忽略
    };
    let scale = app
        .get_webview_window("main")
        .and_then(|w| w.scale_factor().ok())
        .unwrap_or(1.0);
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
    } else {
        tracing::debug!("[Browser] browser_set_visible 丢弃（host 未就绪或已关闭）: server_id={server_id} visible={visible}");
    }
    Ok(())
}

/// 读取浏览器设置（随服务器持久化于该 server 浏览器数据目录 settings.json）
#[tauri::command]
pub fn browser_get_settings(server_id: String, app: AppHandle) -> Result<BrowserSettings, AppError> {
    let dir = super::browser_data_dir(&app, &server_id)?;
    Ok(super::browser_settings::load(&dir))
}

/// 保存浏览器设置：持久化 + 运行中 host 即时生效
///
/// - 搜索引擎：经 SetSearchEngine 转发 runtime，地址栏导航即时采用新引擎
/// - 无痕切换：Environment 代理参数/数据目录创建后不可变，由前端关窗重开
///   会话生效（browser_close_window 保留监听 → browser_open 走原端口重建）
#[tauri::command]
pub async fn browser_set_settings(
    server_id: String,
    settings: BrowserSettings,
    app: AppHandle,
) -> Result<(), AppError> {
    let dir = super::browser_data_dir(&app, &server_id)?;
    super::browser_settings::save(&dir, &settings)?;
    if let Some(host) = host_of(&app, &server_id).ok() {
        host.set_search_engine(settings.search_engine);
    }
    tracing::info!(
        "[Browser] 设置已保存: server_id={server_id} engine={:?} incognito={}",
        settings.search_engine, settings.incognito
    );
    Ok(())
}

#[cfg(test)]
mod tab_book_tests {
    use super::*;

    #[test]
    fn 地址规范化_无scheme域名补https() {
        assert_eq!(normalize_url("example.com", SearchEngine::Google), "https://example.com");
        assert_eq!(normalize_url("  docs.rs  ", SearchEngine::Bing), "https://docs.rs");
        assert_eq!(normalize_url("localhost:3000", SearchEngine::Google), "https://localhost:3000");
    }

    #[test]
    fn 地址规范化_空输入与带scheme原样() {
        assert_eq!(normalize_url("", SearchEngine::Google), "about:blank");
        assert_eq!(normalize_url("   ", SearchEngine::Google), "about:blank");
        assert_eq!(normalize_url("http://a.b/c?d=1", SearchEngine::Google), "http://a.b/c?d=1");
        assert_eq!(normalize_url("about:blank", SearchEngine::Bing), "about:blank");
    }

    #[test]
    fn 地址规范化_含空格视为搜索词() {
        assert_eq!(
            normalize_url("rust async book", SearchEngine::Google),
            "https://www.google.com/search?q=rust%20async%20book"
        );
        assert_eq!(
            normalize_url("铁锈 战争", SearchEngine::Bing),
            "https://www.bing.com/search?q=%E9%93%81%E9%94%88%20%E6%88%98%E4%BA%89"
        );
    }

    #[test]
    fn 地址规范化_无点单词视为搜索词() {
        assert_eq!(
            normalize_url("rustlang", SearchEngine::Bing),
            "https://www.bing.com/search?q=rustlang"
        );
        // 退化输入（纯点）不误判为域名
        assert_eq!(
            normalize_url("...", SearchEngine::Google),
            "https://www.google.com/search?q=..."
        );
    }

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
        // 后台创建不推进激活指针，旧激活仍是 a
        let b = book.create_tab("https://b.example", false);
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
    fn 关闭激活的中间标签转移到前一个() {
        // 控制器补充：验证"相邻前一个"而非"最后一个"语义
        let mut book = TabBook::new();
        let a = book.create_tab("https://a.example", true);
        let b = book.create_tab("https://b.example", true);
        let c = book.create_tab("https://c.example", true);
        // 激活中间的 b（旧激活 c），关闭 b → 转移到 a（相邻前一个），而不是 c
        assert_eq!(book.set_active(b), Some(c));
        assert_eq!(book.close_tab(b), Some(TabAction::Destroy(b)));
        assert_eq!(book.active_id(), Some(a));
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
        book.touch(a, later - std::time::Duration::from_secs(110));
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
        // 前进：a 从 0 移到 2（destinationIndex = 落点最终索引）→ b c a
        assert!(book.move_tab(0, 2));
        let ids: Vec<u64> = book.list().iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![b, c, a]);
        // 后退：a 从 2 移到 0 → a b c
        assert!(book.move_tab(2, 0));
        let ids: Vec<u64> = book.list().iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![a, b, c]);
        // 非法索引静默返回 false（ChromeTabStrip 的 from/to 恒在界内，防御）
        assert!(!book.move_tab(3, 0));
        assert!(!book.move_tab(0, 3));
        // 无位移返回 false（避免无意义 emit）
        assert!(!book.move_tab(1, 1));
    }
}
