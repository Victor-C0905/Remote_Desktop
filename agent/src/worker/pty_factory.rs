//! PTY 工厂 - 用于创建 PTY 会话
//!
//! 该模块负责：
//! - 使用 `forkpty()` 创建 PTY 会话
//! - 配置终端大小、环境变量
//! - 将 master_fd 通过 IPC 发送给 Manager

use anyhow::Result;

/// PTY 工厂
///
/// 负责创建 PTY 会话并将文件描述符转移给 Manager。
pub struct PtyFactory {
    // TODO: TASK-018 添加 IpcClient 引用
    // ipc_client: Arc<IpcClient>,
}

impl PtyFactory {
    /// 创建 PTY 工厂实例
    pub fn new() -> Self {
        Self {}
    }

    /// 创建 PTY 会话
    ///
    /// # 参数
    ///
    /// - `shell`: Shell 程序路径（如 `/bin/bash`）
    /// - `cols`: 终端列数
    /// - `rows`: 终端行数
    /// - `cwd`: 工作目录（可选）
    ///
    /// # 返回
    ///
    /// 成功返回 `(session_id, pid)`，失败返回错误。
    ///
    /// # 注意
    ///
    /// master_fd 会自动通过 IPC 发送给 Manager，不在返回值中。
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let factory = PtyFactory::new();
    /// let (session_id, pid) = factory.create("/bin/bash", 80, 24, None).await?;
    /// ```
    pub async fn create(
        &self,
        _shell: &str,
        _cols: u16,
        _rows: u16,
        _cwd: Option<&str>,
    ) -> Result<(String, u32)> {
        // TODO: TASK-018 实现以下逻辑:
        // 1. forkpty() 创建 PTY
        // 2. 子进程 exec shell
        // 3. 父进程通过 IPC 发送 master_fd 给 Manager
        // 4. 返回 session_id 和 pid

        unimplemented!("PTY creation will be implemented in TASK-018")
    }
}

impl Default for PtyFactory {
    fn default() -> Self {
        Self::new()
    }
}