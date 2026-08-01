//! 连接管理模块
//!
//! 管理客户端连接与 PTY 会话的映射关系。
//!
//! # 设计理念
//!
//! - 一个 QUIC 连接（客户端）可以有多个 PTY 会话（多个终端窗口）
//! - 当客户端断开连接时，自动清理所有关联的 PTY 会话
//! - 与 `PtyRegistry` 集成，确保会话清理时正确释放资源

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use tracing::{info, warn};

#[cfg(unix)]
use super::pty_registry::PtyRegistry;

/// 连接管理器
///
/// 管理 QUIC 连接与 PTY 会话的映射关系。
///
/// # 数据结构
///
/// ```text
/// connections: HashMap<connection_id, Vec<session_id>>
/// ```
///
/// # 示例
///
/// ```rust
/// use manager::connection::ConnectionManager;
/// use manager::pty_registry::PtyRegistry;
/// use std::sync::Arc;
///
/// #[cfg(unix)]
/// async fn example() {
///     let registry = Arc::new(PtyRegistry::new());
///     let manager = ConnectionManager::new(registry);
///
///     // 注册会话
///     manager.register_session("conn-1", "session-1").await.unwrap();
///
///     // 获取连接的所有会话
///     let sessions = manager.get_sessions("conn-1").await;
///     assert_eq!(sessions, vec!["session-1"]);
/// }
/// ```
pub struct ConnectionManager {
    /// 连接ID -> 会话ID列表
    ///
    /// 一个客户端连接可以有多个 PTY 会话（多个终端窗口）
    connections: Arc<RwLock<HashMap<String, Vec<String>>>>,

    /// 反向映射：session_id -> connection_id
    ///
    /// 用于快速查找会话所属的连接
    session_to_connection: Arc<RwLock<HashMap<String, String>>>,

    /// PTY 注册表（用于清理会话）
    ///
    /// 当连接断开时，需要从 PtyRegistry 注销所有关联的会话
    #[cfg(unix)]
    pty_registry: Arc<PtyRegistry>,
}

impl ConnectionManager {
    /// 创建新的连接管理器
    ///
    /// # 参数
    ///
    /// - `pty_registry`: PTY 注册表（用于清理会话）
    #[cfg(unix)]
    pub fn new(pty_registry: Arc<PtyRegistry>) -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            session_to_connection: Arc::new(RwLock::new(HashMap::new())),
            pty_registry,
        }
    }

    /// 创建新的连接管理器（非 Unix 平台）
    #[cfg(not(unix))]
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            session_to_connection: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册新会话
    ///
    /// 将会话 ID 与连接 ID 关联起来。
    ///
    /// # 参数
    ///
    /// - `connection_id`: QUIC 连接 ID
    /// - `session_id`: PTY 会话 ID
    ///
    /// # 返回
    ///
    /// - `Ok(())`: 注册成功或会话已存在且属于同一连接（幂等性）
    /// - `Err`: 会话已存在但属于不同连接
    ///
    /// # 示例
    ///
    /// ```rust
    /// manager.register_session("conn-1", "session-1").await.unwrap();
    /// ```
    pub async fn register_session(&self, connection_id: &str, session_id: &str) -> Result<()> {
        // 检查会话是否已存在
        {
            let session_to_conn = self.session_to_connection.read().await;
            if let Some(existing_conn_id) = session_to_conn.get(session_id) {
                if existing_conn_id == connection_id {
                    // 幂等性：会话已存在且属于同一连接，直接返回成功
                    info!(
                        "会话已注册（幂等）: connection_id={}, session_id={}",
                        connection_id, session_id
                    );
                    return Ok(());
                } else {
                    // 冲突：会话已存在但属于不同连接
                    return Err(anyhow::anyhow!(
                        "会话已注册到不同连接: session_id={}, existing_connection_id={}, new_connection_id={}",
                        session_id, existing_conn_id, connection_id
                    ));
                }
            }
        }

        // 更新连接 -> 会话映射
        {
            let mut connections = self.connections.write().await;
            connections
                .entry(connection_id.to_string())
                .or_insert_with(Vec::new)
                .push(session_id.to_string());
        }

        // 更新会话 -> 连接映射
        {
            let mut session_to_conn = self.session_to_connection.write().await;
            session_to_conn.insert(session_id.to_string(), connection_id.to_string());
        }

        info!(
            "会话已注册: connection_id={}, session_id={}",
            connection_id, session_id
        );

        Ok(())
    }

    /// 注销会话
    ///
    /// 从映射中移除会话，并从 PtyRegistry 注销。
    ///
    /// # 参数
    ///
    /// - `session_id`: PTY 会话 ID
    ///
    /// # 返回
    ///
    /// 返回被注销的会话所属的连接 ID（如果存在）
    ///
    /// # 示例
    ///
    /// ```rust
    /// let connection_id = manager.unregister_session("session-1").await.unwrap();
    /// ```
    pub async fn unregister_session(&self, session_id: &str) -> Result<Option<String>> {
        // 1. 从会话 -> 连接映射中移除
        let connection_id = {
            let mut session_to_conn = self.session_to_connection.write().await;
            session_to_conn.remove(session_id)
        };

        if let Some(ref conn_id) = connection_id {
            // 2. 从连接 -> 会话映射中移除
            {
                let mut connections = self.connections.write().await;
                if let Some(sessions) = connections.get_mut(conn_id) {
                    sessions.retain(|s| s != session_id);

                    // 如果连接没有会话了，移除整个连接条目
                    if sessions.is_empty() {
                        connections.remove(conn_id);
                    }
                }
            }

            // 3. 从 PtyRegistry 注销（仅 Unix）
            #[cfg(unix)]
            {
                if let Some(_session) = self.pty_registry.unregister(session_id).await? {
                    info!(
                        "会话已从 PtyRegistry 注销: session_id={}, connection_id={}",
                        session_id, conn_id
                    );
                }
            }

            info!(
                "会话已注销: session_id={}, connection_id={}",
                session_id, conn_id
            );
        } else {
            warn!("尝试注销不存在的会话: session_id={}", session_id);
        }

        Ok(connection_id)
    }

    /// 获取连接的所有会话
    ///
    /// # 参数
    ///
    /// - `connection_id`: QUIC 连接 ID
    ///
    /// # 返回
    ///
    /// 返回该连接的所有会话 ID 列表（如果连接存在）
    ///
    /// # 示例
    ///
    /// ```rust
    /// let sessions = manager.get_sessions("conn-1").await;
    /// assert_eq!(sessions, vec!["session-1", "session-2"]);
    /// ```
    pub async fn get_sessions(&self, connection_id: &str) -> Vec<String> {
        let connections = self.connections.read().await;
        connections
            .get(connection_id)
            .cloned()
            .unwrap_or_default()
    }

    /// 清理连接的所有会话
    ///
    /// 当客户端断开连接时调用，清理所有关联的 PTY 会话。
    ///
    /// # 参数
    ///
    /// - `connection_id`: QUIC 连接 ID
    ///
    /// # 返回
    ///
    /// 返回被清理的会话数量
    ///
    /// # 错误
    ///
    /// 如果 PtyRegistry 注销失败，返回聚合错误信息
    ///
    /// # 示例
    ///
    /// ```rust
    /// let count = manager.cleanup_connection("conn-1").await.unwrap();
    /// info!("已清理 {} 个会话", count);
    /// ```
    pub async fn cleanup_connection(&self, connection_id: &str) -> Result<usize> {
        // 1. 获取该连接的所有会话
        let sessions = {
            let mut connections = self.connections.write().await;
            connections.remove(connection_id).unwrap_or_default()
        };

        let count = sessions.len();

        if count == 0 {
            info!("连接没有关联的会话: connection_id={}", connection_id);
            return Ok(0);
        }

        // 2. 从会话 -> 连接映射中移除
        {
            let mut session_to_conn = self.session_to_connection.write().await;
            for session_id in &sessions {
                session_to_conn.remove(session_id);
            }
        }

        // 3. 从 PtyRegistry 注销所有会话（仅 Unix）
        #[cfg(unix)]
        {
            let mut errors = Vec::new();
            for session_id in &sessions {
                if let Err(e) = self.pty_registry.unregister(session_id).await {
                    warn!(
                        "从 PtyRegistry 注销会话失败: session_id={}, error={}",
                        session_id, e
                    );
                    errors.push(format!("session_id={}: {}", session_id, e));
                }
            }

            // 如果有注销失败的会话，返回聚合错误
            if !errors.is_empty() {
                return Err(anyhow::anyhow!(
                    "部分会话注销失败: {}",
                    errors.join(", ")
                ));
            }
        }

        info!(
            "连接已清理: connection_id={}, session_count={}",
            connection_id, count
        );

        Ok(count)
    }

    /// 获取活动连接数
    ///
    /// # 返回
    ///
    /// 返回当前活动的连接数量
    pub async fn connection_count(&self) -> usize {
        let connections = self.connections.read().await;
        connections.len()
    }

    /// 获取总会话数
    ///
    /// # 返回
    ///
    /// 返回所有连接的会话总数
    pub async fn total_session_count(&self) -> usize {
        let session_to_conn = self.session_to_connection.read().await;
        session_to_conn.len()
    }

    /// 检查会话是否存在
    ///
    /// # 参数
    ///
    /// - `session_id`: PTY 会话 ID
    ///
    /// # 返回
    ///
    /// 如果会话存在，返回 true
    pub async fn has_session(&self, session_id: &str) -> bool {
        let session_to_conn = self.session_to_connection.read().await;
        session_to_conn.contains_key(session_id)
    }

    /// 获取会话所属的连接 ID
    ///
    /// # 参数
    ///
    /// - `session_id`: PTY 会话 ID
    ///
    /// # 返回
    ///
    /// 如果会话存在，返回连接 ID
    pub async fn get_connection_id(&self, session_id: &str) -> Option<String> {
        let session_to_conn = self.session_to_connection.read().await;
        session_to_conn.get(session_id).cloned()
    }

    /// 验证数据结构一致性
    ///
    /// 检查双向映射是否一致：
    /// - connections 中的所有会话都在 session_to_connection 中有映射
    /// - session_to_connection 中的所有会话都在 connections 中存在
    ///
    /// # 返回
    ///
    /// - `Ok(())`: 数据一致
    /// - `Err`: 数据不一致，包含详细错误信息
    pub async fn verify_consistency(&self) -> Result<()> {
        let connections = self.connections.read().await;
        let session_to_conn = self.session_to_connection.read().await;

        let mut errors = Vec::new();

        // 1. 检查 connections -> session_to_connection 一致性
        for (conn_id, sessions) in connections.iter() {
            for session_id in sessions {
                match session_to_conn.get(session_id) {
                    Some(mapped_conn_id) if mapped_conn_id == conn_id => {
                        // 一致，继续检查
                    }
                    Some(mapped_conn_id) => {
                        errors.push(format!(
                            "会话映射不一致: session_id={}, 在 connections 中属于 {}, 但在 session_to_connection 中属于 {}",
                            session_id, conn_id, mapped_conn_id
                        ));
                    }
                    None => {
                        errors.push(format!(
                            "会话缺失反向映射: session_id={} 在 connections 中存在，但在 session_to_connection 中不存在",
                            session_id
                        ));
                    }
                }
            }
        }

        // 2. 检查 session_to_connection -> connections 一致性
        for (session_id, conn_id) in session_to_conn.iter() {
            match connections.get(conn_id) {
                Some(sessions) if sessions.contains(session_id) => {
                    // 一致，继续检查
                }
                Some(sessions) => {
                    errors.push(format!(
                        "会话不在连接列表中: session_id={} 映射到 {}, 但该连接的会话列表不包含此会话（{:?}）",
                        session_id, conn_id, sessions
                    ));
                }
                None => {
                    errors.push(format!(
                        "连接不存在: session_id={} 映射到不存在的连接 {}",
                        session_id, conn_id
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("数据一致性验证失败:\n{}", errors.join("\n")))
        }
    }
}

#[cfg(unix)]
impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new(Arc::new(PtyRegistry::new()))
    }
}

#[cfg(not(unix))]
impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_session() {
        #[cfg(unix)]
        let manager = ConnectionManager::new(Arc::new(PtyRegistry::new()));

        #[cfg(not(unix))]
        let manager = ConnectionManager::new();

        // 注册会话
        manager.register_session("conn-1", "session-1").await.unwrap();

        // 验证会话已注册
        let sessions = manager.get_sessions("conn-1").await;
        assert_eq!(sessions, vec!["session-1"]);

        // 验证会话存在
        assert!(manager.has_session("session-1").await);
        assert!(!manager.has_session("session-2").await);

        // 验证连接 ID 查找
        let conn_id = manager.get_connection_id("session-1").await;
        assert_eq!(conn_id, Some("conn-1".to_string()));
    }

    #[tokio::test]
    async fn test_multiple_sessions() {
        #[cfg(unix)]
        let manager = ConnectionManager::new(Arc::new(PtyRegistry::new()));

        #[cfg(not(unix))]
        let manager = ConnectionManager::new();

        // 注册多个会话
        manager.register_session("conn-1", "session-1").await.unwrap();
        manager.register_session("conn-1", "session-2").await.unwrap();
        manager.register_session("conn-1", "session-3").await.unwrap();

        // 验证会话数量
        let sessions = manager.get_sessions("conn-1").await;
        assert_eq!(sessions.len(), 3);
        assert!(sessions.contains(&"session-1".to_string()));
        assert!(sessions.contains(&"session-2".to_string()));
        assert!(sessions.contains(&"session-3".to_string()));

        // 验证总数
        assert_eq!(manager.connection_count().await, 1);
        assert_eq!(manager.total_session_count().await, 3);
    }

    #[tokio::test]
    async fn test_unregister_session() {
        #[cfg(unix)]
        let manager = ConnectionManager::new(Arc::new(PtyRegistry::new()));

        #[cfg(not(unix))]
        let manager = ConnectionManager::new();

        // 注册会话
        manager.register_session("conn-1", "session-1").await.unwrap();
        manager.register_session("conn-1", "session-2").await.unwrap();

        // 注销一个会话
        let conn_id = manager.unregister_session("session-1").await.unwrap();
        assert_eq!(conn_id, Some("conn-1".to_string()));

        // 验证会话已移除
        let sessions = manager.get_sessions("conn-1").await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0], "session-2");

        // 验证会话不存在
        assert!(!manager.has_session("session-1").await);
        assert!(manager.has_session("session-2").await);

        // 验证总数
        assert_eq!(manager.total_session_count().await, 1);
    }

    #[tokio::test]
    async fn test_unregister_last_session() {
        #[cfg(unix)]
        let manager = ConnectionManager::new(Arc::new(PtyRegistry::new()));

        #[cfg(not(unix))]
        let manager = ConnectionManager::new();

        // 注册并注销最后一个会话
        manager.register_session("conn-1", "session-1").await.unwrap();
        manager.unregister_session("session-1").await.unwrap();

        // 验证连接也被移除
        let sessions = manager.get_sessions("conn-1").await;
        assert!(sessions.is_empty());

        // 验证连接数
        assert_eq!(manager.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_cleanup_connection() {
        #[cfg(unix)]
        let manager = ConnectionManager::new(Arc::new(PtyRegistry::new()));

        #[cfg(not(unix))]
        let manager = ConnectionManager::new();

        // 注册多个连接和会话
        manager.register_session("conn-1", "session-1").await.unwrap();
        manager.register_session("conn-1", "session-2").await.unwrap();
        manager.register_session("conn-2", "session-3").await.unwrap();

        // 清理 conn-1
        let count = manager.cleanup_connection("conn-1").await.unwrap();
        assert_eq!(count, 2);

        // 验证 conn-1 的会话已清理
        assert!(!manager.has_session("session-1").await);
        assert!(!manager.has_session("session-2").await);
        assert!(manager.has_session("session-3").await);

        // 验证连接数
        assert_eq!(manager.connection_count().await, 1);
        assert_eq!(manager.total_session_count().await, 1);
    }

    #[tokio::test]
    async fn test_unregister_nonexistent_session() {
        #[cfg(unix)]
        let manager = ConnectionManager::new(Arc::new(PtyRegistry::new()));

        #[cfg(not(unix))]
        let manager = ConnectionManager::new();

        // 注销不存在的会话
        let conn_id = manager.unregister_session("nonexistent").await.unwrap();
        assert!(conn_id.is_none());
    }

    #[tokio::test]
    async fn test_get_nonexistent_sessions() {
        #[cfg(unix)]
        let manager = ConnectionManager::new(Arc::new(PtyRegistry::new()));

        #[cfg(not(unix))]
        let manager = ConnectionManager::new();

        // 获取不存在的连接的会话
        let sessions = manager.get_sessions("nonexistent").await;
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_cleanup_empty_connection() {
        #[cfg(unix)]
        let manager = ConnectionManager::new(Arc::new(PtyRegistry::new()));

        #[cfg(not(unix))]
        let manager = ConnectionManager::new();

        // 清理不存在的连接
        let count = manager.cleanup_connection("nonexistent").await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_duplicate_session_registration_same_connection() {
        #[cfg(unix)]
        let manager = ConnectionManager::new(Arc::new(PtyRegistry::new()));

        #[cfg(not(unix))]
        let manager = ConnectionManager::new();

        // 注册会话
        manager.register_session("conn-1", "session-1").await.unwrap();

        // 重复注册到同一连接（应该成功，幂等性）
        manager.register_session("conn-1", "session-1").await.unwrap();

        // 验证只有一个会话
        let sessions = manager.get_sessions("conn-1").await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0], "session-1");

        // 验证数据一致性
        manager.verify_consistency().await.unwrap();
    }

    #[tokio::test]
    async fn test_duplicate_session_registration_different_connection() {
        #[cfg(unix)]
        let manager = ConnectionManager::new(Arc::new(PtyRegistry::new()));

        #[cfg(not(unix))]
        let manager = ConnectionManager::new();

        // 注册会话到 conn-1
        manager.register_session("conn-1", "session-1").await.unwrap();

        // 尝试将同一会话注册到 conn-2（应该失败）
        let result = manager.register_session("conn-2", "session-1").await;
        assert!(result.is_err());

        // 验证错误信息包含相关细节
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("会话已注册到不同连接"));
        assert!(err_msg.contains("session-1"));
        assert!(err_msg.contains("conn-1"));
        assert!(err_msg.contains("conn-2"));

        // 验证数据未被破坏
        let sessions = manager.get_sessions("conn-1").await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0], "session-1");

        let sessions = manager.get_sessions("conn-2").await;
        assert!(sessions.is_empty());

        // 验证数据一致性
        manager.verify_consistency().await.unwrap();
    }

    #[tokio::test]
    async fn test_verify_consistency() {
        #[cfg(unix)]
        let manager = ConnectionManager::new(Arc::new(PtyRegistry::new()));

        #[cfg(not(unix))]
        let manager = ConnectionManager::new();

        // 空状态应该一致
        manager.verify_consistency().await.unwrap();

        // 注册多个会话
        manager.register_session("conn-1", "session-1").await.unwrap();
        manager.register_session("conn-1", "session-2").await.unwrap();
        manager.register_session("conn-2", "session-3").await.unwrap();

        // 验证一致性
        manager.verify_consistency().await.unwrap();

        // 清理后也应该一致
        manager.cleanup_connection("conn-1").await.unwrap();
        manager.verify_consistency().await.unwrap();
    }
}