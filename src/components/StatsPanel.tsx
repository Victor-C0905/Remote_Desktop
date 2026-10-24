import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
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
  const { t } = useTranslation();
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
      setError(err instanceof Error ? err.message : t('stats.loadFailed'));
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
    return <div className="stats-panel">{t('stats.loading')}</div>;
  }

  if (error) {
    return (
      <div className="stats-panel">
        <div className="error">{error}</div>
        <button onClick={fetchStats}>{t('common.retry')}</button>
      </div>
    );
  }

  if (!stats) {
    return null;
  }

  return (
    <div className="stats-panel">
      <h2>{t('stats.title')}</h2>

      {/* 认证统计（仅root可见） */}
      {isRoot && stats.auth && (
        <div className="stats-section">
          <h3>{t('stats.authTitle')}</h3>
          <div className="stats-grid">
            <div className="stat-item">
              <span className="label">{t('stats.totalAttempts')}</span>
              <span className="value">{stats.auth.total_attempts}</span>
            </div>
            <div className="stat-item">
              <span className="label">{t('stats.successful')}</span>
              <span className="value success">{stats.auth.successful}</span>
            </div>
            <div className="stat-item">
              <span className="label">{t('stats.failed')}</span>
              <span className="value error">{stats.auth.failed}</span>
            </div>
            <div className="stat-item">
              <span className="label">{t('stats.locked')}</span>
              <span className="value warning">{stats.auth.locked}</span>
            </div>
            <div className="stat-item">
              <span className="label">{t('stats.rateLimited')}</span>
              <span className="value">{stats.auth.rate_limited}</span>
            </div>
            <div className="stat-item">
              <span className="label">{t('stats.sessionTimeout')}</span>
              <span className="value">{stats.auth.session_timeout}</span>
            </div>
          </div>
        </div>
      )}

      {/* 连接统计（所有用户可见） */}
      <div className="stats-section">
        <h3>{t('stats.connTitle')}</h3>
        <div className="stats-grid">
          <div className="stat-item">
            <span className="label">{t('stats.activeConnections')}</span>
            <span className="value">{stats.connection.active_connections}</span>
          </div>
          <div className="stat-item">
            <span className="label">{t('stats.totalConnections')}</span>
            <span className="value">{stats.connection.total_connections}</span>
          </div>
          <div className="stat-item">
            <span className="label">{t('stats.normalDisconnects')}</span>
            <span className="value success">{stats.connection.normal_disconnects}</span>
          </div>
          <div className="stat-item">
            <span className="label">{t('stats.timeoutDisconnects')}</span>
            <span className="value warning">{stats.connection.timeout_disconnects}</span>
          </div>
          <div className="stat-item">
            <span className="label">{t('stats.errorDisconnects')}</span>
            <span className="value error">{stats.connection.error_disconnects}</span>
          </div>
        </div>
      </div>

      {/* 性能指标（仅root可见） */}
      {isRoot && stats.performance && (
        <div className="stats-section">
          <h3>{t('stats.perfTitle')}</h3>
          <div className="stats-grid">
            <div className="stat-item">
              <span className="label">{t('stats.responseTimeP50')}</span>
              <span className="value">{stats.performance.response_times.p50}ms</span>
            </div>
            <div className="stat-item">
              <span className="label">{t('stats.responseTimeP95')}</span>
              <span className="value">{stats.performance.response_times.p95}ms</span>
            </div>
            <div className="stat-item">
              <span className="label">{t('stats.responseTimeP99')}</span>
              <span className="value">{stats.performance.response_times.p99}ms</span>
            </div>
            <div className="stat-item">
              <span className="label">{t('stats.transfer')}</span>
              <span className="value">{formatBytes(stats.performance.total_bytes_transferred)}</span>
            </div>
            <div className="stat-item">
              <span className="label">{t('stats.terminalOutput')}</span>
              <span className="value">{formatBytes(stats.performance.total_terminal_bytes)}</span>
            </div>
          </div>
        </div>
      )}

      <button onClick={fetchStats} className="refresh-button">
        {t('stats.refresh')}
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