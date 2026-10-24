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
// pwstr 是私有模块，take_pwstr / CoreWebView2EnvironmentOptions / 各 CompletedHandler
// 均由 webview2-com crate 根重导出（pub use pwstr::* / options::* / callback::*）
use webview2_com::take_pwstr;
use webview2_com::{
    CoreWebView2EnvironmentOptions, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, ExecuteScriptCompletedHandler,
};
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, PM_REMOVE, TranslateMessage, MSG,
};

/// WebView2 控制器 + 页面接口句柄
pub(crate) struct WebviewHandle {
    pub controller: ICoreWebView2Controller,
    pub webview: ICoreWebView2,
    /// webview 顶层容器 HWND（parent 的直接 child）。它与主 UI 的 WRY_WEBVIEW
    /// 平级竞争 z-order——实测创建后默认落在主 UI webview 之下（EnumChildWindows
    /// 顺序实证），必须显示时显式提顶，否则页面永远被桌面 DOM 盖住（占位常显）。
    pub hwnd: HWND,
}

/// 直接 child HWND 快照（差集定位新建的 webview 顶层窗口）。
/// 回调与查询经模块级 static 共享（嵌套 fn 的 static 是独立变量）。
static CHILD_SNAPSHOT: std::sync::Mutex<Vec<isize>> = std::sync::Mutex::new(Vec::new());

fn direct_children(parent: HWND) -> Vec<isize> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetParent};

    CHILD_SNAPSHOT.lock().unwrap().clear();
    // 回调无法捕获 parent：经线程绑定传递（STA 线程串行调用，无竞争）
    PARENT_FOR_ENUM.with(|p| p.set(parent.0 as isize));

    unsafe extern "system" fn on_child(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        let parent = HWND(PARENT_FOR_ENUM.with(|p| p.get()) as *mut _);
        // 仅直接 child（EnumChildWindows 会枚举全部后代）
        if unsafe { GetParent(hwnd) }.map(|p| p == parent).unwrap_or(false) {
            CHILD_SNAPSHOT.lock().unwrap().push(hwnd.0 as isize);
        }
        BOOL(1)
    }

    unsafe {
        let _ = EnumChildWindows(Some(parent), Some(on_child), LPARAM(0));
    }
    CHILD_SNAPSHOT.lock().unwrap().clone()
}

thread_local! {
    static PARENT_FOR_ENUM: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
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
///
/// DNS-over-HTTPS（secure）为性能关键项：Chromium 对 socks5:// 代理永远
/// 先本地解析 DNS（socks5h 不支持，crbug/33442），国内污染域名解析出
/// 假 IP 后经隧道去连 → 连接失败/重试 → 明显延迟。强制 DoH 后查询本身
/// 也走代理进隧道（dns.google 经隧道可达），拿真 IP 再建连。系统浏览器
/// 挂同一隧道明显快于内置 webview 的实测差异，主因即在此。
pub(crate) fn create_environment(
    user_data_dir: &Path,
    socks_port: u16,
) -> Result<ICoreWebView2Environment, String> {
    let args = format!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
         --proxy-server=socks5://127.0.0.1:{} \
         --dns-over-https-mode=secure \
         --dns-over-https-templates=https://dns.google/dns-query",
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

    // wait_with_pump 返回双层 Result：外层是泵等待失败（webview2_com::Error），
    // 内层是 handler 转发的 WebView2 HRESULT 失败（windows::core::Error）
    webview2_com::wait_with_pump(rx)
        .map_err(|e| format!("等待 WebView2 环境就绪失败: {e}"))?
        .map_err(|e| format!("创建 WebView2 环境失败: {e}"))
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

    // 创建前快照：用于差集定位新建的 webview 顶层 HWND
    let before = direct_children(parent);

    unsafe {
        env.CreateCoreWebView2Controller(parent, &handler)
            .map_err(|e| format!("创建 WebView2 控制器失败: {e}"))?;
    }
    // 同上：外层泵等待失败 / 内层 WebView2 HRESULT 失败，分别转译
    let controller = webview2_com::wait_with_pump(rx)
        .map_err(|e| format!("等待 WebView2 控制器就绪失败: {e}"))?
        .map_err(|e| format!("创建 WebView2 控制器失败: {e}"))?;

    let webview = unsafe { controller.CoreWebView2() }
        .map_err(|e| format!("获取 WebView2 页面接口失败: {e}"))?;

    unsafe {
        controller.SetBounds(bounds).map_err(|e| format!("设置 bounds 失败: {e}"))?;
        controller.SetIsVisible(visible).map_err(|e| format!("设置可见性失败: {e}"))?;
    }

    // 差集定位 webview 顶层 HWND（WebView2 未提供 Controller→HWND 的接口）
    let hwnd = direct_children(parent)
        .into_iter()
        .find(|h| !before.contains(h))
        .map(|h| HWND(h as *mut _))
        .ok_or_else(|| "未找到新建 webview 窗口（差集为空）".to_string())?;

    let handle = WebviewHandle { controller, webview, hwnd };
    // 初始 bounds 的圆角 region（此后每次 set_bounds 随尺寸重设）
    apply_round_region(&handle, bounds.right - bounds.left, bounds.bottom - bounds.top);
    // 创建即可见（首 tab / wake 重建）时立即提顶：新窗口默认落在主 UI
    // 的 WRY_WEBVIEW 之下，不提顶则页面被桌面 DOM 盖住
    if visible {
        raise(&handle);
    }
    Ok(handle)
}

/// 提顶 webview 窗口（不移动不改尺寸不抢焦点）。
/// webview 顶层 HWND 与主 UI 的 WRY_WEBVIEW 平级：创建后默认在主 UI 之下
/// （EnumChildWindows z-order 实证），不提顶则页面永远被桌面 DOM 盖住。
fn raise(wv: &WebviewHandle) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };
    unsafe {
        // hwndinsertafter=None 即 HWND_TOP（API 内部 zeroed）
        if let Err(e) = SetWindowPos(
            wv.hwnd,
            None,
            0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        ) {
            tracing::warn!("[Browser] webview 提顶失败: err={e}");
        }
    }
}

/// 应用 bounds（物理像素，相对主窗口客户区）
pub(crate) fn set_bounds(wv: &WebviewHandle, bounds: RECT) {
    unsafe {
        // SetBounds 失败会被此处吞掉——不加日志则"webview 不铺满"无从定位
        // （此前 `let _ =` 静默，webview 永远停留在创建时的初始尺寸）
        if let Err(e) = wv.controller.SetBounds(bounds) {
            tracing::error!(
                "[Browser] controller.SetBounds 失败: rect=({},{},{},{}) err={e}",
                bounds.left, bounds.top, bounds.right, bounds.bottom
            );
        }
    }
    apply_round_region(wv, bounds.right - bounds.left, bounds.bottom - bounds.top);
}

/// webview 内容卡片圆角（逻辑 px）：与前端 WEBVIEW_EDGE_INSET / --radius-md
/// 同值，内缩边距与卡片圆角同心，视觉成一组
const WEBVIEW_CORNER_RADIUS_LOGICAL: i32 = 12;

/// 给 webview 顶层窗口设置圆角 region。
/// child HWND 不受 DOM 圆角/overflow 裁剪（airspace），窗口 region 是
/// windowed hosting 下唯一的圆角手段；region 外的四角点击穿透到下层
/// DOM（窗口背景），与内缩边缘的行为一致。SetWindowRgn 成功后 region
/// 归系统所有，无需 DeleteObject；bounds 尺寸变化时需重新应用。
fn apply_round_region(wv: &WebviewHandle, width: i32, height: i32) {
    use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
    use windows::Win32::UI::HiDpi::GetDpiForWindow;

    unsafe {
        // region 是窗口自身坐标（物理 px）；半径按窗口 DPI 换算
        // （GetDpiForWindow 失败返回 0，clamp 到 96 保底）
        let dpi = GetDpiForWindow(wv.hwnd).max(96);
        let diameter = WEBVIEW_CORNER_RADIUS_LOGICAL * 2 * dpi as i32 / 96;
        let rgn = CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter);
        if rgn.0.is_null() {
            tracing::warn!("[Browser] CreateRoundRectRgn 失败（region 为空）");
            return;
        }
        // 返回 0 即失败（窗口无效等）；非 0 成功后 region 归系统所有，无需 DeleteObject
        if SetWindowRgn(wv.hwnd, Some(rgn), true) == 0 {
            tracing::warn!("[Browser] webview 圆角 region 设置失败");
        }
    }
}

/// 设置可见性（切 tab / 窗口激活态同步）
pub(crate) fn set_visible(wv: &WebviewHandle, visible: bool) {
    unsafe {
        if let Err(e) = wv.controller.SetIsVisible(visible) {
            tracing::error!("[Browser] controller.SetIsVisible({visible}) 失败: err={e}");
        }
    }
    // 显示时提顶：防主 UI 的 WRY_WEBVIEW 在窗口激活等时机重新盖回
    if visible {
        raise(wv);
    }
}

/// 诊断缓冲：on_child 回调与 dump_child_windows 共用（嵌套 fn 的 static
/// 是独立变量，回调写入外层读不到，必须模块级共享）
static DIAG_CHILD_LINES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// 诊断：枚举 parent 下全部 child HWND，输出类名与屏幕矩形——实证 WebView2
/// 窗口真实落点（SetBounds/SetIsVisible 均返回成功但屏幕不可见的最终证据）。
/// 前 3 次调用生效，问题闭环后随其他诊断日志一并移除。
pub(crate) fn dump_child_windows(parent: HWND) {
    use windows::core::BOOL;
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetClassNameW, GetWindowRect};

    static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 3 {
        return;
    }
    DIAG_CHILD_LINES.lock().unwrap().clear();

    unsafe extern "system" fn on_child(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        let mut name = [0u16; 64];
        let n = unsafe { GetClassNameW(hwnd, &mut name) }.max(0) as usize;
        let cls = String::from_utf16_lossy(&name[..n.min(63)]);
        let mut rect = RECT::default();
        let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
        DIAG_CHILD_LINES.lock().unwrap().push(format!(
            "{hwnd:?} class={cls} screen_rect=({},{},{},{})",
            rect.left, rect.top, rect.right, rect.bottom
        ));
        BOOL(1)
    }

    unsafe {
        let _ = EnumChildWindows(Some(parent), Some(on_child), LPARAM(0));
    }
    let lines = DIAG_CHILD_LINES.lock().unwrap();
    if lines.is_empty() {
        tracing::info!("[Browser][诊断] 主窗口下无 child HWND——webview 未挂在主窗口！");
    } else {
        for l in lines.iter() {
            tracing::info!("[Browser][诊断] child: {l}");
        }
    }
}

/// 将键盘焦点移入页面（切 tab 时）
pub(crate) fn move_focus(wv: &WebviewHandle) {
    unsafe {
        if let Err(e) = wv.controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) {
            tracing::warn!("[Browser] controller.MoveFocus 失败: err={e}");
        }
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
        if let Err(e) = wv.webview.ExecuteScript(&HSTRING::from(script), &handler) {
            tracing::warn!("[Browser] ExecuteScript 失败: script={script} err={e}");
        }
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
