import { useState, useEffect } from "react";
import { useServerManager, formatLastConnected, getStatusIcon, getStatusColor } from "../context/ServerManager";
import { useWallpaper, getPresetWallpaperName, getWallpaperStyle } from "../context/WallpaperContext";
import { PRESET_WALLPAPERS } from "../context/WallpaperContext";
import { useTheme } from "../hooks/useTheme";
import "./Settings.css";

/* ── Types ─────────────────────────────────────────────── */

type SettingsSection = "connection" | "appearance" | "keyboard" | "files" | "terminal" | "notifications" | "about";

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

/* ── Add Server Modal ───────────────────────────────── */

interface AddServerModalProps {
  isOpen: boolean;
  onClose: () => void;
  onAdd: (name: string, host: string, port: number, token: string) => void;
}

function AddServerModal({ isOpen, onClose, onAdd }: AddServerModalProps) {
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState(8443);
  const [token, setToken] = useState("");

  if (!isOpen) return null;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (name && host && token) {
      onAdd(name, host, port, token);
      setName("");
      setHost("");
      setPort(8443);
      setToken("");
      onClose();
    }
  };

  return (
    <div className="st-modal-overlay" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="st-modal">
        <div className="st-modal-header">
          <span className="st-modal-title">添加服务器</span>
          <button className="st-modal-close" onClick={onClose}>×</button>
        </div>
        <form className="st-modal-body" onSubmit={handleSubmit}>
          <div className="st-form-row">
            <label className="st-form-label">服务器名称</label>
            <input
              type="text"
              className="st-form-input"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="例如: prod-server"
              autoFocus
            />
          </div>
          <div className="st-form-row">
            <label className="st-form-label">主机地址</label>
            <input
              type="text"
              className="st-form-input"
              value={host}
              onChange={(e) => setHost(e.target.value)}
              placeholder="例如: server.example.com 或 127.0.0.1"
            />
          </div>
          <div className="st-form-row">
            <label className="st-form-label">端口</label>
            <input
              type="number"
              className="st-form-input"
              value={port}
              onChange={(e) => setPort(parseInt(e.target.value) || 8443)}
              placeholder="8443"
            />
          </div>
          <div className="st-form-row">
            <label className="st-form-label">认证 Token</label>
            <input
              type="text"
              className="st-form-input"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="例如: gmr_xxxxxx-xxxx-xxxx-xxxx"
            />
            <span className="st-form-hint">从 Agent 日志中获取</span>
          </div>
        </form>
        <div className="st-modal-footer">
          <button className="st-btn" onClick={onClose}>取消</button>
          <button className="st-btn st-btn-primary" onClick={handleSubmit}>添加</button>
        </div>
      </div>
    </div>
  );
}

/* ── Main Component ─────────────────────────────────── */

export function Settings() {
  const [activeSection, setActiveSection] = useState<SettingsSection>("connection");
  const [addModalOpen, setAddModalOpen] = useState(false);
  const [selectedServerId, setSelectedServerId] = useState<string | null>(null);

  const {
    servers,
    activeServer,
    activeServerId,
    addServer,
    removeServer,
    connectServer,
    disconnectServer,
  } = useServerManager();

  const {
    wallpaper,
    setPresetWallpaper,
    importWallpaper,
    clearWallpaper,
  } = useWallpaper();

  const { theme, setTheme } = useTheme();
  
  const [accentColor, setAccentColorState] = useState(() => 
    localStorage.getItem("gnome-remote-accent") || "#3584e4"
  );
  const [fontSize, setFontSize] = useState(() => {
    const saved = localStorage.getItem("gnome-remote-font-size");
    return saved ? parseInt(saved) : 10;
  });
  const [terminalFontSize, setTerminalFontSize] = useState(() => {
    const saved = localStorage.getItem("gnome-remote-terminal-font");
    return saved ? parseInt(saved) : 13;
  });

  // Apply accent color
  useEffect(() => {
    document.documentElement.style.setProperty("--accent-bg", accentColor);
    localStorage.setItem("gnome-remote-accent", accentColor);
  }, [accentColor]);

  // Apply font size
  useEffect(() => {
    document.documentElement.style.setProperty("--font-body", `${fontSize}pt`);
    document.documentElement.style.setProperty("--font-title", `${fontSize + 1}pt`);
    document.documentElement.style.setProperty("--font-small", `${fontSize - 1}pt`);
    localStorage.setItem("gnome-remote-font-size", fontSize.toString());
  }, [fontSize]);

  useEffect(() => {
    localStorage.setItem("gnome-remote-terminal-font", terminalFontSize.toString());
  }, [terminalFontSize]);

  const handleAddServer = (name: string, host: string, port: number, token: string) => {
    addServer({ name, host, port, token });
  };

  const handleConnect = async (id: string) => {
    setSelectedServerId(id);
    await connectServer(id);
  };

  const handleDisconnect = (id: string) => {
    disconnectServer(id);
  };

  const handleRemove = (id: string) => {
    if (confirm("确定要删除此服务器配置吗？")) {
      removeServer(id);
    }
  };

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
                  <span className="st-conn-value">{activeServer?.host || "未连接"}</span>
                </div>
                <div className="st-conn-row">
                  <span className="st-conn-label">端口</span>
                  <span className="st-conn-value">{activeServer?.port || "—"} (QUIC)</span>
                </div>
                <div className="st-conn-row">
                  <span className="st-conn-label">状态</span>
                  <span className="st-conn-value">
                    <span 
                      className="st-status-dot" 
                      style={{ background: activeServer ? getStatusColor(activeServer.status) : "#9a9996" }}
                    />
                    {activeServer?.status === "connected" ? "已连接" : 
                     activeServer?.status === "connecting" ? "连接中..." :
                     activeServer?.status === "error" ? "错误" : "未连接"}
                  </span>
                </div>
                {activeServer?.status === "connected" && (
                  <div className="st-conn-row">
                    <span className="st-conn-label">延迟</span>
                    <span className="st-conn-value">6 ms</span>
                  </div>
                )}
                {activeServer?.error && (
                  <div className="st-conn-error">{activeServer.error}</div>
                )}
              </div>
              <div className="st-card-actions">
                {activeServer?.status === "connected" && (
                  <button 
                    className="st-btn st-btn-danger"
                    onClick={() => handleDisconnect(activeServer.id)}
                  >
                    断开连接
                  </button>
                )}
                {activeServer?.status === "error" && selectedServerId && (
                  <button 
                    className="st-btn st-btn-primary"
                    onClick={() => handleConnect(selectedServerId)}
                  >
                    重试连接
                  </button>
                )}
              </div>
            </div>

            {/* Saved Servers */}
            <div className="st-card">
              <div className="st-card-header">已保存的服务器</div>
              <div className="st-server-list">
                {servers.length === 0 ? (
                  <div className="st-server-empty">
                    <span>暂无保存的服务器</span>
                    <span className="st-server-empty-hint">点击下方按钮添加</span>
                  </div>
                ) : (
                  servers.map(server => (
                    <div
                      key={server.id}
                      className={`st-server-item ${activeServerId === server.id ? "selected" : ""}`}
                      onClick={() => setSelectedServerId(server.id)}
                    >
                      <span 
                        className="st-server-status" 
                        style={{ background: getStatusColor(server.status) }}
                      >
                        {getStatusIcon(server.status)}
                      </span>
                      <span className="st-server-name">{server.name}</span>
                      <span className="st-server-host">{server.host}</span>
                      <span className="st-server-time">{formatLastConnected(server.lastConnected)}</span>
                      <div className="st-server-actions">
                        {server.status === "disconnected" && (
                          <button 
                            className="st-server-action-btn"
                            onClick={(e) => { e.stopPropagation(); handleConnect(server.id); }}
                            title="连接"
                          >
                            🔗
                          </button>
                        )}
                        {server.status === "connected" && (
                          <button 
                            className="st-server-action-btn"
                            onClick={(e) => { e.stopPropagation(); handleDisconnect(server.id); }}
                            title="断开"
                          >
                            ⚡
                          </button>
                        )}
                        <button 
                          className="st-server-action-btn st-server-action-danger"
                          onClick={(e) => { e.stopPropagation(); handleRemove(server.id); }}
                          title="删除"
                        >
                          🗑️
                        </button>
                      </div>
                    </div>
                  ))
                )}
              </div>
              <div className="st-card-actions">
                <button 
                  className="st-btn st-btn-primary"
                  onClick={() => setAddModalOpen(true)}
                >
                  + 添加服务器
                </button>
              </div>
            </div>

            <AddServerModal
              isOpen={addModalOpen}
              onClose={() => setAddModalOpen(false)}
              onAdd={handleAddServer}
            />
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
                    onClick={() => setAccentColorState(color)}
                  />
                ))}
              </div>
            </div>

            {/* Font Size */}
            <div className="st-card">
              <div className="st-card-header">字体大小</div>
              <div className="st-slider-row">
                <span className="st-slider-label">界面字体</span>
                <input 
                  type="range" 
                  min="8" 
                  max="14" 
                  value={fontSize} 
                  onChange={(e) => setFontSize(parseInt(e.target.value))} 
                  className="st-slider" 
                />
                <span className="st-slider-value">{fontSize}pt</span>
              </div>
              <div className="st-slider-row">
                <span className="st-slider-label">终端字体</span>
                <input 
                  type="range" 
                  min="10" 
                  max="16" 
                  value={terminalFontSize} 
                  onChange={(e) => setTerminalFontSize(parseInt(e.target.value))} 
                  className="st-slider" 
                />
                <span className="st-slider-value">{terminalFontSize}pt</span>
              </div>
            </div>

            {/* Wallpaper */}
            <div className="st-card">
              <div className="st-card-header">壁纸</div>
              <div className="st-wallpaper-grid">
                {Object.keys(PRESET_WALLPAPERS).map(presetId => (
                  <button
                    key={presetId}
                    className={`st-wallpaper-btn ${wallpaper.type === "preset" && wallpaper.presetId === presetId ? "selected" : ""}`}
                    style={{ background: PRESET_WALLPAPERS[presetId] }}
                    onClick={() => setPresetWallpaper(presetId)}
                    title={getPresetWallpaperName(presetId)}
                  >
                    <span className="st-wallpaper-label">{getPresetWallpaperName(presetId)}</span>
                  </button>
                ))}
              </div>
              <div className="st-wallpaper-actions">
                <button className="st-btn st-btn-primary" onClick={importWallpaper}>
                  📁 导入本地壁纸
                </button>
                {wallpaper.type === "custom" && (
                  <button className="st-btn" onClick={clearWallpaper}>
                    重置为默认
                  </button>
                )}
              </div>
              {wallpaper.type === "custom" && wallpaper.customPath && (
                <div className="st-wallpaper-preview">
                  <div 
                    className="st-wallpaper-preview-img"
                    style={getWallpaperStyle(wallpaper)}
                  />
                  <span className="st-wallpaper-preview-label">当前自定义壁纸</span>
                </div>
              )}
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