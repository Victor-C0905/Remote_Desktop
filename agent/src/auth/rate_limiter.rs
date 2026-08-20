//! 认证速率限制器
//!
//! 提供两层的认证防护：
//! 1. **失败次数限制**：防止暴力破解（用户名级别）
//! 2. **IP速率限制**：防止DoS攻击（IP级别）
//!
//! ## 配置参数
//! - `max_failed_attempts`: 最大失败次数（默认5次）
//! - `lockout_duration`: 锁定时长（默认15分钟）
//! - `max_requests_per_minute`: 每分钟最大请求数（默认10次）

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use anyhow::Result;

/// 认证失败记录
#[derive(Debug, Clone)]
struct FailedAttempt {
    /// 失败次数
    count: u32,
    /// 首次失败时间
    first_attempt: Instant,
    /// 锁定截止时间
    locked_until: Option<Instant>,
}

/// IP请求记录
#[derive(Debug, Clone)]
struct IpRecord {
    /// 请求时间戳列表（滑动窗口）
    request_times: Vec<Instant>,
}

/// 认证速率限制器
pub struct AuthRateLimiter {
    /// 失败次数缓存（username -> FailedAttempt）
    failed_attempts: Arc<Mutex<HashMap<String, FailedAttempt>>>,

    /// IP请求缓存（ip -> IpRecord）
    ip_requests: Arc<Mutex<HashMap<String, IpRecord>>>,

    /// 最大失败次数（超过此数值将锁定）
    max_failed_attempts: u32,

    /// 锁定时长（失败锁定后的持续时间）
    lockout_duration: Duration,

    /// 每分钟最大请求数（IP级别）
    max_requests_per_minute: u32,

    /// 请求窗口时长（用于IP速率限制）
    request_window: Duration,
}

impl AuthRateLimiter {
    /// 创建新的认证速率限制器
    ///
    /// # 默认配置
    /// - 最大失败次数：5次
    /// - 锁定时长：15分钟
    /// - 每分钟最大请求数：10次
    pub fn new() -> Self {
        Self {
            failed_attempts: Arc::new(Mutex::new(HashMap::new())),
            ip_requests: Arc::new(Mutex::new(HashMap::new())),
            max_failed_attempts: 5,
            lockout_duration: Duration::from_secs(15 * 60), // 15分钟
            max_requests_per_minute: 10,
            request_window: Duration::from_secs(60),
        }
    }

    /// 创建自定义配置的速率限制器
    #[allow(dead_code)]
    pub fn with_config(
        max_failed_attempts: u32,
        lockout_duration_secs: u64,
        max_requests_per_minute: u32,
    ) -> Self {
        Self {
            failed_attempts: Arc::new(Mutex::new(HashMap::new())),
            ip_requests: Arc::new(Mutex::new(HashMap::new())),
            max_failed_attempts,
            lockout_duration: Duration::from_secs(lockout_duration_secs),
            max_requests_per_minute,
            request_window: Duration::from_secs(60),
        }
    }

    /// 检查用户名是否被锁定（暴力破解防护）
    ///
    /// # 参数
    /// - `username`: 用户名
    ///
    /// # 返回
    /// - `Ok(true)`: 允许继续认证
    /// - `Ok(false)`: 用户被锁定
    /// - `Err`: 检查过程中出错
    pub async fn is_username_allowed(&self, username: &str) -> Result<bool> {
        let mut attempts = self.failed_attempts.lock().await;

        if let Some(record) = attempts.get(username) {
            // 检查是否在锁定期内
            if let Some(locked_until) = record.locked_until {
                if Instant::now() < locked_until {
                    tracing::warn!(
                        "用户名被锁定: username={}, remaining={:?}",
                        username,
                        locked_until.duration_since(Instant::now())
                    );
                    return Ok(false);
                } else {
                    // 锁定期已过，清除记录
                    attempts.remove(username);
                    tracing::info!("用户名锁定已解除: username={}", username);
                }
            }
        }

        Ok(true)
    }

    /// 检查IP是否超过速率限制（DoS防护）
    ///
    /// # 参数
    /// - `client_ip`: 客户端IP地址
    ///
    /// # 返回
    /// - `Ok(true)`: 允许继续请求
    /// - `Ok(false)`: IP被限流
    /// - `Err`: 检查过程中出错
    pub async fn is_ip_allowed(&self, client_ip: &str) -> Result<bool> {
        let mut ip_reqs = self.ip_requests.lock().await;
        let now = Instant::now();

        // 获取或创建IP记录
        let record = ip_reqs.entry(client_ip.to_string()).or_insert(IpRecord {
            request_times: Vec::new(),
        });

        // 清理过期的请求记录（滑动窗口）
        record.request_times.retain(|&time| {
            now.duration_since(time) < self.request_window
        });

        // 检查请求次数
        if record.request_times.len() >= self.max_requests_per_minute as usize {
            tracing::warn!(
                "IP超过速率限制: ip={}, requests_in_last_minute={}, max={}",
                client_ip,
                record.request_times.len(),
                self.max_requests_per_minute
            );
            return Ok(false);
        }

        // 记录本次请求
        record.request_times.push(now);

        Ok(true)
    }

    /// 记录认证失败
    ///
    /// # 参数
    /// - `username`: 用户名
    pub async fn record_failure(&self, username: &str) {
        let mut attempts = self.failed_attempts.lock().await;
        let now = Instant::now();

        let record = attempts.entry(username.to_string()).or_insert(FailedAttempt {
            count: 0,
            first_attempt: now,
            locked_until: None,
        });

        record.count += 1;

        // 达到失败阈值，触发锁定
        if record.count >= self.max_failed_attempts {
            record.locked_until = Some(now + self.lockout_duration);
            tracing::warn!(
                "用户名触发锁定: username={}, failed_attempts={}, lockout_duration={:?}",
                username,
                record.count,
                self.lockout_duration
            );

            // 记录到审计日志
            tracing::info!(
                "SECURITY_EVENT: account_locked username={} failed_attempts={} lockout_duration_secs={}",
                username,
                record.count,
                self.lockout_duration.as_secs()
            );
        } else {
            tracing::info!(
                "认证失败记录: username={}, failed_attempts={}, max={}",
                username,
                record.count,
                self.max_failed_attempts
            );
        }
    }

    /// 清除失败记录（认证成功后）
    ///
    /// # 参数
    /// - `username`: 用户名
    pub async fn clear_failures(&self, username: &str) {
        let mut attempts = self.failed_attempts.lock().await;

        if attempts.remove(username).is_some() {
            tracing::info!("清除用户失败记录: username={}", username);
        }
    }

    /// 定期清理过期的记录（防止内存泄漏）
    ///
    /// 应该在后台任务中定期调用（如每5分钟）
    pub async fn cleanup_expired(&self) {
        // 清理过期的失败记录
        {
            let mut attempts = self.failed_attempts.lock().await;
            let now = Instant::now();

            let expired: Vec<String> = attempts
                .iter()
                .filter(|(_, record)| {
                    // 锁定期已过且超过1小时无活动
                    if let Some(locked_until) = record.locked_until {
                        now > locked_until + Duration::from_secs(3600)
                    } else {
                        // 未锁定但超过1小时
                        now.duration_since(record.first_attempt) > Duration::from_secs(3600)
                    }
                })
                .map(|(username, _)| username.clone())
                .collect();

            for username in &expired {
                attempts.remove(username);
                tracing::debug!("清理过期失败记录: username={}", username);
            }

            tracing::info!(
                "清理失败记录完成: expired={}, remaining={}",
                expired.len(),
                attempts.len()
            );
        }

        // 清理IP请求记录
        {
            let mut ip_reqs = self.ip_requests.lock().await;
            let now = Instant::now();

            // 只保留最近10分钟有活动的IP
            ip_reqs.retain(|_ip, record| {
                record.request_times.iter().any(|&time| {
                    now.duration_since(time) < Duration::from_secs(600)
                })
            });

            tracing::info!("清理IP记录完成: remaining_ips={}", ip_reqs.len());
        }
    }

    /// 获取当前统计信息（用于监控）
    #[allow(dead_code)]
    pub async fn get_stats(&self) -> AuthRateLimiterStats {
        let attempts = self.failed_attempts.lock().await;
        let ip_reqs = self.ip_requests.lock().await;

        let locked_count = attempts
            .values()
            .filter(|r| r.locked_until.is_some() && Instant::now() < r.locked_until.unwrap())
            .count();

        AuthRateLimiterStats {
            total_failed_attempts: attempts.len(),
            locked_accounts: locked_count,
            active_ips: ip_reqs.len(),
        }
    }
}

impl Default for AuthRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// 统计信息
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AuthRateLimiterStats {
    /// 总失败尝试数
    pub total_failed_attempts: usize,
    /// 被锁定的账户数
    pub locked_accounts: usize,
    /// 活跃IP数
    pub active_ips: usize,
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_username_lockout() {
        let limiter = AuthRateLimiter::new();

        // 初始应该允许
        assert!(limiter.is_username_allowed("testuser").await.unwrap());

        // 模拟5次失败
        for _ in 0..5 {
            limiter.record_failure("testuser").await;
        }

        // 第6次应该被锁定
        assert!(!limiter.is_username_allowed("testuser").await.unwrap());
    }

    #[tokio::test]
    async fn test_ip_rate_limit() {
        let limiter = AuthRateLimiter::new();

        // 模拟10次请求（达到限制）
        for _ in 0..10 {
            assert!(limiter.is_ip_allowed("192.168.1.1").await.unwrap());
        }

        // 第11次应该被拒绝
        assert!(!limiter.is_ip_allowed("192.168.1.1").await.unwrap());
    }

    #[tokio::test]
    async fn test_clear_failures() {
        let limiter = AuthRateLimiter::new();

        // 模拟失败
        limiter.record_failure("testuser").await;
        limiter.clear_failures("testuser").await;

        // 应该允许
        assert!(limiter.is_username_allowed("testuser").await.unwrap());
    }
}