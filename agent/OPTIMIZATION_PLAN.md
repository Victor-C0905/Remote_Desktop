# quireld 发布优化方案

> 日期：2026-07-19
> 状态：待执行
**目标**：满足成熟工具发布的前置条件

---

## 优化分级

### P0 — 发布前必须做

**移除 aws-lc-rs 双加密后端**

当前 `Cargo.toml` 第 17 行：
```toml
rustls = { version = "0.23", features = ["ring"] }
```
未禁用 default-features，导致 rustls 0.23 默认的 `aws-lc-rs` 后端与显式指定的 `ring` 同时编译。

**影响**：
- 构建需要 cmake + C/C++ 工具链，用户 clone 后可能编译失败
- aws-lc-sys 每次完整编译多 2-3 分钟
- 安全审计时出现两个加密后端，产生困惑
- 虽然最终二进制因 LTO 死代码消除不含 aws-lc 代码（已验证），但构建产物多 ~177MB

**改动**（3 行）：
```toml
rustls = { version = "0.23", default-features = false, features = ["ring", "logging", "std"] }
tokio-rustls = { version = "0.26", default-features = false, features = ["ring", "logging"] }
```

---

### P1 — 建议做

**1. tokio 精确特性替代 "full"**

当前：`tokio = { version = "1", features = ["full"] }`（启用全部 15 个特性）

项目实际使用：rt-multi-thread, macros, net, io-util, sync, time。不需要：fs, process, signal, parking_lot, io-std。

```toml
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros", "net", "io-util", "sync", "time"] }
```

**2. quinn 禁用 platform-verifier**

```toml
quinn = { version = "0.11", default-features = false, features = ["rustls-ring", "runtime-tokio"] }
```

移除 `platform-verifier`（拉入 rustls-platform-verifier + 系统证书库）和 `bloom`（额外依赖）。项目自签证书不需要平台验证器。

**3. notify 禁用 macOS 特性**

```toml
notify = { version = "6", default-features = false, features = ["crossbeam-channel"] }
```

移除 `macos_fsevent` 特性（在 Linux 上无用）。

---

### P2 — 锦上添花

**1. sysinfo 禁用默认特性**

```toml
sysinfo = { version = "0.32", default-features = false, features = ["system"] }
```

移除 component, disk, network, user, multithread(rayon) 等不需要的采集器。仅在 Windows 交叉编译时有意义（省 121MB windows crate），Linux 上影响不大。

**2. Unix-only 依赖移到平台条件块**

```toml
[target.'cfg(unix)'.dependencies]
nix = { version = "0.29", features = ["term", "process", "ioctl", "fs"] }
libc = "0.2"
```

Windows 上 `cargo check` 时不再编译 nix/libc。

**3. tempfile 移到 dev-dependencies**

```toml
[dev-dependencies]
tempfile = "3"
```

当前 tempfile 在 `[dependencies]` 中，但它是测试专用库，不应进入 release 二进制（虽然 LTO 会消除，但依赖图不干净）。

---

## 不建议做

| 优化 | 原因 |
|------|------|
| panic = "abort" | 节省 480KB，但失去 panic 时的栈展开能力，不利于线上排查 |
| opt-level = "z" | 优化体积而非速度，对网络服务端不合适 |
| UPX 压缩 | 启动时解压消耗内存，且部分杀毒软件误报 |
| 统一重复依赖版本 | getrandom/rand/thiserror 的版本分裂由上游引起，无法在不 fork 的情况下解决 |

---

## 最终 Cargo.toml

```toml
[package]
name = "quireld"
version = "0.1.0"
edition = "2021"
description = "Quirel Control — 远程 Agent 服务端"

[[bin]]
name = "agent"
path = "src/main.rs"

[dependencies]
# 异步运行时
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros", "net", "io-util", "sync", "time"] }

# QUIC 协议栈
quinn = { version = "0.11", default-features = false, features = ["rustls-ring", "runtime-tokio"] }
rustls = { version = "0.23", default-features = false, features = ["ring", "logging", "std"] }
tokio-rustls = { version = "0.26", default-features = false, features = ["ring", "logging"] }
rcgen = "0.12"
rustls-pemfile = "2"

# WebSocket 备选协议
tokio-tungstenite = "0.21"
futures-util = "0.3"

# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# 日志
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# 工具
bytes = "1"
uuid = { version = "1", features = ["v4"] }
lazy_static = "1.4"
anyhow = "1"
thiserror = "1"
clap = { version = "4", features = ["derive"] }

# 配置热重载
notify = { version = "6", default-features = false, features = ["crossbeam-channel"] }

# 系统信息
sysinfo = { version = "0.32", default-features = false, features = ["system"] }

# 用户信息
whoami = "1.4"

# 时间格式化
chrono = "0.4"

# Unix-only 依赖
[target.'cfg(unix)'.dependencies]
nix = { version = "0.29", features = ["term", "process", "ioctl", "fs"] }
libc = "0.2"

[dev-dependencies]
tempfile = "3"

[profile.release]
opt-level = 3
lto = true
strip = true
codegen-units = 1
```

---

## 预期效果

| 指标 | 改动前 | 改动后 | 说明 |
|------|--------|--------|------|
| 构建工具链 | Rust + cmake + C/C++ | 仅 Rust | P0 移除 aws-lc-sys |
| clean build 时间 | ~5 min | ~2 min | P0 主导 |
| Release 二进制 | 5.32 MB | ~5.0 MB | 变化不大 |
| 依赖数量 | 245 crate / 387 条目 | ~200 crate / ~280 条目 | P0+P1 |
| cmake 依赖 | 需要 | 不需要 | P0 |

二进制体积变化不大（LTO 已经消除了 aws-lc 的死代码），主要改善在**构建体验**和**依赖整洁度**。

---

## 部署方式

发布时只需分发两个文件：

```
agent          ~5 MB    二进制
quireld.toml     <1 KB    配置模板
```

首次运行自动生成 cert.pem / key.pem。目标机器需要 glibc 2.17+（CentOS 7+ / Ubuntu 16.04+）。

```bash
# 部署
scp agent quireld.toml user@server:~/.local/bin/
ssh user@server
chmod +x ~/.local/bin/agent
agent --config ~/.local/bin/quireld.toml
```
