/**
 * SystemMonitor 骨架屏组件
 * 在系统指标数据加载期间展示，布局与真实监控面板一致
 */
export function MonitorSkeleton() {
  return (
    <div className="sm">
      {/* HeaderBar 骨架：Tab 栏 + 菜单按钮 */}
      <div className="sm-headerbar">
        <div className="sm-skeleton-tabs">
          <div className="skeleton-shimmer sm-skeleton-tab" />
          <div className="skeleton-shimmer sm-skeleton-tab" />
          <div className="skeleton-shimmer sm-skeleton-tab" />
        </div>
        <div className="sm-headerbar-spacer" />
        <div className="skeleton-shimmer" style={{ width: 28, height: 28, borderRadius: 6 }} />
      </div>

      {/* Content 骨架：模拟资源页面布局 */}
      <div className="sm-skeleton-content">
        {/* CPU / 内存卡片行 */}
        <div style={{ display: 'flex', gap: 16, marginBottom: 16 }}>
          <div className="skeleton-shimmer sm-skeleton-card" />
          <div className="skeleton-shimmer sm-skeleton-card" />
        </div>

        {/* 内存/交换进度条行 */}
        <div className="skeleton-shimmer sm-skeleton-bar" />
        <div className="skeleton-shimmer sm-skeleton-bar" />

        {/* 网络 + 运行时间卡片行 */}
        <div style={{ display: 'flex', gap: 16, marginTop: 16 }}>
          <div className="skeleton-shimmer sm-skeleton-card" style={{ flex: 2 }} />
          <div className="skeleton-shimmer sm-skeleton-card" style={{ flex: 1 }} />
        </div>

        {/* 磁盘用量区域 */}
        <div style={{ marginTop: 16 }}>
          <div className="skeleton-shimmer" style={{ width: 100, height: 16, borderRadius: 4, marginBottom: 8 }} />
          <div className="skeleton-shimmer sm-skeleton-bar" />
          <div className="skeleton-shimmer sm-skeleton-bar" />
          <div className="skeleton-shimmer sm-skeleton-bar" />
        </div>
      </div>
    </div>
  );
}
