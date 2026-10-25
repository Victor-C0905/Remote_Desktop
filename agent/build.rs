fn main() {
    // 跳过 proto 编译（Windows 上的临时问题）
    // TODO: 在 Linux 环境中恢复 proto 编译
    println!("cargo:rerun-if-changed=protocol/agent.proto");

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
