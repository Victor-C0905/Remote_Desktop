fn main() {
    tauri_build::build();

    // Track icon assets so that `cargo tauri dev` / `cargo tauri build`
    // re-embed the logo when any icon file changes. Tauri v2's
    // tauri_build::build() only registers tauri.conf.json as a rebuild
    // dependency by default, so swapping icon bytes alone never triggered
    // a recompile (the old binary kept the previous icon).
    println!("cargo:rerun-if-changed=icons/32x32.png");
    println!("cargo:rerun-if-changed=icons/128x128.png");
    println!("cargo:rerun-if-changed=icons/128x128@2x.png");
    println!("cargo:rerun-if-changed=icons/icon.png");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/icon.icns");
    println!("cargo:rerun-if-changed=icons/Square30x30Logo.png");
    println!("cargo:rerun-if-changed=icons/Square44x44Logo.png");
    println!("cargo:rerun-if-changed=icons/Square71x71Logo.png");
    println!("cargo:rerun-if-changed=icons/Square89x89Logo.png");
    println!("cargo:rerun-if-changed=icons/Square107x107Logo.png");
    println!("cargo:rerun-if-changed=icons/Square142x142Logo.png");
    println!("cargo:rerun-if-changed=icons/Square150x150Logo.png");
    println!("cargo:rerun-if-changed=icons/Square284x284Logo.png");
    println!("cargo:rerun-if-changed=icons/Square310x310Logo.png");
    println!("cargo:rerun-if-changed=icons/Square89x89Logo.png");

    // 构建信息（分支@git describe）：区分测试构建，与版本号无关（AGENTS.md 第 5 节）
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    };
    // --tags：release 脚本打的是轻量 tag，不带 --tags 时 describe 只认注解 tag，
    // 正式发版后构建信息仍指向旧注解 tag 基点
    let describe = git(&["describe", "--always", "--dirty", "--tags"]).unwrap_or_else(|| "unknown".to_string());
    let build_info = match git(&["rev-parse", "--abbrev-ref", "HEAD"]) {
        Some(b) => format!("{b}@{describe}"),
        None => describe,
    };
    println!("cargo:rustc-env=QUIREL_BUILD_INFO={build_info}");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs/heads");
}
