fn main() {
    // 跳过 proto 编译（Windows 上的临时问题）
    // TODO: 在 Linux 环境中恢复 proto 编译
    println!("cargo:rerun-if-changed=protocol/agent.proto");
}