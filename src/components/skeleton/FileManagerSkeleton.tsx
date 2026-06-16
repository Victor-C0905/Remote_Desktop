/**
 * FileManager 骨架屏组件
 * 在 FileManager 数据预加载期间展示，布局与真实 FileManager 完全一致
 */
export function FileManagerSkeleton() {
  return (
    <div className="fm">
      {/* HeaderBar 骨架 */}
      <div className="fm-headerbar">
        <div className="skeleton-shimmer" style={{ width: 32, height: 28, borderRadius: 6, display: 'inline-block' }} />
        <div className="skeleton-shimmer" style={{ width: 32, height: 28, borderRadius: 6, display: 'inline-block', marginLeft: 6 }} />
        <div className="skeleton-shimmer" style={{ width: 32, height: 28, borderRadius: 6, display: 'inline-block', marginLeft: 6 }} />
        <div className="skeleton-shimmer" style={{ flex: 1, height: 28, borderRadius: 6, display: 'inline-block', marginLeft: 12 }} />
        <div className="skeleton-shimmer" style={{ width: 80, height: 28, borderRadius: 6, display: 'inline-block', marginLeft: 8 }} />
      </div>

      {/* Content 骨架 */}
      <div className="fm-content">
        {/* 侧边栏骨架 */}
        <div className="fm-skeleton-sidebar">
          <div>
            <div className="skeleton-shimmer fm-skeleton-section-title" />
            {[...Array(6)].map((_, i) => (
              <div key={i} className="skeleton-shimmer fm-skeleton-sidebar-item" />
            ))}
          </div>
          <div>
            <div className="skeleton-shimmer fm-skeleton-section-title" />
            {[...Array(3)].map((_, i) => (
              <div key={i} className="skeleton-shimmer fm-skeleton-sidebar-item" />
            ))}
          </div>
        </div>

        {/* 主内容区骨架 */}
        <div className="fm-skeleton-main">
          {/* 列表头骨架 */}
          <div style={{ display: 'flex', gap: 8, marginBottom: 8 }}>
            <div className="skeleton-shimmer" style={{ flex: 3, height: 24 }} />
            <div className="skeleton-shimmer" style={{ flex: 1, height: 24 }} />
            <div className="skeleton-shimmer" style={{ flex: 2, height: 24 }} />
            <div className="skeleton-shimmer" style={{ flex: 1, height: 24 }} />
          </div>
          {/* 列表行骨架 */}
          {[...Array(10)].map((_, i) => (
            <div key={i} className="skeleton-shimmer fm-skeleton-row" />
          ))}
        </div>
      </div>

      {/* StatusBar 骨架 */}
      <div className="fm-statusbar">
        <div className="skeleton-shimmer" style={{ width: 140, height: 14, borderRadius: 4 }} />
        <div className="skeleton-shimmer" style={{ width: 100, height: 14, borderRadius: 4 }} />
      </div>
    </div>
  );
}
