// src/config/subscription.ts

export const SUBSCRIPTION_CONFIG = {
  // 默认推送间隔（与 Agent 保持一致）
  DEFAULT_INTERVAL_SECS: {
    metrics: 1,
    process_events: 2,
    service_status: 5,
  },

  // 文件变化监听延迟
  FILE_CHANGES_DELAY_MS: 100,

  // 最大历史数据长度（秒）
  HISTORY_LENGTH: 120, // 2 分钟历史

  // 图表刷新频率（前端渲染）
  CHART_REFRESH_MS: 1000,
};

export interface SubscriptionType {
  type: 'metrics' | 'file_changes' | 'process_events' | 'app_logs' | 'service_status';
  params?: {
    interval_secs?: number;
    path?: string;
    recursive?: boolean;
    app_name?: string;
    level?: string;
    service?: string;
  };
}

export interface SubscriptionEvent {
  server_id: string;
  event_type: string;
  data: any;
  timestamp: number;
}