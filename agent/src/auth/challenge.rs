//! 挑战-响应认证管理
//!
//! 提供防重放攻击的挑战-响应机制

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use uuid::Uuid;

/// 挑战记录
struct ChallengeRecord {
    /// 挑战数据
    challenge: Vec<u8>,
    /// 创建时间
    created_at: Instant,
    /// 用户名
    username: String,
    /// 公钥
    public_key: Vec<u8>,
}

/// 挑战管理器
pub struct ChallengeManager {
    /// 挑战缓存（challenge_id -> ChallengeRecord）
    challenges: Arc<Mutex<HashMap<String, ChallengeRecord>>>,
    /// 挑战有效期（秒）
    ttl_secs: u64,
}

impl ChallengeManager {
    /// 创建新的挑战管理器
    pub fn new() -> Self {
        Self {
            challenges: Arc::new(Mutex::new(HashMap::new())),
            ttl_secs: 60, // 默认60秒有效期
        }
    }

    /// 生成新的挑战
    ///
    /// # 参数
    /// - `username`: 用户名
    /// - `public_key`: 公钥数据
    ///
    /// # 返回
    /// - (challenge_id, challenge_data)
    pub async fn generate_challenge(
        &self,
        username: String,
        public_key: Vec<u8>,
    ) -> Result<(String, Vec<u8>)> {
        // 生成随机挑战数据（32字节）
        let challenge = Self::generate_random_bytes(32);
        let challenge_id = Uuid::new_v4().to_string();

        // 缓存挑战记录
        let record = ChallengeRecord {
            challenge: challenge.clone(),
            created_at: Instant::now(),
            username,
            public_key,
        };

        let mut challenges = self.challenges.lock().await;
        challenges.insert(challenge_id.clone(), record);

        tracing::debug!(
            "Generated challenge for user: challenge_id={}, len={}",
            challenge_id,
            challenge.len()
        );

        Ok((challenge_id, challenge))
    }

    /// 验证挑战响应
    ///
    /// # 参数
    /// - `challenge_id`: 挑战ID
    /// - `signature`: 签名数据
    /// - `public_key`: 公钥数据
    ///
    /// # 返回
    /// - Ok((username, expected_public_key)): 验证成功，返回用户名和公钥
    /// - Err: 验证失败
    pub async fn verify_response(
        &self,
        challenge_id: &str,
        _signature: &[u8],
        public_key: &[u8],
    ) -> Result<(String, Vec<u8>, Vec<u8>)> {
        let mut challenges = self.challenges.lock().await;

        // 查找挑战记录
        let record = challenges
            .remove(challenge_id)
            .ok_or_else(|| anyhow::anyhow!("Challenge not found or expired"))?;

        // 检查挑战是否过期
        let elapsed = record.created_at.elapsed();
        if elapsed > Duration::from_secs(self.ttl_secs) {
            return Err(anyhow::anyhow!(
                "Challenge expired (elapsed: {:?})",
                elapsed
            ));
        }

        // 验证公钥匹配
        if record.public_key != public_key {
            return Err(anyhow::anyhow!("Public key mismatch"));
        }

        // 返回用户名、挑战数据和公钥（供签名验证使用）
        Ok((record.username, record.challenge, record.public_key))
    }

    /// 清理过期挑战
    #[allow(dead_code)]
    pub async fn cleanup_expired(&self) {
        let mut challenges = self.challenges.lock().await;
        let now = Instant::now();
        let ttl = Duration::from_secs(self.ttl_secs);

        let expired: Vec<String> = challenges
            .iter()
            .filter(|(_, record)| now.duration_since(record.created_at) > ttl)
            .map(|(id, _)| id.clone())
            .collect();

        for id in expired {
            challenges.remove(&id);
        }
    }

    /// 生成随机字节
    fn generate_random_bytes(len: usize) -> Vec<u8> {
        use rand::RngCore;
        let mut bytes = vec![0u8; len];
        rand::thread_rng().fill_bytes(&mut bytes);
        bytes
    }
}

impl Default for ChallengeManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_challenge_generation() {
        let manager = ChallengeManager::new();

        // 生成挑战
        let (challenge_id, challenge) = manager
            .generate_challenge("testuser".to_string(), vec![1, 2, 3])
            .await
            .unwrap();

        assert!(!challenge_id.is_empty());
        assert_eq!(challenge.len(), 32); // 32字节随机数据
    }

    #[tokio::test]
    async fn test_challenge_verification() {
        let manager = ChallengeManager::new();

        // 生成挑战
        let (challenge_id, challenge) = manager
            .generate_challenge("testuser".to_string(), vec![1, 2, 3])
            .await
            .unwrap();

        // 验证响应
        let result = manager
            .verify_response(&challenge_id, &[], &[1, 2, 3])
            .await
            .unwrap();

        assert_eq!(result.0, "testuser");
        assert_eq!(result.1, challenge);
    }

    #[tokio::test]
    async fn test_challenge_not_found() {
        let manager = ChallengeManager::new();

        // 尝试验证不存在的挑战
        let result = manager.verify_response("nonexistent", &[], &[1, 2, 3]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_public_key_mismatch() {
        let manager = ChallengeManager::new();

        // 生成挑战
        let (challenge_id, _) = manager
            .generate_challenge("testuser".to_string(), vec![1, 2, 3])
            .await
            .unwrap();

        // 使用不同的公钥验证
        let result = manager.verify_response(&challenge_id, &[], &[4, 5, 6]).await;
        assert!(result.is_err());
    }
}