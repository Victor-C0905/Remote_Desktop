// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;

fn print_version() {
    // 版本号编译期读取（AGENTS 第 4 节版本号硬编码禁令）；构建信息由 build.rs 注入
    println!(
        "quirel {} ({})",
        env!("CARGO_PKG_VERSION"),
        env!("QUIREL_BUILD_INFO")
    );
    let _ = std::io::stdout().flush();
}

fn main() {
    // --version：输出版本与构建信息后退出。release 是 GUI 子系统（无控制台），
    // 需先附着父进程控制台，println 才有去处
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::System::Console::{AttachConsole, FreeConsole, ATTACH_PARENT_PROCESS};
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
            print_version();
            let _ = FreeConsole();
        }
        #[cfg(not(windows))]
        print_version();
        return;
    }
    quirel_lib::run()
}
