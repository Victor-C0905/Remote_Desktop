/**
 * Terminal 骨架屏组件
 * 在 xterm.js 动态加载期间展示，模拟真实终端外观（深色背景 + 光标闪烁）
 */
export function TerminalSkeleton() {
  return (
    <div className="terminal-app">
      {/* TabBar 骨架 */}
      <div className="terminal-tab-bar">
        <div className="skeleton-shimmer" style={{ width: 100, height: 32, borderRadius: 6 }} />
        <div style={{ flex: 1 }} />
        <div className="skeleton-shimmer" style={{ width: 28, height: 28, borderRadius: 14 }} />
      </div>

      {/* 终端区域骨架 — 模拟终端外观 */}
      <div className="terminal-skeleton">
        <div
          style={{
            color: 'rgba(255,255,255,0.5)',
            fontSize: 13,
            fontFamily: "'Source Code Pro', 'Cascadia Code', monospace",
            lineHeight: 1.6,
            width: '100%',
          }}
        >
          <div className="terminal-skeleton-line" />
          <div className="terminal-skeleton-line" style={{ width: '45%' }} />
          <div className="terminal-skeleton-line" style={{ width: '70%' }} />
          <div className="terminal-skeleton-line" style={{ width: '30%' }} />
          {/* 模拟命令提示符 + 闪烁光标 */}
          <div style={{ marginTop: 8 }}>
            <span style={{ color: '#4ec9a066' }}>user@quirel</span>
            <span style={{ color: '#ffffff66' }}>:</span>
            <span style={{ color: '#6699ff66' }}>~</span>
            <span style={{ color: '#ffffff66' }}>{' $ '}</span>
            <span className="terminal-skeleton-cursor" />
          </div>
        </div>
      </div>

      {/* StatusBar 骨架 */}
      <div className="terminal-status-bar">
        <div className="status-item">
          <div
            className="skeleton-shimmer"
            style={{ width: 8, height: 8, borderRadius: '50%', display: 'inline-block' }}
          />
          <div
            className="skeleton-shimmer"
            style={{ width: 60, height: 12, borderRadius: 4, display: 'inline-block', marginLeft: 6 }}
          />
        </div>
        <div className="status-spacer" />
        <div className="skeleton-shimmer" style={{ width: 70, height: 12, borderRadius: 4 }} />
      </div>
    </div>
  );
}
