//! 认证和连接统计模块
//!
//! 提供运行时监控数据，包括：
//! - 认证统计（成功/失败/锁定/速率限制）
//! - 连接统计（活跃连接/历史连接/断开原因）
//! - 性能指标（响应时间/吞吐量）

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::sync::Mutex;
use std::collections::VecDeque;

/// 认证统计
#[derive(Debug, Default)]
pub struct AuthStats {
    /// 总认证尝试次数
    pub total_attempts: AtomicU64,
    /// 成功次数
    pub successful: AtomicU64,
    /// 失败次数（密码错误、签名验证失败等）
    pub failed: AtomicU64,
    /// 账户锁定次数
    pub locked: AtomicU64,
    /// IP速率限制触发次数
    pub rate_limited: AtomicU64,
    /// 会话超时次数
    pub session_timeout: AtomicU64,
    /// 密码认证次数
    pub password_attempts: AtomicU64,
    /// 公钥认证次数
    pub pubkey_attempts: AtomicU64,
}

/// 连接统计
#[derive(Debug, Default)]
pub struct ConnectionStats {
    /// 当前活跃连接数
    pub active_connections: AtomicU64,
    /// 历史总连接数
    pub total_connections: AtomicU64,
    /// 正常断开次数
    pub normal_disconnects: AtomicU64,
    /// 超时断开次数
    pub timeout_disconnects: AtomicU64,
    /// 错误断开次数
    pub error_disconnects: AtomicU64,
}

/// 响应时间记录（滑动窗口）
struct ResponseTimeRecord {
    #[allow(dead_code)]
    timestamp: Instant,
    duration_ms: u64,
}

/// 性能指标
pub struct PerformanceMetrics {
    /// 最近100次API响应时间（滑动窗口）
    response_times: Mutex<VecDeque<ResponseTimeRecord>>,
    /// 文件传输总字节数
    pub total_bytes_transferred: AtomicU64,
    /// 终端输出总字节数
    pub total_terminal_bytes: AtomicU64,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            response_times: Mutex::new(VecDeque::with_capacity(100)),
            total_bytes_transferred: AtomicU64::new(0),
            total_terminal_bytes: AtomicU64::new(0),
        }
    }
}

impl PerformanceMetrics {
    /// 记录响应时间
    pub async fn record_response_time(&self, duration_ms: u64) {
        let mut times = self.response_times.lock().await;
        
        // 添加新记录
        times.push_back(ResponseTimeRecord {
            timestamp: Instant::now(),
            duration_ms,
        });
        
        // 保持窗口大小为100
        while times.len() > 100 {
            times.pop_front();
        }
    }
    
    /// 计算响应时间百分位数
    pub async fn get_percentiles(&self) -> ResponseTimePercentiles {
        let times = self.response_times.lock().await;
        
        if times.is_empty() {
            return ResponseTimePercentiles::default();
        }
        
        let mut sorted: Vec<u64> = times.iter().map(|r| r.duration_ms).collect();
        sorted.sort();
        
        let len = sorted.len();
        
        ResponseTimePercentiles {
            p50: sorted[len / 2],
            p95: sorted[len * 95 / 100],
            p99: sorted[len * 99 / 100],
            min: sorted[0],
            max: sorted[len - 1],
            count: len as u64,
        }
    }
}

/// 响应时间百分位数
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResponseTimePercentiles {
    /// 中位数（50百分位）
    pub p50: u64,
    /// 95百分位
    pub p95: u64,
    /// 99百分位
    pub p99: u64,
    /// 最小值
    pub min: u64,
    /// 最大值
    pub max: u64,
    /// 样本数
    pub count: u64,
}

/// 统计管理器（全局单例）
pub struct StatsManager {
    pub auth: AuthStats,
    pub connections: ConnectionStats,
    pub performance: PerformanceMetrics,
}

impl StatsManager {
    /// 创建新的统计管理器
    pub fn new() -> Self {
        Self {
            auth: AuthStats::default(),
            connections: ConnectionStats::default(),
            performance: PerformanceMetrics::default(),
        }
    }

    // ========== 认证统计方法 ==========

    /// 记录认证成功
    pub fn record_auth_success(&self, auth_type: &str) {
        self.auth.total_attempts.fetch_add(1, Ordering::Relaxed);
        self.auth.successful.fetch_add(1, Ordering::Relaxed);

        match auth_type {
            "password" => self.auth.password_attempts.fetch_add(1, Ordering::Relaxed),
            "pubkey" => self.auth.pubkey_attempts.fetch_add(1, Ordering::Relaxed),
            _ => 0,
        };
    }

    /// 记录认证失败
    pub fn record_auth_failure(&self, auth_type: &str) {
        self.auth.total_attempts.fetch_add(1, Ordering::Relaxed);
        self.auth.failed.fetch_add(1, Ordering::Relaxed);

        match auth_type {
            "password" => self.auth.password_attempts.fetch_add(1, Ordering::Relaxed),
            "pubkey" => self.auth.pubkey_attempts.fetch_add(1, Ordering::Relaxed),
            _ => 0,
        };
    }

    /// 记录账户锁定
    pub fn record_account_locked(&self) {
        self.auth.locked.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录IP速率限制触发
    pub fn record_rate_limited(&self) {
        self.auth.rate_limited.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录会话超时
    pub fn record_session_timeout(&self) {
        self.auth.session_timeout.fetch_add(1, Ordering::Relaxed);
    }

    // ========== 连接统计方法 ==========

    /// 记录新连接
    pub fn record_connection_opened(&self) {
        self.connections.active_connections.fetch_add(1, Ordering::Relaxed);
        self.connections.total_connections.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录连接关闭
    pub fn record_connection_closed(&self, reason: ConnectionCloseReason) {
        self.connections.active_connections.fetch_sub(1, Ordering::Relaxed);

        match reason {
            ConnectionCloseReason::Normal => {
                self.connections.normal_disconnects.fetch_add(1, Ordering::Relaxed);
            }
            ConnectionCloseReason::Timeout => {
                self.connections.timeout_disconnects.fetch_add(1, Ordering::Relaxed);
            }
            ConnectionCloseReason::Error => {
                self.connections.error_disconnects.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    // ========== 性能指标方法 ==========

    /// 记录API响应时间
    pub async fn record_api_response_time(&self, duration_ms: u64) {
        self.performance.record_response_time(duration_ms).await;
    }

    /// 记录文件传输字节数
    pub fn record_file_transfer_bytes(&self, bytes: u64) {
        self.performance.total_bytes_transferred.fetch_add(bytes, Ordering::Relaxed);
    }

    /// 记录终端输出字节数
    #[allow(dead_code)]
    pub fn record_terminal_bytes(&self, bytes: u64) {
        self.performance.total_terminal_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    // ========== 权限检查方法 ==========

    /// 检查用户是否有权限查看指定统计类型
    ///
    /// # 参数
    /// - `session`: 用户会话信息
    /// - `stats_type`: 统计类型 ("auth" | "connection" | "performance" | "all")
    ///
    /// # 返回
    /// - `true`: 有权限
    /// - `false`: 无权限
    #[allow(dead_code)]
    pub fn check_permission(&self, session: &crate::auth::UserSession, stats_type: &str) -> bool {
        match stats_type {
            "auth" | "performance" => {
                // 只有 root 用户可以查看认证统计和性能指标
                session.uid == 0
            }
            "connection" | "all" => {
                // root用户可以查看，普通用户也可以查看（但数据会过滤）
                true
            }
            _ => false,
        }
    }

    /// 获取连接统计（根据用户权限过滤）
    ///
    /// # 参数
    /// - `session`: 用户会话信息
    ///
    /// # 返回
    /// - root用户: 返回全局统计
    /// - 普通用户: 返回个人统计（TODO: 未来实现）
    pub fn get_connection_stats_for_user(&self, session: &crate::auth::UserSession) -> ConnectionStatsSnapshot {
        if session.uid == 0 {
            // root用户：返回全局统计
            self.get_connection_stats()
        } else {
            // 普通用户：返回个人统计（当前实现为空数据）
            // TODO: 未来需要追踪每个用户的连接数
            ConnectionStatsSnapshot::default()
        }
    }

    // ========== 查询方法 ==========

    /// 获取认证统计快照
    pub fn get_auth_stats(&self) -> AuthStatsSnapshot {
        AuthStatsSnapshot {
            total_attempts: self.auth.total_attempts.load(Ordering::Relaxed),
            successful: self.auth.successful.load(Ordering::Relaxed),
            failed: self.auth.failed.load(Ordering::Relaxed),
            locked: self.auth.locked.load(Ordering::Relaxed),
            rate_limited: self.auth.rate_limited.load(Ordering::Relaxed),
            session_timeout: self.auth.session_timeout.load(Ordering::Relaxed),
            password_attempts: self.auth.password_attempts.load(Ordering::Relaxed),
            pubkey_attempts: self.auth.pubkey_attempts.load(Ordering::Relaxed),
        }
    }

    /// 获取连接统计快照
    pub fn get_connection_stats(&self) -> ConnectionStatsSnapshot {
        ConnectionStatsSnapshot {
            active_connections: self.connections.active_connections.load(Ordering::Relaxed),
            total_connections: self.connections.total_connections.load(Ordering::Relaxed),
            normal_disconnects: self.connections.normal_disconnects.load(Ordering::Relaxed),
            timeout_disconnects: self.connections.timeout_disconnects.load(Ordering::Relaxed),
            error_disconnects: self.connections.error_disconnects.load(Ordering::Relaxed),
        }
    }

    /// 获取性能指标快照
    pub async fn get_performance_stats(&self) -> PerformanceStatsSnapshot {
        let percentiles = self.performance.get_percentiles().await;

        PerformanceStatsSnapshot {
            response_times: percentiles,
            total_bytes_transferred: self.performance.total_bytes_transferred.load(Ordering::Relaxed),
            total_terminal_bytes: self.performance.total_terminal_bytes.load(Ordering::Relaxed),
        }
    }
}

impl Default for StatsManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 连接关闭原因
#[derive(Debug, Clone, Copy)]
pub enum ConnectionCloseReason {
    /// 正常关闭（客户端主动断开）
    Normal,
    /// 超时关闭（空闲超时或会话超时）
    #[allow(dead_code)]
    Timeout,
    /// 错误关闭（异常断开）
    #[allow(dead_code)]
    Error,
}

/// 认证统计快照（用于API返回）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthStatsSnapshot {
    pub total_attempts: u64,
    pub successful: u64,
    pub failed: u64,
    pub locked: u64,
    pub rate_limited: u64,
    pub session_timeout: u64,
    pub password_attempts: u64,
    pub pubkey_attempts: u64,
}

/// 连接统计快照（用于API返回）
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConnectionStatsSnapshot {
    pub active_connections: u64,
    pub total_connections: u64,
    pub normal_disconnects: u64,
    pub timeout_disconnects: u64,
    pub error_disconnects: u64,
}

/// 性能指标快照（用于API返回）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PerformanceStatsSnapshot {
    pub response_times: ResponseTimePercentiles,
    pub total_bytes_transferred: u64,
    pub total_terminal_bytes: u64,
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_auth_stats() {
        let stats = StatsManager::new();
        
        stats.record_auth_success("password");
        stats.record_auth_failure("pubkey");
        stats.record_account_locked();
        
        let snapshot = stats.get_auth_stats();
        assert_eq!(snapshot.total_attempts, 2);
        assert_eq!(snapshot.successful, 1);
        assert_eq!(snapshot.failed, 1);
        assert_eq!(snapshot.locked, 1);
    }
    
    #[test]
    fn test_connection_stats() {
        let stats = StatsManager::new();
        
        stats.record_connection_opened();
        stats.record_connection_opened();
        stats.record_connection_closed(ConnectionCloseReason::Normal);
        
        let snapshot = stats.get_connection_stats();
        assert_eq!(snapshot.active_connections, 1);
        assert_eq!(snapshot.total_connections, 2);
        assert_eq!(snapshot.normal_disconnects, 1);
    }
    
    #[tokio::test]
    async fn test_performance_metrics() {
        let stats = StatsManager::new();
        
        stats.record_api_response_time(50).await;
        stats.record_api_response_time(100).await;
        stats.record_api_response_time(150).await;
        
        let snapshot = stats.get_performance_stats().await;
        assert_eq!(snapshot.response_times.count, 3);
        assert!(snapshot.response_times.p50 > 0);
    }
}