import { useState, useEffect } from "react";
import { useServerManager, formatLastConnected, getStatusColor } from "../context/ServerManager";
import { useWallpaper, getPresetWallpaperName, getWallpaperStyle } from "../context/WallpaperContext";
import { PRESET_WALLPAPERS } from "../stores/wallpaperStore";
import { useSettingsStore } from "../stores/settingsStore";
import { themes, accentColors } from "../config/themes";
import { ThemeId } from "../config/themes";
import { createLogger } from '../utils/logger';
import { AuthMethod } from '../types/server';
// import { useWindowState } from "../window-system/hooks/useWindowState"; // 未来集成时使用
import "./Settings.css";

const log = createLogger('Settings');

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
  onAdd: (name: string, host: string, port: number, auth: any) => void;
}

function AddServerModal({ isOpen, onClose, onAdd }: AddServerModalProps) {
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState(8443);

  // 认证配置状态
  const [authMethod, setAuthMethod] = useState<AuthMethod>(AuthMethod.PASSWORD);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [privateKeyFile, setPrivateKeyFile] = useState("");
  const [passphrase, setPassphrase] = useState("");

  if (!isOpen) return null;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (name && host) {
      // 构建认证配置对象
      const auth = {
        method: authMethod,
        username,
        ...(authMethod === AuthMethod.PASSWORD && { password }),
        ...(authMethod === AuthMethod.PUBKEY && {
          privateKey: privateKeyFile,
          ...(passphrase && { passphrase }), // 只在密码非空时才添加
        }),
      };

      onAdd(name, host, port, auth);
      setName("");
      setHost("");
      setPort(8443);
      // 重置认证配置
      setAuthMethod(AuthMethod.PASSWORD);
      setUsername("");
      setPassword("");
      setPrivateKeyFile("");
      setPassphrase("");
      onClose();
    }
  };

  // 处理私钥文件选择（AddServerModal）
  const handlePrivateKeySelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (file) {
      try {
        // 读取文件内容
        const content = await file.text();

        // 基本格式验证
        if (!content.includes('-----BEGIN')) {
          alert('选择的文件不是有效的私钥文件。\n\n私钥文件应以 -----BEGIN 开头。\n\n支持的格式：\n• OpenSSH 格式（如 ~/.ssh/id_ed25519）\n• PEM 格式（云服务商提供的密钥）\n\n不支持：PuTTY 格式（.ppk）');
          return;
        }

        // 检查是否是 PuTTY 格式
        if (content.includes('PuTTY')) {
          alert('检测到 PuTTY 格式私钥（.ppk）。\n\n请使用 PuTTYgen 转换为 OpenSSH 格式：\n1. 打开 PuTTYgen\n2. 加载您的 .ppk 文件\n3. 点击 "Conversions" -> "Export OpenSSH key"\n4. 保存新的文件');
          return;
        }

        // 检查是否加密
        if (content.includes('ENCRYPTED') && !passphrase) {
          console.log('检测到加密私钥，用户需要在密码字段输入密码');
        }

        setPrivateKeyFile(content);
        log.info('已加载私钥文件:', file.name);
      } catch (error) {
        console.error("读取私钥文件失败:", error);
        alert("读取私钥文件失败，请检查文件格式和权限");
      }
    }
  };

  return (
    <div className="st-modal-overlay">
      <div className="st-modal">
        <div className="st-modal-header">
          <span className="st-modal-title">添加服务器</span>
          <button className="st-modal-close" onClick={onClose}>×</button>
        </div>
        <form className="st-modal-body" onSubmit={handleSubmit}>
          <div className="st-form-row">
            <label className="st-form-label text-label">服务器名称</label>
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
            <label className="st-form-label text-label">主机地址</label>
            <input
              type="text"
              className="st-form-input"
              value={host}
              onChange={(e) => setHost(e.target.value)}
              placeholder="例如: server.example.com 或 172.20.10.3"
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

          {/* 认证配置 */}
          <div className="st-form-row">
            <label className="st-form-label">认证方式</label>
            <select
              className="st-form-input"
              value={authMethod}
              onChange={(e) => setAuthMethod(e.target.value as AuthMethod)}
            >
              <option value={AuthMethod.PASSWORD}>密码认证</option>
              <option value={AuthMethod.PUBKEY}>公钥认证</option>
            </select>
          </div>

          <div className="st-form-row">
            <label className="st-form-label">用户名</label>
            <input
              type="text"
              className="st-form-input"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="SSH 登录用户名"
            />
          </div>

          {/* 密码认证字段 */}
          {authMethod === AuthMethod.PASSWORD && (
            <div className="st-form-row">
              <label className="st-form-label">密码</label>
              <input
                type="password"
                className="st-form-input"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="SSH 登录密码"
              />
            </div>
          )}

          {/* 公钥认证字段 */}
          {authMethod === AuthMethod.PUBKEY && (
            <>
              <div className="st-form-row">
                <label className="st-form-label">私钥文件</label>
                <input
                  type="file"
                  className="st-form-input"
                  onChange={handlePrivateKeySelect}
                  accept=".pem,.key,id_rsa,id_ed25519,id_ecdsa,id_dsa"
                />
                <span className="st-form-hint">
                  选择 SSH 私钥文件（OpenSSH 或 PEM 格式）<br/>
                  支持：id_ed25519、id_rsa、云服务商提供的密钥<br/>
                  不支持：PuTTY 格式（.ppk）- 请先转换
                </span>
              </div>
              {privateKeyFile && (
                <div className="st-form-row">
                  <span className="st-form-hint">已选择: {privateKeyFile.split('\n')[0].substring(0, 50)}...</span>
                </div>
              )}
              <div className="st-form-row">
                <label className="st-form-label">私钥密码 (可选)</label>
                <input
                  type="password"
                  className="st-form-input"
                  value={passphrase}
                  onChange={(e) => setPassphrase(e.target.value)}
                  placeholder="如果私钥有密码保护,请输入"
                />
              </div>
            </>
          )}
        </form>
        <div className="st-modal-footer">
          <button className="st-btn" onClick={onClose}>取消</button>
          <button className="st-btn st-btn-primary" onClick={handleSubmit}>添加</button>
        </div>
      </div>
    </div>
  );
}

/* ── Edit Server Modal ───────────────────────────────── */

interface EditServerModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSave: (id: string, name: string, host: string, port: number, auth: any) => void;
  server: { id: string; name: string; host: string; port: number; auth?: any } | null;
}

function EditServerModal({ isOpen, onClose, onSave, server }: EditServerModalProps) {
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState(8443);

  // 认证配置状态
  const [authMethod, setAuthMethod] = useState<AuthMethod>(AuthMethod.PASSWORD);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [privateKeyFile, setPrivateKeyFile] = useState("");
  const [passphrase, setPassphrase] = useState("");

  // 当 server 变化时，更新表单数据
  useEffect(() => {
    if (server) {
      setName(server.name);
      setHost(server.host);
      setPort(server.port);
      // 加载认证配置
      if (server.auth) {
        setAuthMethod(server.auth.method || AuthMethod.PASSWORD);
        setUsername(server.auth.username || "");
        setPassword(server.auth.password || "");
        setPrivateKeyFile(server.auth.privateKey || "");
        setPassphrase(server.auth.passphrase || "");
      }
    }
  }, [server]);

  if (!isOpen || !server) return null;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (name && host) {
      // 构建认证配置对象
      const auth = {
        method: authMethod,
        username,
        ...(authMethod === AuthMethod.PASSWORD && { password }),
        ...(authMethod === AuthMethod.PUBKEY && {
          privateKey: privateKeyFile,
          ...(passphrase && { passphrase }), // 只在密码非空时才添加
        }),
      };

      onSave(server.id, name, host, port, auth);
      onClose();
    }
  };

  // 处理私钥文件选择（EditServerModal）
  const handlePrivateKeySelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (file) {
      try {
        // 读取文件内容
        const content = await file.text();

        // 基本格式验证
        if (!content.includes('-----BEGIN')) {
          alert('选择的文件不是有效的私钥文件。\n\n私钥文件应以 -----BEGIN 开头。\n\n支持的格式：\n• OpenSSH 格式（如 ~/.ssh/id_ed25519）\n• PEM 格式（云服务商提供的密钥）\n\n不支持：PuTTY 格式（.ppk）');
          return;
        }

        // 检查是否是 PuTTY 格式
        if (content.includes('PuTTY')) {
          alert('检测到 PuTTY 格式私钥（.ppk）。\n\n请使用 PuTTYgen 转换为 OpenSSH 格式：\n1. 打开 PuTTYgen\n2. 加载您的 .ppk 文件\n3. 点击 "Conversions" -> "Export OpenSSH key"\n4. 保存新的文件');
          return;
        }

        // 检查是否加密
        if (content.includes('ENCRYPTED') && !passphrase) {
          console.log('检测到加密私钥，用户需要在密码字段输入密码');
        }

        setPrivateKeyFile(content);
        log.info('已加载私钥文件:', file.name);
      } catch (error) {
        console.error("读取私钥文件失败:", error);
        alert("读取私钥文件失败，请检查文件格式和权限");
      }
    }
  };

  return (
    <div className="st-modal-overlay">
      <div className="st-modal">
        <div className="st-modal-header">
          <span className="st-modal-title">编辑服务器</span>
          <button className="st-modal-close" onClick={onClose}>×</button>
        </div>
        <form className="st-modal-body" onSubmit={handleSubmit}>
          <div className="st-form-row">
            <label className="st-form-label text-label">服务器名称</label>
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
            <label className="st-form-label text-label">主机地址</label>
            <input
              type="text"
              className="st-form-input"
              value={host}
              onChange={(e) => setHost(e.target.value)}
              placeholder="例如: server.example.com 或 172.20.10.3"
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

          {/* 认证配置 */}
          <div className="st-form-row">
            <label className="st-form-label">认证方式</label>
            <select
              className="st-form-input"
              value={authMethod}
              onChange={(e) => setAuthMethod(e.target.value as AuthMethod)}
            >
              <option value={AuthMethod.PASSWORD}>密码认证</option>
              <option value={AuthMethod.PUBKEY}>公钥认证</option>
            </select>
          </div>

          <div className="st-form-row">
            <label className="st-form-label">用户名</label>
            <input
              type="text"
              className="st-form-input"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="SSH 登录用户名"
            />
          </div>

          {/* 密码认证字段 */}
          {authMethod === AuthMethod.PASSWORD && (
            <div className="st-form-row">
              <label className="st-form-label">密码</label>
              <input
                type="password"
                className="st-form-input"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="SSH 登录密码"
              />
            </div>
          )}

          {/* 公钥认证字段 */}
          {authMethod === AuthMethod.PUBKEY && (
            <>
              <div className="st-form-row">
                <label className="st-form-label">私钥文件</label>
                <input
                  type="file"
                  className="st-form-input"
                  onChange={handlePrivateKeySelect}
                  accept=".pem,.key,id_rsa,id_ed25519,id_ecdsa,id_dsa"
                />
                <span className="st-form-hint">
                  选择 SSH 私钥文件（OpenSSH 或 PEM 格式）<br/>
                  支持：id_ed25519、id_rsa、云服务商提供的密钥<br/>
                  不支持：PuTTY 格式（.ppk）- 请先转换
                </span>
              </div>
              {privateKeyFile && (
                <div className="st-form-row">
                  <span className="st-form-hint">已选择: {privateKeyFile.split('\n')[0].substring(0, 50)}...</span>
                </div>
              )}
              <div className="st-form-row">
                <label className="st-form-label">私钥密码 (可选)</label>
                <input
                  type="password"
                  className="st-form-input"
                  value={passphrase}
                  onChange={(e) => setPassphrase(e.target.value)}
                  placeholder="如果私钥有密码保护,请输入"
                />
              </div>
            </>
          )}
        </form>
        <div className="st-modal-footer">
          <button className="st-btn" onClick={onClose}>取消</button>
          <button className="st-btn st-btn-primary" onClick={handleSubmit}>保存</button>
        </div>
      </div>
    </div>
  );
}

/* ── Main Component ─────────────────────────────────── */

export function Settings({ windowId: _windowId }: { windowId: string }) {
  // 窗口系统集成（未来可能需要使用 windowState）
  // const windowState = useWindowState(windowId);
  const [activeSection, setActiveSection] = useState<SettingsSection>("connection");
  const [addModalOpen, setAddModalOpen] = useState(false);
  const [editModalOpen, setEditModalOpen] = useState(false);
  const [editingServer, setEditingServer] = useState<{ id: string; name: string; host: string; port: number; auth?: any } | null>(null);
  const [selectedServerId, setSelectedServerId] = useState<string | null>(null);

  // 终端默认路径配置
  const [terminalDefaultPath, setTerminalDefaultPath] = useState(() => {
    return localStorage.getItem("terminal-default-path") || "";
  });

  const handleTerminalPathChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value;
    setTerminalDefaultPath(value);
    localStorage.setItem("terminal-default-path", value);
  };

  const {
    servers,
    activeServer,
    activeServerId,
    addServer,
    updateServer,
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

  // 使用 Zustand settingsStore
  const {
    fontSize,
    setFontSize,
    terminalFontSize,
    setTerminalFontSize,
    themeId,
    setThemeId,
    accentColorId,
    setAccentColorId,
  } = useSettingsStore();

  // 主题已由 Desktop.tsx 全局应用，此处不再重复调用
  // useTheme(themeId, accentColorId);

  const handleAddServer = (name: string, host: string, port: number, auth: any) => {
    addServer({
      name,
      host,
      port,
      auth,
    });
  };

  const handleEditServer = (id: string, name: string, host: string, port: number, auth: any) => {
    updateServer(id, { name, host, port, auth });
  };

  const handleOpenEditModal = (server: { id: string; name: string; host: string; port: number; auth?: any }) => {
    setEditingServer(server);
    setEditModalOpen(true);
  };

  const handleConnect = async (id: string) => {
    log.debug("handleConnect 被调用, id:", id);
    setSelectedServerId(id);
    log.debug("调用 connectServer");
    await connectServer(id);
    log.debug("connectServer 完成");
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
            <div className="st-section-title text-heading">连接服务器配置</div>

            {/* Current Connection */}
            <div className="st-card">
              <div className="st-card-header text-title">当前连接</div>
              <div className="st-current-connection">
                <div className="st-conn-row">
                  <span className="st-conn-label text-label">主机</span>
                  <span className="st-conn-value st-conn-value-host">
                    {activeServer?.host || <span className="st-conn-placeholder">未连接</span>}
                  </span>
                </div>
                <div className="st-conn-row">
                  <span className="st-conn-label text-label">端口</span>
                  <span className="st-conn-value st-conn-value-port">
                    {activeServer?.port || <span className="st-conn-placeholder">—</span>}
                    <span className="st-conn-protocol">(QUIC)</span>
                  </span>
                </div>
                <div className="st-conn-row">
                  <span className="st-conn-label text-label">状态</span>
                  <span className="st-conn-value st-conn-value-status">
                    <span 
                      className="st-status-dot" 
                      style={{ background: activeServer ? getStatusColor(activeServer.status) : "#9a9996" }}
                    />
                    {activeServer?.status === "connected" ? "已连接" : 
                     activeServer?.status === "connecting" ? "连接中..." :
                     activeServer?.status === "error" ? "错误" : 
                     <span className="st-conn-placeholder">未连接</span>}
                  </span>
                </div>
                {activeServer?.status === "connected" && (
                  <div className="st-conn-row">
                    <span className="st-conn-label text-label">延迟</span>
                    <span className="st-conn-value st-conn-value-latency">6 ms</span>
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
              <div className="st-card-header text-title">已保存的服务器</div>
              <div className="st-server-list">
                {servers.length === 0 ? (
                  <div className="st-server-empty">
                    <span className="text-body">暂无保存的服务器</span>
                    <span className="st-server-empty-hint text-caption">点击下方按钮添加</span>
                  </div>
                ) : (
                  servers.map(server => (
                    <div
                      key={server.id}
                      className={`st-server-item ${activeServerId === server.id ? "selected" : ""}`}
                      onClick={() => handleOpenEditModal(server)}
                    >
                      <span
                        className="st-server-status"
                        style={{ background: getStatusColor(server.status) }}
                      />
                      <span className="st-server-name">{server.name}</span>
                      <span className="st-server-host">{server.host}</span>
                      <span className="st-server-time">{formatLastConnected(server.lastConnected)}</span>
                      <div className="st-server-actions">
                        {server.status === "connected" ? (
                          <button 
                            className="st-server-action-btn st-server-action-connected"
                            onClick={(e) => { e.stopPropagation(); handleDisconnect(server.id); }}
                            title="已连接 - 点击断开"
                          >
                            ✓
                          </button>
                        ) : server.status === "connecting" ? (
                          <button 
                            className="st-server-action-btn st-server-action-connecting"
                            disabled
                            title="连接中..."
                          >
                            ◷
                          </button>
                        ) : (
                          <button 
                            className="st-server-action-btn"
                            onClick={(e) => { e.stopPropagation(); handleConnect(server.id); }}
                            title="连接"
                          >
                            🔗
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

            <EditServerModal
              isOpen={editModalOpen}
              onClose={() => setEditModalOpen(false)}
              onSave={handleEditServer}
              server={editingServer}
            />
          </div>
        );

      case "appearance":
        const currentTheme = themes[themeId];
        return (
          <div className="st-section">
            <div className="st-section-title text-heading">外观设置</div>

            {/* 主题选择 */}
            <div className="st-card">
              <div className="st-card-header text-title">主题</div>
              <div className="st-theme-options">
                {Object.entries(themes).map(([id, theme]) => (
                  <div
                    key={id}
                    className={`st-theme-option ${themeId === id ? "selected" : ""}`}
                    onClick={() => {
                      setThemeId(id as ThemeId);
                      // 切换到纸张主题时，默认选中绿色强调色
                      if (id === 'paper') {
                        setAccentColorId('paperAccent');
                      } else {
                        // 切换到其他主题时，清除强调色选择
                        setAccentColorId(null);
                      }
                    }}
                    title={theme.name}
                  >
                    <div className="st-theme-preview" style={{ background: theme.lightColors.viewBg }}>
                      <div className="st-theme-preview-header" style={{ background: theme.lightColors.headerbarBg }} />
                      <div className="st-theme-preview-sidebar" style={{ background: theme.lightColors.sidebarBg }} />
                      <div className="st-theme-preview-card" style={{ background: theme.lightColors.cardBg }} />
                    </div>
                  </div>
                ))}
              </div>
            </div>

            {/* 强调色选择（仅当主题支持可选强调色时显示） */}
            {currentTheme.accentColorOptions && (
              <div className="st-card">
                <div className="st-card-header text-title">强调色</div>
                <div className="st-accent-options">
                  {currentTheme.accentColorOptions.map((option) => (
                    <div
                      key={option}
                      className={`st-accent-option ${accentColorId === option ? "selected" : ""}`}
                      onClick={() => setAccentColorId(option)}
                    >
                      <div
                        className="st-accent-preview"
                        style={{ background: accentColors[option].light }}
                      />
                      <div className="st-accent-name">
                        {option === "warmBlue" ? "暖蓝色" : "绿色"}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* 字体大小 */}
            <div className="st-card">
              <div className="st-card-header text-title">字体大小</div>
              <div className="st-slider-row">
                <span className="st-slider-label text-body">界面字体</span>
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
                <span className="st-slider-label text-body">终端字体</span>
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

            {/* 壁纸 */}
            <div className="st-card">
              <div className="st-card-header text-title">壁纸</div>
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
              <div className="st-card-header text-title">全局快捷键</div>
              <div className="st-shortcut-list">
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action text-body">打开活动概览</span>
                  <span className="st-shortcut-key">Super</span>
                </div>
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action">打开文件管理器</span>
                  <span className="st-shortcut-key text-mono">Super + 1</span>
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
                  <span className="st-shortcut-action text-body">打开设置</span>
                  <span className="st-shortcut-key">Super + 4</span>
                </div>
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action text-body">切换终端标签</span>
                  <span className="st-shortcut-key">Ctrl + Tab</span>
                </div>
              </div>
            </div>
          </div>
        );

      case "files":
        return (
          <div className="st-section">
            <div className="st-section-title text-heading">文件管理设置</div>
            <div className="st-card">
              <div className="st-card-header text-title">默认视图</div>
              <div className="st-option-row">
                <span className="st-option-label text-body">默认视图模式</span>
                <select className="st-select">
                  <option>列表视图</option>
                  <option>网格视图</option>
                </select>
              </div>
              <div className="st-option-row">
                <span className="st-option-label text-body">显示隐藏文件</span>
                <input type="checkbox" className="st-checkbox" />
              </div>
              <div className="st-option-row">
                <span className="st-option-label text-body">排序方式</span>
                <select className="st-select">
                  <option>名称</option>
                  <option>大小</option>
                  <option>修改时间</option>
                </select>
              </div>
            </div>
            <div className="st-card">
              <div className="st-card-header text-title">传输设置</div>
              <div className="st-option-row">
                <span className="st-option-label text-body">最大传输大小</span>
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

            {/* 默认工作目录 */}
            <div className="st-card">
              <div className="st-card-header text-title">默认工作目录</div>
              <div className="st-option-row">
                <input
                  type="text"
                  className="st-input"
                  value={terminalDefaultPath}
                  onChange={handleTerminalPathChange}
                  placeholder="留空使用用户主目录 (~)"
                  spellCheck={false}
                  style={{ width: '100%' }}
                />
              </div>
              <div className="st-hint">
                💡 在文件管理器地址栏输入 <code>shell</code> 可在当前目录打开终端
              </div>
            </div>

            <div className="st-card">
              <div className="st-card-header text-title">配色方案</div>
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
              <div className="st-card-header text-title">终端选项</div>
              <div className="st-option-row">
                <span className="st-option-label text-body">光标样式</span>
                <select className="st-select">
                  <option>方块</option>
                  <option>竖线</option>
                  <option>下划线</option>
                </select>
              </div>
              <div className="st-option-row">
                <span className="st-option-label text-body">光标闪烁</span>
                <input type="checkbox" className="st-checkbox" defaultChecked />
              </div>
              <div className="st-option-row">
                <span className="st-option-label text-body">滚动缓冲区</span>
                <input type="number" className="st-input" defaultValue="5000" />
                <span className="st-input-unit text-caption">行</span>
              </div>
            </div>
          </div>
        );

      case "notifications":
        return (
          <div className="st-section">
            <div className="st-section-title text-heading">通知设置</div>
            <div className="st-card">
              <div className="st-card-header text-title">通知类型</div>
              <div className="st-option-row">
                <span className="st-option-label text-body">系统监控警报</span>
                <input type="checkbox" className="st-checkbox" defaultChecked />
              </div>
              <div className="st-option-row">
                <span className="st-option-label text-body">文件操作完成</span>
                <input type="checkbox" className="st-checkbox" defaultChecked />
              </div>
              <div className="st-option-row">
                <span className="st-option-label text-body">连接状态变化</span>
                <input type="checkbox" className="st-checkbox" defaultChecked />
              </div>
            </div>
            <div className="st-card">
              <div className="st-card-header text-title">警报阈值</div>
              <div className="st-slider-row">
                <span className="st-slider-label text-body">CPU 使用率警报</span>
                <input type="range" min="50" max="100" defaultValue="80" className="st-slider" />
                <span className="st-slider-value">80%</span>
              </div>
              <div className="st-slider-row">
                <span className="st-slider-label text-body">磁盘使用率警报</span>
                <input type="range" min="50" max="100" defaultValue="90" className="st-slider" />
                <span className="st-slider-value">90%</span>
              </div>
            </div>
          </div>
        );

      case "about":
        return (
          <div className="st-section">
            <div className="st-section-title text-heading">关于</div>
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
              <div className="st-card-header text-title">技术栈</div>
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
                  <span className="st-tech-name text-label">终端模拟</span>
                  <span className="st-tech-value text-caption">xterm.js 5.5</span>
                </div>
                <div className="st-tech-item">
                  <span className="st-tech-name text-label">设计系统</span>
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
              <span className="st-sb-label text-label">{item.label}</span>
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