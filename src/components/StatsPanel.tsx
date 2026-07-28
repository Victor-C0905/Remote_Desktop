import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import './StatsPanel.css';

interface AuthStats {
  total_attempts: number;
  successful: number;
  failed: number;
  locked: number;
  rate_limited: number;
  session_timeout: number;
  password_attempts: number;
  pubkey_attempts: number;
}

interface ConnectionStats {
  active_connections: number;
  total_connections: number;
  normal_disconnects: number;
  timeout_disconnects: number;
  error_disconnects: number;
}

interface PerformanceStats {
  response_times: {
    p50: number;
    p95: number;
    p99: number;
    min: number;
    max: number;
    count: number;
  };
  total_bytes_transferred: number;
  total_terminal_bytes: number;
}

interface StatsResponse {
  auth?: AuthStats;
  connection: ConnectionStats;
  performance?: PerformanceStats;
}

interface StatsPanelProps {
  serverId: string;
  isRoot: boolean;
}

export const StatsPanel: React.FC<StatsPanelProps> = ({ serverId, isRoot }) => {
  const [stats, setStats] = useState<StatsResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchStats = async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await invoke<StatsResponse>('get_stats', {
        serverId,
        statsType: 'all',
      });
      setStats(response);
    } catch (err) {
      setError(err instanceof Error ? err.message : '获取统计失败');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchStats();
    const interval = setInterval(fetchStats, 30000); // 每30秒刷新
    return () => clearInterval(interval);
  }, [serverId]);

  if (loading) {
    return <div className="stats-panel">加载中...</div>;
  }

  if (error) {
    return (
      <div className="stats-panel">
        <div className="error">{error}</div>
        <button onClick={fetchStats}>重试</button>
      </div>
    );
  }

  if (!stats) {
    return null;
  }

  return (
    <div className="stats-panel">
      <h2>📊 系统监控</h2>

      {/* 认证统计（仅root可见） */}
      {isRoot && stats.auth && (
        <div className="stats-section">
          <h3>认证统计</h3>
          <div className="stats-grid">
            <div className="stat-item">
              <span className="label">总尝试次数</span>
              <span className="value">{stats.auth.total_attempts}</span>
            </div>
            <div className="stat-item">
              <span className="label">成功次数</span>
              <span className="value success">{stats.auth.successful}</span>
            </div>
            <div className="stat-item">
              <span className="label">失败次数</span>
              <span className="value error">{stats.auth.failed}</span>
            </div>
            <div className="stat-item">
              <span className="label">锁定次数</span>
              <span className="value warning">{stats.auth.locked}</span>
            </div>
            <div className="stat-item">
              <span className="label">速率限制</span>
              <span className="value">{stats.auth.rate_limited}</span>
            </div>
            <div className="stat-item">
              <span className="label">会话超时</span>
              <span className="value">{stats.auth.session_timeout}</span>
            </div>
          </div>
        </div>
      )}

      {/* 连接统计（所有用户可见） */}
      <div className="stats-section">
        <h3>连接统计</h3>
        <div className="stats-grid">
          <div className="stat-item">
            <span className="label">活跃连接</span>
            <span className="value">{stats.connection.active_connections}</span>
          </div>
          <div className="stat-item">
            <span className="label">总连接数</span>
            <span className="value">{stats.connection.total_connections}</span>
          </div>
          <div className="stat-item">
            <span className="label">正常断开</span>
            <span className="value success">{stats.connection.normal_disconnects}</span>
          </div>
          <div className="stat-item">
            <span className="label">超时断开</span>
            <span className="value warning">{stats.connection.timeout_disconnects}</span>
          </div>
          <div className="stat-item">
            <span className="label">错误断开</span>
            <span className="value error">{stats.connection.error_disconnects}</span>
          </div>
        </div>
      </div>

      {/* 性能指标（仅root可见） */}
      {isRoot && stats.performance && (
        <div className="stats-section">
          <h3>性能指标</h3>
          <div className="stats-grid">
            <div className="stat-item">
              <span className="label">响应时间(p50)</span>
              <span className="value">{stats.performance.response_times.p50}ms</span>
            </div>
            <div className="stat-item">
              <span className="label">响应时间(p95)</span>
              <span className="value">{stats.performance.response_times.p95}ms</span>
            </div>
            <div className="stat-item">
              <span className="label">响应时间(p99)</span>
              <span className="value">{stats.performance.response_times.p99}ms</span>
            </div>
            <div className="stat-item">
              <span className="label">文件传输</span>
              <span className="value">{formatBytes(stats.performance.total_bytes_transferred)}</span>
            </div>
            <div className="stat-item">
              <span className="label">终端输出</span>
              <span className="value">{formatBytes(stats.performance.total_terminal_bytes)}</span>
            </div>
          </div>
        </div>
      )}

      <button onClick={fetchStats} className="refresh-button">
        刷新数据
      </button>
    </div>
  );
};

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}