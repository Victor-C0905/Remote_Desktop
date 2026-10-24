//! 主窗口全屏（F11，浏览器语义）
//!
//! 主体走 tao（tauri 底层）的全屏机制 `set_fullscreen(Borderless)`——它在
//! Windows 上是样式级处理：移除 WS_CAPTION/WS_THICKFRAME（边缘 resize
//! 命中区随样式消失）、保存并恢复 WINDOWPLACEMENT（尺寸/位置/最大化状态
//! 自动还原）。不做任何功能开关式补丁。
//!
//! 仅补两处框架不覆盖的边界：
//! - 任务栏：Explorer 对「前台无边框窗口铺满显示器」会自动隐藏任务栏；
//!   SW_HIDE 兜底（透明窗口等边缘场景下 Explorer 判定可能不成立），退出
//!   还原，无害
//! - 键盘焦点：全屏切换激活顶层窗口后，焦点可能悬在 WebView2 内部窗口，
//!   DOM keydown 收不到——显式归还主 UI webview
//!
//! 状态双轨：全屏标志自维护于 FullscreenState（拖拽裁决用，与 toggle
//! 写入同一来源）；tauri is_fullscreen 仅作查询参考。

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use tauri::{AppHandle, Emitter, Manager, State};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, GWLP_WNDPROC, SC_CLOSE, SetWindowLongPtrW, WM_NCLBUTTONDOWN,
    WM_SYSCOMMAND,
};

/* ── WndProc 子类：全屏期间在消息层物理拦截系统移动/缩放 ───────────
 *
 * 移动循环（modal loop）在 Windows 上的启动入口只有两个：WM_NCLBUTTONDOWN
 * （命中 caption/边缘）与 WM_SYSCOMMAND（SC_MOVE/SC_SIZE 系统命令）。
 * 在子类过程里吞掉这两个入口，循环根本无法启动——零抖动，且与移动源的
 * 具体身份无关（样式层干净仍能拖，说明有未知命中路径，但入口必经其二）。
 * 事件层拉回（lib.rs on_window_event）保留为兜底，防程序性连续移动。 */

/// 拦截开关（toggle 进入置 true / 退出置 false；子类常驻）
static BLOCK_WINDOW_MOVE: AtomicBool = AtomicBool::new(false);
/// 原窗口过程（0 = 子类未安装；安装一次永久复用）
static ORIGINAL_WNDPROC: AtomicIsize = AtomicIsize::new(0);

/// 子类窗口过程：全屏期间吞掉系统移动/缩放的启动消息，其余全部转发
unsafe extern "system" fn fullscreen_block_proc(
    hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
) -> LRESULT {
    if BLOCK_WINDOW_MOVE.load(Ordering::Relaxed) {
        match msg {
            // NC 区按下（caption/边缘命中）：移动/缩放循环的鼠标入口
            WM_NCLBUTTONDOWN => return LRESULT(0),
            // 系统菜单命令（SC_MOVE/SC_SIZE/SC_MAXIMIZE 等 modal 入口）；
            // 保留 SC_CLOSE——Alt+F4 关闭在全屏下仍可用
            WM_SYSCOMMAND if (wparam.0 & 0xFFF0) != SC_CLOSE as usize => {
                return LRESULT(0);
            }
            _ => {}
        }
    }
    let orig = ORIGINAL_WNDPROC.load(Ordering::Relaxed);
    if orig != 0 {
        // isize 还原为函数指针（SetWindowLongPtrW 返回的原 proc）
        let prev: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT =
            unsafe { std::mem::transmute(orig) };
        return unsafe { CallWindowProcW(Some(prev), hwnd, msg, wparam, lparam) };
    }
    LRESULT(0) // 不可达：安装先于 BLOCK=true
}

/// 禁用窗口的 DWM 过渡动画（最大化/还原/最小化的滑动效果）。
/// 全屏切换经过 unmaximize/maximize/resize 多步窗口操作，每步都会触发
/// 系统滑动动画造成拖沓感——一次设置永久生效（幂等可重复调用）。
fn disable_dwm_transitions(hwnd: HWND) {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
    unsafe {
        let disable: i32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            &disable as *const i32 as *const std::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// 安装子类（幂等：只装一次，此后行为由 BLOCK_WINDOW_MOVE 开关控制，
/// 避免反复换 proc 的竞态）
fn install_move_block(hwnd: HWND) {
    if ORIGINAL_WNDPROC.load(Ordering::Relaxed) != 0 {
        return;
    }
    unsafe {
        let orig = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, fullscreen_block_proc as usize as isize);
        ORIGINAL_WNDPROC.store(orig, Ordering::Relaxed);
        tracing::info!("[Fullscreen] WndProc 子类已安装: orig={orig:#x}");
    }
    // 首次进入全屏时顺带禁用 DWM 过渡（幂等）
    disable_dwm_transitions(hwnd);
}

/// 全屏布尔标志（自维护，与 tauri is_fullscreen 双轨）。
/// start_os_drag 的裁决源用它而非 is_fullscreen——二者理应同源，但
/// 实测出现过状态读取不一致，拖拽裁决必须与 toggle 写入严格同一来源。
#[derive(Default)]
pub struct FullscreenState {
    active: std::sync::Mutex<bool>,
    /// 进入全屏前的 resizable（退出时还原）
    was_resizable: std::sync::Mutex<bool>,
    /// 全屏锁定 rect（x,y 物理坐标 + w,h 物理尺寸）。Some=全屏中，窗口
    /// Moved/Resized 事件与它不符时立即拉回——移动源在系统/框架深层
    /// （样式层干净、前端无调用仍能拖），物理锁死是唯一可靠防线。
    /// （pub(crate)：lib.rs 的 on_window_event 锁位回调读取）
    pub(crate) locked: std::sync::Mutex<Option<(i32, i32, u32, u32)>>,
    /// 进入全屏前是否最大化（退出时还原）。最大化窗口直接进全屏时，
    /// WebView2 的 bounds 重算会走 maximize 分支而滞后（底部露出任务栏
    /// 高度的透明条）——先 unmaximize 再全屏，退出时按此还原。
    was_maximized: std::sync::Mutex<bool>,
}

/// F11 切换全屏；返回切换后的状态（true=已进入全屏）
#[tauri::command]
pub fn toggle_fullscreen(app: AppHandle, state: State<FullscreenState>) -> Result<bool, String> {
    let win = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;

    let was = win.is_fullscreen().map_err(|e| e.to_string())?;
    if was {
        // 解锁必须先于 set_fullscreen(false)：tao 恢复 placement 会同步
        // 触发 Moved/Resized 事件，若 locked 仍为 Some，on_window_event 的
        // 拦截分支会把正在退出的窗口拉回全屏尺寸（尺寸恢复失败）
        *state.locked.lock().unwrap() = None;
        BLOCK_WINDOW_MOVE.store(false, Ordering::Relaxed);
        win.set_fullscreen(false).map_err(|e| format!("退出全屏失败: {e}"))?;
        set_taskbars_visible(true);
        // 还原最大化（进入前是最大化则重新放大；tao 保存的 placement 是
        // unmaximize 后的 normal rect，不会自动回最大化）
        if *state.was_maximized.lock().unwrap() {
            let _ = win.maximize();
        }
        // 还原 resizable（进入时保存，通常为 true）
        let was_res = *state.was_resizable.lock().unwrap();
        let _ = win.set_resizable(was_res);
    } else {
        // 先装子类并开启拦截，再做任何窗口操作——样式/位置变化期间已处于
        // 保护之下（NC/SC 移动入口在消息层被吞，modal loop 无法启动）
        if let Ok(raw) = win.hwnd() {
            install_move_block(HWND(raw.0 as *mut _));
        }
        BLOCK_WINDOW_MOVE.store(true, Ordering::Relaxed);
        // 全屏期间必须关 resizable：tao 无边框窗口的 resize 命中区由内部
        // RESIZABLE 标志判定（不查 WS_THICKFRAME 样式）——全屏移除样式后
        // 边缘命中区仍在，贴边拖动会 resize/移动窗口（NC 消息不进前端，
        // 前端层无法拦截）。Chrome F11 同语义：全屏窗口无 resize 边。
        let was_res = win.is_resizable().unwrap_or(true);
        let _ = win.set_resizable(false);
        *state.was_resizable.lock().unwrap() = was_res;
        // 最大化 → 全屏前先退出最大化：zoomed 状态位与全屏 rect 并存时
        // WebView2 bounds 重算走 maximize 分支，底部露出任务栏高度的透明条
        let was_max = win.is_maximized().unwrap_or(false);
        if was_max {
            let _ = win.unmaximize();
        }
        *state.was_maximized.lock().unwrap() = was_max;
        win.set_fullscreen(true).map_err(|e| format!("进入全屏失败: {e}"))?;
        set_taskbars_visible(false);
        // 诊断：全屏建立后 dump 窗口样式——THICKFRAME 残留即系统 resize
        // 命中区的直接证据（WS_THICKFRAME=0x40000 WS_CAPTION=0xC00000）
        if let Ok(raw) = win.hwnd() {
            use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongW, GWL_STYLE};
            let style =
                unsafe { GetWindowLongW(HWND(raw.0 as *mut _), GWL_STYLE) };
            tracing::info!(
                "[Fullscreen] 进入后 style={style:#010x} THICKFRAME={} CAPTION={}",
                style & 0x0004_0000 != 0,
                style & 0x00C0_0000 != 0
            );
        }
        // 锁定全屏 rect（on_window_event Moved/Resized 拉回的基准）
        if let (Ok(pos), Ok(size)) = (win.outer_position(), win.inner_size()) {
            *state.locked.lock().unwrap() =
                Some((pos.x, pos.y, size.width, size.height));
        }
    }

    // 键盘焦点归还（进入/退出都可能有焦点悬空）
    if let Ok(raw) = win.hwnd() {
        restore_webview_keyboard_focus(HWND(raw.0 as *mut _));
    }

    let now = !was;
    *state.active.lock().unwrap() = now;
    let _ = app.emit("fullscreen://changed", now);
    tracing::info!("[Fullscreen] toggle 完成: now={now}");
    Ok(now)
}

/// TopBar 拖拽的唯一入口：全屏态由自维护 FullscreenState 裁决。
#[tauri::command]
pub fn start_os_drag(app: AppHandle, state: State<FullscreenState>) -> Result<(), String> {
    let fullscreen = *state.active.lock().unwrap();
    tracing::info!("[Fullscreen] start_os_drag 调用: fullscreen={fullscreen}");
    if fullscreen {
        tracing::info!("[Fullscreen] 全屏态拒绝拖拽");
        return Ok(());
    }
    let win = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    win.start_dragging().map_err(|e| format!("启动拖拽失败: {e}"))
}

/// 查询当前是否全屏（前端初始化用，读 tao 同源状态）
#[tauri::command]
pub fn is_fullscreen(app: AppHandle) -> Result<bool, String> {
    let win = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    win.is_fullscreen().map_err(|e| e.to_string())
}

/// 隐藏/显示 Windows 任务栏（主屏 Shell_TrayWnd + 各副屏 Shell_SecondaryTrayWnd）。
/// Explorer 全屏判定的兜底；EnumWindows 全量枚举比 FindWindowW 可靠。
/// 回调内直接 ShowWindow 安全（枚举线程 = 主窗口所在 UI 线程）。
fn set_taskbars_visible(visible: bool) {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, ShowWindow, SHOW_WINDOW_CMD, SW_HIDE, SW_SHOW,
    };

    thread_local! {
        /// 传给回调的 ShowWindow 命令（i32: SW_HIDE=0 / SW_SHOW=5）
        static SHOW_CMD: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
    }

    unsafe extern "system" fn on_window(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        let mut buf = [0u16; 32];
        let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
        let class = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
        if class == "Shell_TrayWnd" || class == "Shell_SecondaryTrayWnd" {
            let cmd = SHOW_CMD.with(|c| c.get());
            unsafe { ShowWindow(hwnd, SHOW_WINDOW_CMD(cmd)) };
        }
        BOOL(1)
    }

    SHOW_CMD.with(|c| c.set(if visible { SW_SHOW.0 } else { SW_HIDE.0 }));
    unsafe {
        let _ = EnumWindows(Some(on_window), LPARAM(0));
    }
}

/// 把键盘焦点还给主 UI webview（WRY_WEBVIEW wrapper，主窗口直接 child）。
/// FindWindowExW 按类名定位（不递归，仅直接 child 层）；找不到时静默
/// （焦点悬空只影响快捷键，不致命）。
fn restore_webview_keyboard_focus(parent: HWND) {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::FindWindowExW;

    unsafe {
        let class = HSTRING::from("WRY_WEBVIEW");
        if let Ok(child) =
            FindWindowExW(Some(parent), None, PCWSTR(class.as_ptr()), PCWSTR::null())
        {
            if !child.is_invalid() {
                let _ = SetFocus(Some(child));
            }
        }
    }
}
