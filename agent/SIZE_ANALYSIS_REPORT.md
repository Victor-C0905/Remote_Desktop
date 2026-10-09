# quireld 体积分析最终报告

> 分析日期：2026-07-19
> 项目：quireld v0.1.0
> 源码规模：3,760 行 Rust（16 个文件）

---

## 一、当前状况

| 维度 | 当前值 | 说明 |
|------|--------|------|
| Release 二进制 | **5.32 MB** | ELF 64-bit, stripped, 动态链接 |
| target/ 目录 | **2.0 GB** | debug 1.6GB + release 423MB |
| 依赖总数 | 245 个唯一 crate | 387 条目（含重复版本） |
| 动态库依赖 | libc, libgcc_s, libm | 仅 3 个系统库 |

### 二进制 ELF 段分布

| 段 | 大小 | 占比 | 说明 |
|----|------|------|------|
| .text | 4,244 KB | 76.1% | 机器码（核心） |
| .rodata | 476 KB | 8.5% | 常量/字符串 |
| .eh_frame | 337 KB | 6.0% | 异常展开表 |
| .rela.dyn | 178 KB | 3.2% | 重定位条目 |
| .gcc_except_table | 145 KB | 2.6% | C++ 异常表 |
| .data.rel.ro | 123 KB | 2.2% | 只读重定位数据 |
| 其他 | 72 KB | 1.4% | — |

### target/ 目录构成

| 子目录 | 大小 | 说明 |
|--------|------|------|
| debug/deps/ | 1,200 MB | .rlib + .rmeta + .o（245 个 crate × 多版本） |
| debug/incremental/ | 228 MB | rust-analyzer 增量编译缓存 |
| debug/build/ | 145 MB | build script 输出（aws-lc-sys 占 82MB） |
| release/deps/ | 380 MB | 同上，release 配置 |
| release/build/ | 42 MB | 同上，release 配置 |
| release/agent | 5.3 MB | 最终二进制 |

---

## 二、六大根因

### 根因 1：rustls 双加密后端同时编译（最严重）

**现状**：`Cargo.toml` 第 17 行 `rustls = { version = "0.23", features = ["ring"] }` 未禁用 default-features，导致 rustls 0.23 默认特性 `aws-lc-rs` 与显式指定的 `ring` 同时编译。

**验证**：二进制中 aws-lc 标记为 0（LTO 死代码消除），但构建阶段仍完整编译 aws-lc-sys C 库。

**浪费**：
- debug: aws-lc-sys rlib 18MB + aws-lc-rs rlib 7MB + 4 个 build 目录 82MB + 静态库 30MB = **~137 MB**
- release: aws-lc-sys rlib 10MB + aws-lc-rs rlib 3MB + build 目录 20MB + 静态库 7MB = **~40 MB**
- 额外需要 cmake + C/C++ 工具链

**修复**：
```toml
rustls = { version = "0.23", default-features = false, features = ["ring", "logging", "std"] }
tokio-rustls = { version = "0.26", default-features = false, features = ["ring", "logging"] }
```

### 根因 2：三版本 getrandom + 三版本 windows-sys

| crate | 版本 | 来源 |
|-------|------|------|
| getrandom | v0.2 | ring (旧) |
| getrandom | v0.3 | quinn-proto → fastbloom |
| getrandom | v0.4 | tempfile, uuid (新) |
| windows-sys | v0.48 | notify |
| windows-sys | v0.60 | quinn-udp |
| windows-sys | v0.61 | tokio, clap |
| rand | v0.8 | tungstenite |
| rand | v0.9 | quinn-proto → fastbloom |
| thiserror | v1 | 项目直接依赖 |
| thiserror | v2 | quinn, quinn-proto |

每个版本独立编译为独立的 .rlib + .rmeta + .o，无法共享。

**修复**：大部分由上游版本分裂导致，难以完全消除。升级 tokio-tungstenite 到 0.24+ 可统一 rand 版本。

### 根因 3：tokio "full" 特性过宽

`tokio = { features = ["full"] }` 启用全部 15 个特性。项目实际使用：

| 需要的特性 | 用途 |
|-----------|------|
| rt-multi-thread | async runtime |
| macros | #[tokio::main], tokio::select! |
| net | TcpListener (WebSocket) |
| io-util | AsyncReadExt/AsyncWriteExt |
| sync | Mutex, broadcast |
| time | interval (metrics) |

不需要的：fs, process, signal, parking_lot, io-std, tracing

**浪费**：libtokio.rlib 43MB(debug) / 11MB(release)

**修复**：
```toml
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros", "net", "io-util", "sync", "time"] }
```

### 根因 4：sysinfo 拉入 121MB windows crate

sysinfo 默认特性拉入 `windows v0.57.0`，在 Windows 上编译时产生 **121MB 的 libwindows.rlib**（全项目最大单项）。

项目目标平台是 Linux（pty.rs 全 `#[cfg(unix)]`，有 wsl-build.sh），Windows 上编译这些 API 绑定完全无用。

**修复**：
```toml
# 方案 A：禁用默认特性
sysinfo = { version = "0.32", default-features = false, features = ["system"] }

# 方案 B：仅 Linux 平台依赖
[target.'cfg(target_os = "linux")'.dependencies]
sysinfo = { version = "0.32", default-features = false, features = ["system"] }
```

### 根因 5：平台条件依赖未隔离

`nix` 和 `libc` 是 Unix-only 依赖（pty.rs 全部 `#[cfg(unix)]`），但放在全局 `[dependencies]` 中。在 Windows 上编译时仍被编译（虽然代码不执行）。

**修复**：
```toml
[target.'cfg(unix)'.dependencies]
nix = { version = "0.29", features = ["term", "process", "ioctl", "fs"] }
libc = "0.2"
```

### 根因 6：build-time crate 体积

以下 crate 仅在编译期使用（proc-macro / build script），不进入最终二进制，但仍占用 target/ 空间：

| crate | debug rlib | 性质 |
|-------|-----------|------|
| syn | 13 MB | proc-macro（所有 derive 宏的底层） |
| proc-macro2 | ×15 份 | proc-macro 基础设施 |
| quote | ×14 份 | proc-macro |
| cc | 4 MB | C 编译器封装（aws-lc-sys build script） |
| cmake | — | aws-lc-sys build script |

**修复**：移除 aws-lc-sys 后 cc/cmake 依赖随之消失。

---

## 三、50MB 目标可行性判定

### 判定 1：二进制 — 已达标，可进一步压到 3.8MB

当前 5.32MB，已远低于 50MB。通过以下优化可降至 ~3.8MB：

| 优化 | 节省 | 方法 |
|------|------|------|
| panic = "abort" | ~480 KB | 移除 .eh_frame + .gcc_except_table |
| opt-level = "z" | ~200 KB | 优化代码体积而非速度 |
| 移除 quinn 平台验证器 | ~100 KB | `quinn = { default-features = false, features = ["rustls-ring", "runtime-tokio"] }` |
| 移除 sysinfo rayon | ~50 KB | sysinfo 禁用 multithread |
| UPX 压缩（可选） | ~2.5 MB | 二进制压缩为自解压 |

**预期最优二进制**：~3.8 MB（不压缩）/ ~1.5 MB（UPX 压缩）

### 判定 2：分发包 — 已达标

```
agent              3.8 MB  (优化后二进制)
cert.pem           1.5 KB
key.pem            3.2 KB
quireld.toml         0.8 KB
─────────────────────────
总计               ~3.8 MB
```

### 判定 3：target/ 构建目录 — 无法达标

即使执行全部优化 + `cargo clean` + 仅 release 构建，最低约 180MB：

| 组成 | 优化后大小 | 说明 |
|------|-----------|------|
| release/deps/ | ~120 MB | 200 个 crate 的 .rlib + .rmeta（无法避免） |
| release/build/ | ~15 MB | ring 静态库 + zerocopy 等 build script 输出 |
| release/agent | 4 MB | 最终二进制 |
| release/.fingerprint/ | ~10 MB | 依赖指纹缓存 |
| release/incremental/ | ~30 MB | 增量编译缓存（首次为 0） |
| **合计** | **~180 MB** | |

**原因**：Rust 的编译模型决定每个 crate 独立编译为 .rlib（包含元数据 + 目标码 + 调试信息），即使最终二进制很小，中间产物仍需完整保留以支持增量编译。245 个依赖 × 平均 0.5MB/crate = ~120MB 是不可压缩的下限。

---

## 四、最终优化 Cargo.toml

```toml
[dependencies]
# 异步运行时 — 精确特性替代 "full"
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros", "net", "io-util", "sync", "time"] }

# QUIC — 禁用默认特性，只用 ring
quinn = { version = "0.11", default-features = false, features = ["rustls-ring", "runtime-tokio"] }

# TLS — 关键修复：禁用 aws-lc-rs 默认后端
rustls = { version = "0.23", default-features = false, features = ["ring", "logging", "std"] }
tokio-rustls = { version = "0.26", default-features = false, features = ["ring", "logging"] }
rcgen = "0.12"
rustls-pemfile = "2"

# WebSocket
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
chrono = "0.4"

# 配置热重载 — 禁用 macOS fsevent
notify = { version = "6", default-features = false, features = ["crossbeam-channel"] }

# 系统信息 — 禁用默认特性，仅 system
sysinfo = { version = "0.32", default-features = false, features = ["system"] }

# 用户信息
whoami = "1.4"

# Unix-only 依赖 — 平台条件
[target.'cfg(unix)'.dependencies]
nix = { version = "0.29", features = ["term", "process", "ioctl", "fs"] }
libc = "0.2"

[dev-dependencies]
tempfile = "3"

[profile.release]
opt-level = "z"       # 优化体积
lto = true
strip = true
codegen-units = 1
panic = "abort"        # 移除异常处理开销
```

---

## 五、优化效果汇总

| 维度 | 优化前 | 优化后 | 减少 |
|------|--------|--------|------|
| target/ (debug+release) | 2,000 MB | ~350 MB | 82% |
| target/ (release only, after clean) | 423 MB | ~180 MB | 57% |
| Release 二进制 | 5.32 MB | ~3.8 MB | 29% |
| Release 二进制 (UPX) | 5.32 MB | ~1.5 MB | 72% |
| 构建依赖数 | 245 crate / 387 条目 | ~200 crate / ~280 条目 | 28% |
| 构建工具链要求 | Rust + cmake + C/C++ | 仅 Rust | — |

### 50MB 目标结论

| 目标 | 是否可行 | 说明 |
|------|---------|------|
| 二进制 < 50MB | ✅ 已达标（5.3MB） | 优化后 3.8MB |
| 分发包 < 50MB | ✅ 已达标（~4MB） | 二进制 + 配置文件 |
| target/ < 50MB | ❌ 不可行 | Rust 编译模型决定下限 ~180MB |

**target/ 的 50MB 目标受限于 Rust 编译架构**：每个 crate 必须独立编译为 .rlib（含元数据 + 目标码），200 个依赖 × 平均 0.6MB = 120MB 是不可压缩的物理下限。这是 Rust 生态的固有特性，非项目设计问题。
