fn main() {
    // 只在 .proto 文件存在时才编译（避免在迁移阶段报错）
    let proto_file = std::path::Path::new("protocol/agent.proto");
    if proto_file.exists() {
        // 配置生成的代码输出位置
        prost_build::Config::new()
            .out_dir("src/protocol")
            .compile_protos(&["protocol/agent.proto"], &["protocol/"])
            .expect("Failed to compile protos");

        // prost-build 会生成 agent.rs,我们需要将其重命名为 generated.rs
        let generated = std::path::Path::new("src/protocol/agent.rs");
        let target = std::path::Path::new("src/protocol/generated.rs");
        if generated.exists() {
            std::fs::rename(generated, target)
                .expect("Failed to rename generated file");
        }
    }
}