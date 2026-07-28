# 项目规范与约定

## 开发环境配置

### WSL 环境检测注意事项

**问题描述：**
使用 `wsl -e bash -c "command"` 执行命令时，不会加载用户的 `.bashrc` 或 `.zshrc` 配置文件，导致：
- Rust 版本检测错误（显示系统级旧版本而非用户安装的新版本）
- 环境变量未正确加载
- 工具链路径不正确

**正确检测方法：**
```powershell
# ❌ 错误方法（不会加载用户配置）
wsl -e bash -c "rustc --version"  # 可能显示系统级旧版本

# ✅ 正确方法（加载用户配置）
wsl -e bash -l -c "rustc --version"  # 使用 -l 参数加载登录 shell
```

**实际环境版本：**
- Windows Rust: 最新稳定版（通过 rustup 安装）
- WSL Rust: 最新稳定版（通过 rustup 安装，用户级）
- 项目使用 Edition 2024 特性，需要 Rust 1.85+ 版本

**编译注意事项：**
1. 在 WSL 中编译时，确保使用登录 shell 模式
2. 如果遇到 Cargo.lock 版本错误，检查 Rust 版本是否正确加载
3. 项目依赖包含 `edition2024` 特性的包（如 idna_adapter v1.2.2）

### 编译命令规范

**Windows 编译：**
```powershell
cd agent
cargo build --release
```

**WSL 编译：**
```powershell
# 方法1：直接进入 WSL（推荐）
wsl
cd /mnt/e/MyWork/gnome-remote/agent
cargo build --release

# 方法2：单行命令（需要加载环境）
wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo build --release"
```

**重要：**
- ❌ 不要使用 `wsl -e bash -c` （不加载用户环境）
- ✅ 必须使用 `wsl -e bash -l -c` 或直接进入 WSL

---

## 测试环境要求

### Agent 测试环境
- Linux 环境（WSL 或原生 Linux）
- Rust 1.85+ 版本
- root 权限（用于测试 PAM 认证和用户切换）

### 客户端测试环境
- Windows 10/11
- Node.js 18+
- Tauri 支持的浏览器环境

---

## 版本控制规范

### Cargo.lock 文件
- 当前版本：Version 4
- 需要 Rust 1.85+ 版本支持
- 不要降级 Cargo.lock 版本
- 如果遇到版本错误，检查 Rust 工具链版本

---

**文档版本：** 1.0
**创建日期：** 2026-07-28
**最后更新：** 2026-07-28