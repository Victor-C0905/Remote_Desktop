//! User Namespace隔离模块
//!
//! 提供Linux User Namespace隔离能力，确保每个用户会话在独立的命名空间中运行。
//!
//! ## 设计原则
//! - **高内聚**: 只处理User Namespace相关逻辑
//! - **低耦合**: 独立于具体业务逻辑，提供通用隔离能力
//!
//! ## 安全机制
//! - 创建User Namespace防止提权
//! - UID/GID映射限制权限范围
//! - 禁用setgroups防止权限提升攻击

use anyhow::{Context, Result};
use std::fs::File;
use std::io::Write;

/// User Namespace隔离器
///
/// 封装User Namespace的创建和管理逻辑
pub struct UserNamespace {
    /// 容器内的UID
    inner_uid: u32,
    /// 容器内的GID
    inner_gid: u32,
}

impl UserNamespace {
    /// 创建新的User Namespace实例
    ///
    /// # 参数
    /// - `inner_uid`: 容器内的UID
    /// - `inner_gid`: 容器内的GID
    pub fn new(inner_uid: u32, inner_gid: u32) -> Self {
        Self {
            inner_uid,
            inner_gid,
        }
    }

    /// 创建并切换到User Namespace（Linux平台）
    ///
    /// # 实现步骤
    /// 1. 使用unshare创建User Namespace
    /// 2. 写入uid_map和gid_map建立映射
    /// 3. 禁用setgroups防止提权
    ///
    /// # 安全性
    /// - 使用CLONE_NEWUSER标志创建User Namespace
    /// - UID/GID映射限制在指定范围内
    /// - 禁用setgroups防止权限提升攻击
    ///
    /// # 错误
    /// - 如果unshare失败，返回错误
    /// - 如果写入映射文件失败，返回错误
    #[cfg(target_os = "linux")]
    pub fn create_and_switch(&self) -> Result<()> {
        use nix::sched::CloneFlags;
        use nix::unistd::getuid;

        tracing::info!(
            "创建User Namespace: inner_uid={}, inner_gid={}",
            self.inner_uid,
            self.inner_gid
        );

        // 第一步：创建User Namespace
        nix::sched::unshare(CloneFlags::CLONE_NEWUSER)
            .context("Failed to unshare user namespace")?;

        tracing::debug!("User Namespace创建成功");

        // 获取当前真实的UID/GID
        let outer_uid = getuid().as_raw();
        let outer_gid = nix::unistd::getgid().as_raw();

        tracing::debug!(
            "映射配置: outer_uid={}, outer_gid={}",
            outer_uid,
            outer_gid
        );

        // 第二步：禁用setgroups（防止提权）
        self.write_setgroups_disable()
            .context("Failed to disable setgroups")?;

        tracing::debug!("已禁用setgroups");

        // 第三步：写入UID映射
        self.write_uid_map(outer_uid)
            .context("Failed to write uid_map")?;

        tracing::debug!("UID映射写入成功");

        // 第四步：写入GID映射
        self.write_gid_map(outer_gid)
            .context("Failed to write gid_map")?;

        tracing::debug!("GID映射写入成功");

        tracing::info!("User Namespace切换完成");
        Ok(())
    }

    /// 非Linux平台的stub实现
    ///
    /// 在Windows/macOS等非Linux平台上，User Namespace不可用
    #[cfg(not(target_os = "linux"))]
    pub fn create_and_switch(&self) -> Result<()> {
        tracing::warn!("User Namespace仅支持Linux平台，当前平台跳过隔离");
        Ok(())
    }

    /// 写入UID映射到 /proc/self/uid_map
    ///
    /// # 格式
    /// `<inner_uid> <outer_uid> <count>`
    ///
    /// - inner_uid: 容器内的起始UID
    /// - outer_uid: 宿主机的真实UID
    /// - count: 映射范围（通常为1）
    ///
    /// # 参考
    /// man 7 user_namespaces
    #[cfg(target_os = "linux")]
    fn write_uid_map(&self, outer_uid: u32) -> Result<()> {
        let mut file = File::create("/proc/self/uid_map")
            .context("Failed to open /proc/self/uid_map")?;

        let content = format!("{} {} 1\n", self.inner_uid, outer_uid);
        file.write_all(content.as_bytes())
            .context("Failed to write uid_map")?;

        tracing::debug!("写入uid_map: {}", content.trim());
        Ok(())
    }

    /// 写入GID映射到 /proc/self/gid_map
    ///
    /// # 格式
    /// `<inner_gid> <outer_gid> <count>`
    ///
    /// - inner_gid: 容器内的起始GID
    /// - outer_gid: 宿主机的真实GID
    /// - count: 映射范围（通常为1）
    ///
    /// # 参考
    /// man 7 user_namespaces
    #[cfg(target_os = "linux")]
    fn write_gid_map(&self, outer_gid: u32) -> Result<()> {
        let mut file = File::create("/proc/self/gid_map")
            .context("Failed to open /proc/self/gid_map")?;

        let content = format!("{} {} 1\n", self.inner_gid, outer_gid);
        file.write_all(content.as_bytes())
            .context("Failed to write gid_map")?;

        tracing::debug!("写入gid_map: {}", content.trim());
        Ok(())
    }

    /// 禁用setgroups以防止提权攻击
    ///
    /// # 安全性
    /// 在User Namespace中，如果不禁用setgroups，
    /// 恶意进程可以通过setgroups提升权限。
    ///
    /// # 参考
    /// man 7 user_namespaces - "The /proc/[pid]/setgroups file"
    #[cfg(target_os = "linux")]
    fn write_setgroups_disable(&self) -> Result<()> {
        let mut file = File::create("/proc/self/setgroups")
            .context("Failed to open /proc/self/setgroups")?;

        file.write_all(b"deny\n")
            .context("Failed to write setgroups")?;

        tracing::debug!("写入setgroups: deny");
        Ok(())
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_namespace_creation() {
        let ns = UserNamespace::new(1000, 1000);
        assert_eq!(ns.inner_uid, 1000);
        assert_eq!(ns.inner_gid, 1000);
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn test_stub_implementation() {
        let ns = UserNamespace::new(1000, 1000);
        // 非Linux平台应该返回Ok
        assert!(ns.create_and_switch().is_ok());
    }
}