import { useState, useEffect } from "react";
import "./Settings.css";

/* ── Types ─────────────────────────────────────────────── */

interface ServerConfig {
  id: string;
  name: string;
  host: string;
  port: number;
  lastConnected?: string;
  status: "connected" | "disconnected" | "error";
}

type SettingsSection = "connection" | "appearance" | "keyboard" | "files" | "terminal" | "notifications" | "about";

/* ── Demo Server Data ────────────────────────────────── */

const DEMO_SERVERS: ServerConfig[] = [
  { id: "srv-1", name: "prod-server", host: "prod.example.com", port: 8443, status: "connected", lastConnected: "刚刚" },
  { id: "srv-2", name: "dev-server", host: "dev.example.com", port: 8443, status: "disconnected", lastConnected: "2 小时前" },
  { id: "srv-3", name: "staging-server", host: "staging.example.com", port: 8443, status: "disconnected", lastConnected: "昨天" },
];

/* ── Sidebar Items ──────────────────────────────────── */

interface SidebarItem {
  id: SettingsSection;
  icon: string;
  label: string;
}

const SIDEBAR_ITEMS: SidebarItem[] = [
  { id: "connection", icon: "🔗", label: "连接" },
  { id: "appearance", icon: "🎨", label: "外观" },
  { id: "keyboard", icon: "⌨️", label: "快捷键" },
  { id: "files", icon: "📁", label: "文件" },
  { id: "terminal", icon: "🖥️", label: "终端" },
  { id: "notifications", icon: "🔔", label: "通知" },
  { id: "about", icon: "ℹ️", label: "关于" },
];

/* ── Main Component ─────────────────────────────────── */

export function Settings() {
  const [activeSection, setActiveSection] = useState<SettingsSection>("connection");
  const [servers, setServers] = useState<ServerConfig[]>(DEMO_SERVERS);
  const [selectedServerId, setSelectedServerId] = useState<string | null>("srv-1");
  const [theme, setTheme] = useState<"light" | "dark">("light");
  const [accentColor, setAccentColor] = useState("#3584e4");

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
  }, [theme]);

  const selectedServer = servers.find(s => s.id === selectedServerId);

  const renderSection = () => {
    switch (activeSection) {
      case "connection":
        return (
          <div className="st-section">
            <div className="st-section-title">连接服务器配置</div>

            {/* Current Connection */}
            <div className="st-card">
              <div className="st-card-header">当前连接</div>
              <div className="st-current-connection">
                <div className="st-conn-row">
                  <span className="st-conn-label">主机</span>
                  <span className="st-conn-value">{selectedServer?.host || "未连接"}</span>
                </div>
                <div className="st-conn-row">
                  <span className="st-conn-label">端口</span>
                  <span className="st-conn-value">{selectedServer?.port || "—"} (QUIC)</span>
                </div>
                <div className="st-conn-row">
                  <span className="st-conn-label">状态</span>
                  <span className="st-conn-value">
                    <span className={`st-status-dot ${selectedServer?.status || "disconnected"}`} />
                    {selectedServer?.status === "connected" ? "已连接" : "未连接"}
                  </span>
                </div>
                <div className="st-conn-row">
                  <span className="st-conn-label">延迟</span>
                  <span className="st-conn-value">6 ms</span>
                </div>
              </div>
              <div className="st-card-actions">
                <button className="st-btn st-btn-danger">断开连接</button>
              </div>
            </div>

            {/* Saved Servers */}
            <div className="st-card">
              <div className="st-card-header">已保存的服务器</div>
              <div className="st-server-list">
                {servers.map(server => (
                  <div
                    key={server.id}
                    className={`st-server-item ${selectedServerId === server.id ? "selected" : ""}`}
                    onClick={() => setSelectedServerId(server.id)}
                  >
                    <span className={`st-server-status ${server.status}`} />
                    <span className="st-server-name">{server.name}</span>
                    <span className="st-server-host">{server.host}</span>
                    <span className="st-server-time">{server.lastConnected || "从未"}</span>
                  </div>
                ))}
              </div>
              <div className="st-card-actions">
                <button className="st-btn st-btn-primary">+ 添加服务器</button>
              </div>
            </div>
          </div>
        );

      case "appearance":
        return (
          <div className="st-section">
            <div className="st-section-title">外观设置</div>

            {/* Theme */}
            <div className="st-card">
              <div className="st-card-header">主题</div>
              <div className="st-theme-toggle">
                <button
                  className={`st-theme-btn ${theme === "light" ? "active" : ""}`}
                  onClick={() => setTheme("light")}
                >
                  <span className="st-theme-icon">☀️</span>
                  <span className="st-theme-label">亮色</span>
                </button>
                <button
                  className={`st-theme-btn ${theme === "dark" ? "active" : ""}`}
                  onClick={() => setTheme("dark")}
                >
                  <span className="st-theme-icon">🌙</span>
                  <span className="st-theme-label">暗色</span>
                </button>
              </div>
            </div>

            {/* Accent Color */}
            <div className="st-card">
              <div className="st-card-header">强调色</div>
              <div className="st-accent-colors">
                {["#3584e4", "#2ec27e", "#e66100", "#c061cb", "#f6d32d", "#26a269"].map(color => (
                  <button
                    key={color}
                    className={`st-accent-btn ${accentColor === color ? "selected" : ""}`}
                    style={{ background: color }}
                    onClick={() => setAccentColor(color)}
                  />
                ))}
              </div>
            </div>

            {/* Font Size */}
            <div className="st-card">
              <div className="st-card-header">字体大小</div>
              <div className="st-slider-row">
                <span className="st-slider-label">界面字体</span>
                <input type="range" min="8" max="14" defaultValue="10" className="st-slider" />
                <span className="st-slider-value">10pt</span>
              </div>
              <div className="st-slider-row">
                <span className="st-slider-label">终端字体</span>
                <input type="range" min="10" max="16" defaultValue="13" className="st-slider" />
                <span className="st-slider-value">13pt</span>
              </div>
            </div>
          </div>
        );

      case "keyboard":
        return (
          <div className="st-section">
            <div className="st-section-title">快捷键</div>
            <div className="st-card">
              <div className="st-card-header">全局快捷键</div>
              <div className="st-shortcut-list">
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action">打开活动概览</span>
                  <span className="st-shortcut-key">Super</span>
                </div>
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action">打开文件管理器</span>
                  <span className="st-shortcut-key">Super + 1</span>
                </div>
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action">打开终端</span>
                  <span className="st-shortcut-key">Super + 2</span>
                </div>
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action">打开系统监控</span>
                  <span className="st-shortcut-key">Super + 3</span>
                </div>
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action">打开设置</span>
                  <span className="st-shortcut-key">Super + 4</span>
                </div>
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action">切换终端标签</span>
                  <span className="st-shortcut-key">Ctrl + Tab</span>
                </div>
              </div>
            </div>
          </div>
        );

      case "files":
        return (
          <div className="st-section">
            <div className="st-section-title">文件管理设置</div>
            <div className="st-card">
              <div className="st-card-header">默认视图</div>
              <div className="st-option-row">
                <span className="st-option-label">默认视图模式</span>
                <select className="st-select">
                  <option>列表视图</option>
                  <option>网格视图</option>
                </select>
              </div>
              <div className="st-option-row">
                <span className="st-option-label">显示隐藏文件</span>
                <input type="checkbox" className="st-checkbox" />
              </div>
              <div className="st-option-row">
                <span className="st-option-label">排序方式</span>
                <select className="st-select">
                  <option>名称</option>
                  <option>大小</option>
                  <option>修改时间</option>
                </select>
              </div>
            </div>
            <div className="st-card">
              <div className="st-card-header">传输设置</div>
              <div className="st-option-row">
                <span className="st-option-label">最大传输大小</span>
                <input type="number" className="st-input" defaultValue="1000" />
                <span className="st-input-unit">MB</span>
              </div>
            </div>
          </div>
        );

      case "terminal":
        return (
          <div className="st-section">
            <div className="st-section-title">终端设置</div>
            <div className="st-card">
              <div className="st-card-header">配色方案</div>
              <div className="st-theme-toggle">
                <button className="st-theme-btn active">
                  <span className="st-theme-icon">⬛</span>
                  <span className="st-theme-label">暗色</span>
                </button>
                <button className="st-theme-btn">
                  <span className="st-theme-icon">⬜</span>
                  <span className="st-theme-label">亮色</span>
                </button>
                <button className="st-theme-btn">
                  <span className="st-theme-icon">🔲</span>
                  <span className="st-theme-label">白底</span>
                </button>
              </div>
            </div>
            <div className="st-card">
              <div className="st-card-header">终端选项</div>
              <div className="st-option-row">
                <span className="st-option-label">光标样式</span>
                <select className="st-select">
                  <option>方块</option>
                  <option>竖线</option>
                  <option>下划线</option>
                </select>
              </div>
              <div className="st-option-row">
                <span className="st-option-label">光标闪烁</span>
                <input type="checkbox" className="st-checkbox" defaultChecked />
              </div>
              <div className="st-option-row">
                <span className="st-option-label">滚动缓冲区</span>
                <input type="number" className="st-input" defaultValue="5000" />
                <span className="st-input-unit">行</span>
              </div>
            </div>
          </div>
        );

      case "notifications":
        return (
          <div className="st-section">
            <div className="st-section-title">通知设置</div>
            <div className="st-card">
              <div className="st-card-header">通知类型</div>
              <div className="st-option-row">
                <span className="st-option-label">系统监控警报</span>
                <input type="checkbox" className="st-checkbox" defaultChecked />
              </div>
              <div className="st-option-row">
                <span className="st-option-label">文件操作完成</span>
                <input type="checkbox" className="st-checkbox" defaultChecked />
              </div>
              <div className="st-option-row">
                <span className="st-option-label">连接状态变化</span>
                <input type="checkbox" className="st-checkbox" defaultChecked />
              </div>
            </div>
            <div className="st-card">
              <div className="st-card-header">警报阈值</div>
              <div className="st-slider-row">
                <span className="st-slider-label">CPU 使用率警报</span>
                <input type="range" min="50" max="100" defaultValue="80" className="st-slider" />
                <span className="st-slider-value">80%</span>
              </div>
              <div className="st-slider-row">
                <span className="st-slider-label">磁盘使用率警报</span>
                <input type="range" min="50" max="100" defaultValue="90" className="st-slider" />
                <span className="st-slider-value">90%</span>
              </div>
            </div>
          </div>
        );

      case "about":
        return (
          <div className="st-section">
            <div className="st-section-title">关于</div>
            <div className="st-card st-about-card">
              <div className="st-about-logo">🖥️</div>
              <div className="st-about-name">GNOME Remote</div>
              <div className="st-about-version">版本 0.1.0</div>
              <div className="st-about-desc">
                基于 GNOME 设计系统的远程 Linux 服务器控制客户端
              </div>
              <div className="st-about-tech">
                <span>Tauri 2.x + React 18</span>
                <span>QUIC 传输</span>
                <span>Adwaita 设计</span>
              </div>
            </div>
            <div className="st-card">
              <div className="st-card-header">技术栈</div>
              <div className="st-tech-list">
                <div className="st-tech-item">
                  <span className="st-tech-name">前端框架</span>
                  <span className="st-tech-value">React 19 + TypeScript</span>
                </div>
                <div className="st-tech-item">
                  <span className="st-tech-name">桌面框架</span>
                  <span className="st-tech-value">Tauri 2.x (Rust)</span>
                </div>
                <div className="st-tech-item">
                  <span className="st-tech-name">终端模拟</span>
                  <span className="st-tech-value">xterm.js 5.5</span>
                </div>
                <div className="st-tech-item">
                  <span className="st-tech-name">设计系统</span>
                  <span className="st-tech-value">GNOME Adwaita</span>
                </div>
              </div>
            </div>
          </div>
        );

      default:
        return null;
    }
  };

  return (
    <div className="st">
      {/* Header Bar */}
      <div className="st-headerbar">
        <button className="st-back-btn" title="返回">←</button>
        <span className="st-title">
          {SIDEBAR_ITEMS.find(i => i.id === activeSection)?.label || "设置"}
        </span>
        <div className="st-headerbar-spacer" />
        <button className="st-search-btn" title="搜索设置">🔍</button>
      </div>

      {/* Content */}
      <div className="st-content">
        {/* Sidebar */}
        <div className="st-sidebar">
          {SIDEBAR_ITEMS.map(item => (
            <div
              key={item.id}
              className={`st-sidebar-item ${activeSection === item.id ? "active" : ""}`}
              onClick={() => setActiveSection(item.id)}
            >
              <span className="st-sb-icon">{item.icon}</span>
              <span className="st-sb-label">{item.label}</span>
            </div>
          ))}
        </div>

        {/* Main Area */}
        <div className="st-main">
          {renderSection()}
        </div>
      </div>
    </div>
  );
}