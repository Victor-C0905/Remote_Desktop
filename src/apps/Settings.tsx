import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useServerManager, formatLastConnected, getStatusColor } from "../context/ServerManager";
import { useWallpaper, getPresetWallpaperName, getWallpaperStyle } from "../context/WallpaperContext";
import { PRESET_WALLPAPERS } from "../stores/wallpaperStore";
import { useSettingsStore } from "../stores/settingsStore";
import { themes, accentColors } from "../config/themes";
import { ThemeId } from "../config/themes";
import { createLogger } from '../utils/logger';
import { AuthMethod } from '../types/server';
import { StatsPanel } from '../components/StatsPanel';
import { TransferLimitCard } from "../components/TransferLimitCard";
import { SymbolicIcon } from '../components/symbolic';
import { getServerErrorInfo } from '../stores/serversStore';
import { getSettingsStorage } from '../utils/storage';
import { RESOURCES, type Locale } from "../i18n";
import { useTranslation } from "react-i18next";
// import { useWindowState } from "../window-system/hooks/useWindowState";
import "./Settings.css";

const log = createLogger('Settings');

/* ── Types ─────────────────────────────────────────────── */

type SettingsSection = "connection" | "appearance" | "keyboard" | "files" | "terminal" | "browser" | "notifications" | "stats" | "about";

/** 本机可用浏览器（Rust 端 proxy_list_browsers 返回） */
interface BrowserInfo {
  id: string;
  name: string;
  path: string;
}

/* ── Sidebar Items ──────────────────────────────────── */

interface SidebarItem {
  id: SettingsSection;
  icon: string;
  labelKey: string;
}

const SIDEBAR_ITEMS: SidebarItem[] = [
  { id: "connection", icon: "🔗", labelKey: "settings.section.connection" },
  { id: "appearance", icon: "🎨", labelKey: "settings.section.appearance" },
  { id: "keyboard", icon: "⌨️", labelKey: "settings.section.keyboard" },
  { id: "files", icon: "📁", labelKey: "settings.section.files" },
  { id: "terminal", icon: "🖥️", labelKey: "settings.section.terminal" },
  { id: "browser", icon: "🌐", labelKey: "settings.section.browser" },
  { id: "notifications", icon: "🔔", labelKey: "settings.section.notifications" },
  { id: "stats", icon: "📊", labelKey: "settings.section.stats" },
  { id: "about", icon: "ℹ️", labelKey: "settings.section.about" },
];

/* ── Add Server Modal ───────────────────────────────── */

interface AddServerModalProps {
  isOpen: boolean;
  onClose: () => void;
  onAdd: (name: string, host: string, port: number, auth: any) => void;
}

function AddServerModal({ isOpen, onClose, onAdd }: AddServerModalProps) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState(8443);

  // 认证配置状态
  const [authMethod, setAuthMethod] = useState<AuthMethod>(AuthMethod.PASSWORD);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [privateKeyFile, setPrivateKeyFile] = useState("");
  const [passphrase, setPassphrase] = useState("");
  // 表单内联错误（替代系统 alert）
  const [formError, setFormError] = useState<string | null>(null);

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
      setFormError(null);
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
      setFormError(null);
      try {
        // 读取文件内容
        const content = await file.text();

        // 基本格式验证
        if (!content.includes('-----BEGIN')) {
          setFormError(t('settings.errors.invalidKeyFile'));
          return;
        }

        // 检查是否是 PuTTY 格式
        if (content.includes('PuTTY')) {
          setFormError(t('settings.errors.puttyKeyFile'));
          return;
        }

        // 检查是否加密
        if (content.includes('ENCRYPTED') && !passphrase) {
          console.log('检测到加密私钥，用户需要在密码字段输入密码');
        }

        setPrivateKeyFile(content);
        log.info('Private key file loaded:', file.name);
      } catch (error) {
        console.error("读取私钥文件失败:", error);
        setFormError(t('settings.errors.readKeyFailed'));
      }
    }
  };

  return (
    <div className="st-modal-overlay">
      <div className="st-modal">
        <div className="st-modal-header">
          <span className="st-modal-title">{t("settings.addServerTitle")}</span>
          <button className="st-modal-close" onClick={onClose}>×</button>
        </div>
        <form className="st-modal-body" onSubmit={handleSubmit}>
          <div className="st-form-row">
            <label className="st-form-label text-label">{t("settings.serverName")}</label>
            <input
              type="text"
              className="st-form-input"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("settings.serverNamePlaceholder")}
              autoFocus
            />
          </div>
          <div className="st-form-row">
            <label className="st-form-label text-label">{t("settings.host")}</label>
            <input
              type="text"
              className="st-form-input"
              value={host}
              onChange={(e) => setHost(e.target.value)}
              placeholder={t("settings.hostPlaceholder")}
            />
          </div>
          <div className="st-form-row">
            <label className="st-form-label">{t("settings.port")}</label>
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
            <label className="st-form-label">{t("settings.authMethod")}</label>
            <select
              className="st-form-input"
              value={authMethod}
              onChange={(e) => setAuthMethod(e.target.value as AuthMethod)}
            >
              <option value={AuthMethod.PASSWORD}>{t("settings.authPassword")}</option>
              <option value={AuthMethod.PUBKEY}>{t("settings.authPubkey")}</option>
            </select>
          </div>

          <div className="st-form-row">
            <label className="st-form-label">{t("settings.username")}</label>
            <input
              type="text"
              className="st-form-input"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder={t("settings.usernamePlaceholder")}
            />
          </div>

          {/* 密码认证字段 */}
          {authMethod === AuthMethod.PASSWORD && (
            <div className="st-form-row">
              <label className="st-form-label">{t("settings.password")}</label>
              <input
                type="password"
                className="st-form-input"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={t("settings.passwordPlaceholder")}
              />
            </div>
          )}

          {/* 公钥认证字段 */}
          {authMethod === AuthMethod.PUBKEY && (
            <>
              <div className="st-form-row">
                <label className="st-form-label">{t("settings.privateKeyFile")}</label>
                <input
                  type="file"
                  className="st-form-input"
                  onChange={handlePrivateKeySelect}
                  accept=".pem,.key,id_rsa,id_ed25519,id_ecdsa,id_dsa"
                />
                <span className="st-form-hint">
                  {t("settings.privateKeyHint1")}<br/>
                  {t("settings.privateKeyHint2")}<br/>
                  {t("settings.privateKeyHint3")}
                </span>
              </div>
              {privateKeyFile && (
                <div className="st-form-row">
                  <span className="st-form-hint">{t("settings.keyPreview", { key: privateKeyFile.split('\n')[0].substring(0, 50) })}</span>
                </div>
              )}
              <div className="st-form-row">
                <label className="st-form-label">{t("settings.passphrase")}</label>
                <input
                  type="password"
                  className="st-form-input"
                  value={passphrase}
                  onChange={(e) => setPassphrase(e.target.value)}
                  placeholder={t("settings.passphrasePlaceholder")}
                />
              </div>
            </>
          )}
          {formError && <div className="st-form-error">{formError}</div>}
        </form>
        <div className="st-modal-footer">
          <button className="st-btn" onClick={onClose}>{t("common.cancel")}</button>
          <button className="st-btn st-btn-primary" onClick={handleSubmit}>{t("settings.addBtn")}</button>
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
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState(8443);

  // 认证配置状态
  const [authMethod, setAuthMethod] = useState<AuthMethod>(AuthMethod.PASSWORD);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [privateKeyFile, setPrivateKeyFile] = useState("");
  const [passphrase, setPassphrase] = useState("");
  // 表单内联错误（替代系统 alert）
  const [formError, setFormError] = useState<string | null>(null);

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
      setFormError(null);
      try {
        // 读取文件内容
        const content = await file.text();

        // 基本格式验证
        if (!content.includes('-----BEGIN')) {
          setFormError(t('settings.errors.invalidKeyFile'));
          return;
        }

        // 检查是否是 PuTTY 格式
        if (content.includes('PuTTY')) {
          setFormError(t('settings.errors.puttyKeyFile'));
          return;
        }

        // 检查是否加密
        if (content.includes('ENCRYPTED') && !passphrase) {
          console.log('检测到加密私钥，用户需要在密码字段输入密码');
        }

        setPrivateKeyFile(content);
        log.info('Private key file loaded:', file.name);
      } catch (error) {
        console.error("读取私钥文件失败:", error);
        setFormError(t('settings.errors.readKeyFailed'));
      }
    }
  };

  return (
    <div className="st-modal-overlay">
      <div className="st-modal">
        <div className="st-modal-header">
          <span className="st-modal-title">{t("settings.editServerTitle")}</span>
          <button className="st-modal-close" onClick={onClose}>×</button>
        </div>
        <form className="st-modal-body" onSubmit={handleSubmit}>
          <div className="st-form-row">
            <label className="st-form-label text-label">{t("settings.serverName")}</label>
            <input
              type="text"
              className="st-form-input"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("settings.serverNamePlaceholder")}
              autoFocus
            />
          </div>
          <div className="st-form-row">
            <label className="st-form-label text-label">{t("settings.host")}</label>
            <input
              type="text"
              className="st-form-input"
              value={host}
              onChange={(e) => setHost(e.target.value)}
              placeholder={t("settings.hostPlaceholder")}
            />
          </div>
          <div className="st-form-row">
            <label className="st-form-label">{t("settings.port")}</label>
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
            <label className="st-form-label">{t("settings.authMethod")}</label>
            <select
              className="st-form-input"
              value={authMethod}
              onChange={(e) => setAuthMethod(e.target.value as AuthMethod)}
            >
              <option value={AuthMethod.PASSWORD}>{t("settings.authPassword")}</option>
              <option value={AuthMethod.PUBKEY}>{t("settings.authPubkey")}</option>
            </select>
          </div>

          <div className="st-form-row">
            <label className="st-form-label">{t("settings.username")}</label>
            <input
              type="text"
              className="st-form-input"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder={t("settings.usernamePlaceholder")}
            />
          </div>

          {/* 密码认证字段 */}
          {authMethod === AuthMethod.PASSWORD && (
            <div className="st-form-row">
              <label className="st-form-label">{t("settings.password")}</label>
              <input
                type="password"
                className="st-form-input"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={t("settings.passwordPlaceholder")}
              />
            </div>
          )}

          {/* 公钥认证字段 */}
          {authMethod === AuthMethod.PUBKEY && (
            <>
              <div className="st-form-row">
                <label className="st-form-label">{t("settings.privateKeyFile")}</label>
                <input
                  type="file"
                  className="st-form-input"
                  onChange={handlePrivateKeySelect}
                  accept=".pem,.key,id_rsa,id_ed25519,id_ecdsa,id_dsa"
                />
                <span className="st-form-hint">
                  {t("settings.privateKeyHint1")}<br/>
                  {t("settings.privateKeyHint2")}<br/>
                  {t("settings.privateKeyHint3")}
                </span>
              </div>
              {privateKeyFile && (
                <div className="st-form-row">
                  <span className="st-form-hint">{t("settings.keyPreview", { key: privateKeyFile.split('\n')[0].substring(0, 50) })}</span>
                </div>
              )}
              <div className="st-form-row">
                <label className="st-form-label">{t("settings.passphrase")}</label>
                <input
                  type="password"
                  className="st-form-input"
                  value={passphrase}
                  onChange={(e) => setPassphrase(e.target.value)}
                  placeholder={t("settings.passphrasePlaceholder")}
                />
              </div>
            </>
          )}
          {formError && <div className="st-form-error">{formError}</div>}
        </form>
        <div className="st-modal-footer">
          <button className="st-btn" onClick={onClose}>{t("common.cancel")}</button>
          <button className="st-btn st-btn-primary" onClick={handleSubmit}>{t("settings.saveBtn")}</button>
        </div>
      </div>
    </div>
  );
}

/* ── Main Component ─────────────────────────────────── */

export function Settings({ windowId: _windowId }: { windowId: string }) {
  const { t } = useTranslation();
  // 窗口系统集成（未来可能需要使用 windowState）
  // const windowState = useWindowState(windowId);
  const [activeSection, setActiveSection] = useState<SettingsSection>("connection");
  const [addModalOpen, setAddModalOpen] = useState(false);
  const [editModalOpen, setEditModalOpen] = useState(false);
  const [editingServer, setEditingServer] = useState<{ id: string; name: string; host: string; port: number; auth?: any } | null>(null);
  const [selectedServerId, setSelectedServerId] = useState<string | null>(null);

  // 终端默认路径配置（通过 Tauri Store 异步加载）
  const [terminalDefaultPath, setTerminalDefaultPath] = useState("");

  // 版本号：从 Rust 端读取（编译期单一来源 Cargo.toml），不再前端硬编码
  const [appVersion, setAppVersion] = useState("");

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const storage = await getSettingsStorage();
      const value = await storage.getItem("terminal-default-path");
      if (!cancelled) setTerminalDefaultPath(value || "");
    })();
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    let cancelled = false;
    invoke<string>("get_app_version")
      .then((v) => { if (!cancelled) setAppVersion(v); })
      .catch((e) => log.error("获取版本号失败:", e));
    return () => { cancelled = true; };
  }, []);

  // 本机已安装浏览器列表（浏览器设置分区展示）
  const [browserList, setBrowserList] = useState<BrowserInfo[]>([]);
  useEffect(() => {
    let cancelled = false;
    invoke<BrowserInfo[]>("proxy_list_browsers")
      .then((list) => { if (!cancelled) setBrowserList(list); })
      .catch((e) => log.error("获取浏览器列表失败:", e));
    return () => { cancelled = true; };
  }, []);

  // 默认浏览器（全局设置，远程浏览应用启动时使用）
  const browserIdPref = useSettingsStore((s) => s.browserId);
  // SOCKS5 固定端口（断线重连后浏览器无需重开）
  const browserProxyPort = useSettingsStore((s) => s.browserProxyPort);
  // 启动时恢复上次窗口（响应式读取，勾选即时生效）
  const restoreWindowsOnStartup = useSettingsStore((s) => s.restoreWindowsOnStartup);
  // 界面语言
  const locale = useSettingsStore((s) => s.locale);
  const setLocale = useSettingsStore((s) => s.setLocale);

  const handleTerminalPathChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value;
    setTerminalDefaultPath(value);
    const storage = await getSettingsStorage();
    await storage.setItem("terminal-default-path", value);
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

  // 断开后 activeServerId 已清空（activeServer 为 null），
  // 回退到最近选择的服务器，保证"当前连接"卡片仍显示上下文与重连入口
  const cardServer = activeServer || servers.find((s) => s.id === selectedServerId) || null;

  const handleRemove = (id: string) => {
    if (confirm(t("settings.confirmRemove"))) {
      removeServer(id);
    }
  };

  const renderSection = () => {
    switch (activeSection) {
      case "connection":
        return (
          <div className="st-section">
            <div className="st-section-title text-heading">{t("settings.connTitle")}</div>

            {/* Current Connection */}
            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.currentConnection")}</div>
              <div className="st-current-connection">
                <div className="st-conn-row">
                  <span className="st-conn-label text-label">{t("settings.connHost")}</span>
                  <span className="st-conn-value st-conn-value-host">
                    {cardServer?.host || <span className="st-conn-placeholder">{t("settings.notConnected")}</span>}
                  </span>
                </div>
                <div className="st-conn-row">
                  <span className="st-conn-label text-label">{t("settings.port")}</span>
                  <span className="st-conn-value st-conn-value-port">
                    {cardServer?.port || <span className="st-conn-placeholder">—</span>}
                    <span className="st-conn-protocol">(QUIC)</span>
                  </span>
                </div>
                <div className="st-conn-row">
                  <span className="st-conn-label text-label">{t("settings.connStatus")}</span>
                  <span className="st-conn-value st-conn-value-status">
                    <span
                      className="st-status-dot"
                      style={{ background: cardServer ? getStatusColor(cardServer.status) : "#9a9996" }}
                    />
                    {cardServer?.status === "connected" ? t("settings.statusConnected") :
                     cardServer?.status === "connecting" ? t("settings.statusConnecting") :
                     cardServer?.status === "reconnecting" ? t("settings.statusReconnecting") :
                     cardServer?.status === "error" ? t("settings.statusError") :
                     <span className="st-conn-placeholder">{t("settings.notConnected")}</span>}
                  </span>
                </div>
                {cardServer?.status === "connected" && (
                  <div className="st-conn-row">
                    <span className="st-conn-label text-label">{t("settings.connLatency")}</span>
                    <span className="st-conn-value st-conn-value-latency">
                      {/* 心跳 RTT 回传（每 5s 刷新）；连接瞬间显示握手 RTT，首个 5s 内由心跳接管 */}
                      {cardServer.rttMs != null ? `${Math.round(cardServer.rttMs)} ms` : "—"}
                    </span>
                  </div>
                )}
                {(() => {
                  if (!cardServer?.error) return null;
                  const ei = getServerErrorInfo(cardServer);
                  return (
                    <div className="st-conn-error">
                      <SymbolicIcon name="dialog-error" size={14} className="st-conn-error-icon" />
                      <div className="st-conn-error-body">
                        <div className="st-conn-error-title">{ei.title}</div>
                        <div className="st-conn-error-message">
                          {ei.message}{ei.detail ? `（${ei.detail}）` : ""}
                        </div>
                        {ei.hint && <div className="st-conn-error-hint">{ei.hint}</div>}
                      </div>
                    </div>
                  );
                })()}
              </div>
              <div className="st-card-actions">
                {cardServer?.status === "connected" && (
                  <button 
                    className="st-btn st-btn-danger"
                    onClick={() => handleDisconnect(cardServer.id)}
                  >
                    {t("settings.disconnectBtn")}
                  </button>
                )}
                {cardServer && (cardServer.status === "error" || cardServer.status === "disconnected") && (
                  <button 
                    className="st-btn st-btn-primary"
                    onClick={() => handleConnect(cardServer.id)}
                  >
                    {t("settings.retryConnectBtn")}
                  </button>
                )}
              </div>
            </div>

            {/* Saved Servers */}
            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.savedServers")}</div>
              <div className="st-server-list">
                {servers.length === 0 ? (
                  <div className="st-server-empty">
                    <span className="text-body">{t("settings.noServers")}</span>
                    <span className="st-server-empty-hint text-caption">{t("settings.noServersHint")}</span>
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
                            title={t("settings.connectedTip")}
                          >
                            ✓
                          </button>
                        ) : server.status === "connecting" ? (
                          <button 
                            className="st-server-action-btn st-server-action-connecting"
                            disabled
                            title={t("settings.statusConnecting")}
                          >
                            ◷
                          </button>
                        ) : server.status === "reconnecting" ? (
                          <button 
                            className="st-server-action-btn st-server-action-connecting"
                            disabled
                            title={t("settings.reconnectingTip")}
                          >
                            ◷
                          </button>
                        ) : (
                          <button 
                            className="st-server-action-btn"
                            onClick={(e) => { e.stopPropagation(); handleConnect(server.id); }}
                            title={t("settings.connectTip")}
                          >
                            🔗
                          </button>
                        )}
                        <button 
                          className="st-server-action-btn st-server-action-danger"
                          onClick={(e) => { e.stopPropagation(); handleRemove(server.id); }}
                          title={t("settings.deleteTip")}
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
                  {t("settings.addServerBtn")}
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
            <div className="st-section-title text-heading">{t("settings.appearanceTitle")}</div>

            {/* 语言（label 双语：语言切换前无法依赖词典自身） */}
            <div className="st-card">
              <div className="st-card-header text-title">语言 / Language</div>
              <div className="st-form-row">
                <select
                  className="st-form-input"
                  value={locale}
                  onChange={(e) => setLocale(e.target.value as Locale)}
                >
                  {Object.entries(RESOURCES).map(([code, { label }]) => (
                    <option key={code} value={code}>{label}</option>
                  ))}
                </select>
              </div>
            </div>

            {/* 主题选择 */}
            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.themeTitle")}</div>
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
                    title={t(theme.nameKey)}
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
                <div className="st-card-header text-title">{t("settings.accentColorTitle")}</div>
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
                        {option === "warmBlue" ? t("settings.accentWarmBlue") : t("settings.accentGreen")}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* 字体大小 */}
            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.fontSizeTitle")}</div>
              <div className="st-slider-row">
                <span className="st-slider-label text-body">{t("settings.uiFont")}</span>
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
                <span className="st-slider-label text-body">{t("settings.terminalFont")}</span>
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
              <div className="st-card-header text-title">{t("settings.wallpaperTitle")}</div>
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
                  {t("settings.importWallpaperBtn")}
                </button>
                {wallpaper.type === "custom" && (
                  <button className="st-btn" onClick={clearWallpaper}>
                    {t("settings.resetWallpaperBtn")}
                  </button>
                )}
              </div>
              {wallpaper.type === "custom" && wallpaper.customPath && (
                <div className="st-wallpaper-preview">
                  <div
                    className="st-wallpaper-preview-img"
                    style={getWallpaperStyle(wallpaper)}
                  />
                  <span className="st-wallpaper-preview-label">{t("settings.customWallpaperLabel")}</span>
                </div>
              )}
            </div>
            {/* 启动行为：桌面会话恢复开关（关闭后每次启动为干净桌面） */}
            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.startupTitle")}</div>
              <div className="st-option-row">
                <span className="st-option-label text-body">{t("settings.restoreWindows")}</span>
                <input
                  type="checkbox"
                  className="st-checkbox"
                  checked={restoreWindowsOnStartup}
                  onChange={(e) =>
                    useSettingsStore.getState().setRestoreWindowsOnStartup(e.target.checked)
                  }
                />
              </div>
            </div>
          </div>
        );

      case "keyboard":
        return (
          <div className="st-section">
            <div className="st-section-title">{t("settings.section.keyboard")}</div>
            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.globalShortcutsTitle")}</div>
              <div className="st-shortcut-list">
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action text-body">{t("settings.shortcutOverview")}</span>
                  <span className="st-shortcut-key">Super</span>
                </div>
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action">{t("settings.shortcutFiles")}</span>
                  <span className="st-shortcut-key text-mono">Super + 1</span>
                </div>
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action">{t("settings.shortcutTerminal")}</span>
                  <span className="st-shortcut-key">Super + 2</span>
                </div>
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action">{t("settings.shortcutMonitor")}</span>
                  <span className="st-shortcut-key">Super + 3</span>
                </div>
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action text-body">{t("settings.shortcutSettings")}</span>
                  <span className="st-shortcut-key">Super + 4</span>
                </div>
                <div className="st-shortcut-row">
                  <span className="st-shortcut-action text-body">{t("settings.shortcutSwitchTerminalTab")}</span>
                  <span className="st-shortcut-key">Ctrl + Tab</span>
                </div>
              </div>
            </div>
          </div>
        );

      case "files":
        return (
          <div className="st-section">
            <div className="st-section-title text-heading">{t("settings.filesTitle")}</div>
            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.defaultViewTitle")}</div>
              <div className="st-option-row">
                <span className="st-option-label text-body">{t("settings.viewMode")}</span>
                <select className="st-select">
                  <option>{t("settings.viewList")}</option>
                  <option>{t("settings.viewGrid")}</option>
                </select>
              </div>
              <div className="st-option-row">
                <span className="st-option-label text-body">{t("settings.showHiddenFiles")}</span>
                <input type="checkbox" className="st-checkbox" />
              </div>
              <div className="st-option-row">
                <span className="st-option-label text-body">{t("settings.sortBy")}</span>
                <select className="st-select">
                  <option>{t("settings.sortName")}</option>
                  <option>{t("settings.sortSize")}</option>
                  <option>{t("settings.sortModified")}</option>
                </select>
              </div>
            </div>
            {/* 上传大小限制：值的单一事实来源在 Agent 端，组件内完成能力门控+查询+应用 */}
            <TransferLimitCard serverId={activeServer?.id || null} />
          </div>
        );

      case "terminal":
        return (
          <div className="st-section">
            <div className="st-section-title">{t("settings.terminalTitle")}</div>

            {/* 默认工作目录 */}
            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.workingDirTitle")}</div>
              <div className="st-option-row">
                <input
                  type="text"
                  className="st-input"
                  value={terminalDefaultPath}
                  onChange={handleTerminalPathChange}
                  placeholder={t("settings.workingDirPlaceholder")}
                  spellCheck={false}
                  style={{ width: '100%' }}
                />
              </div>
              <div className="st-hint">
                {t("settings.terminalHintPre")} <code>shell</code> {t("settings.terminalHintPost")}
              </div>
            </div>

            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.colorSchemeTitle")}</div>
              <div className="st-theme-toggle">
                <button className="st-theme-btn active">
                  <span className="st-theme-icon">⬛</span>
                  <span className="st-theme-label">{t("settings.schemeDark")}</span>
                </button>
                <button className="st-theme-btn">
                  <span className="st-theme-icon">⬜</span>
                  <span className="st-theme-label">{t("settings.schemeLight")}</span>
                </button>
                <button className="st-theme-btn">
                  <span className="st-theme-icon">🔲</span>
                  <span className="st-theme-label">{t("settings.schemeWhite")}</span>
                </button>
              </div>
            </div>
            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.terminalOptionsTitle")}</div>
              <div className="st-option-row">
                <span className="st-option-label text-body">{t("settings.cursorStyle")}</span>
                <select className="st-select">
                  <option>{t("settings.cursorBlock")}</option>
                  <option>{t("settings.cursorBar")}</option>
                  <option>{t("settings.cursorUnderline")}</option>
                </select>
              </div>
              <div className="st-option-row">
                <span className="st-option-label text-body">{t("settings.cursorBlink")}</span>
                <input type="checkbox" className="st-checkbox" defaultChecked />
              </div>
              <div className="st-option-row">
                <span className="st-option-label text-body">{t("settings.scrollBuffer")}</span>
                <input type="number" className="st-input" defaultValue="5000" />
                <span className="st-input-unit text-caption">{t("settings.linesUnit")}</span>
              </div>
            </div>
          </div>
        );

      case "browser": {
        const setBrowserId = useSettingsStore.getState().setBrowserId;
        return (
          <div className="st-section">
            <div className="st-section-title text-heading">{t("settings.browserTitle")}</div>

            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.defaultBrowserTitle")}</div>
              <div className="st-option-row">
                <span className="st-option-label text-body">{t("settings.remoteBrowserUse")}</span>
                <select
                  className="st-select"
                  value={browserList.some((b) => b.id === browserIdPref) || browserIdPref === "auto" ? browserIdPref : "auto"}
                  onChange={(e) => setBrowserId(e.target.value)}
                >
                  <option value="auto">{t("settings.browserAuto")}</option>
                  {browserList.map((b) => (
                    <option key={b.id} value={b.id}>{b.name}</option>
                  ))}
                </select>
              </div>
              {browserList.length === 0 && (
                <div className="st-hint">{t("settings.noBrowsersHint")}</div>
              )}
              <div className="st-option-row">
                <span className="st-option-label text-body">{t("settings.socks5Port")}</span>
                <input
                  type="number"
                  className="st-input"
                  min={1024}
                  max={65535}
                  value={browserProxyPort}
                  onChange={(e) => {
                    const v = parseInt(e.target.value, 10);
                    if (!Number.isNaN(v) && v >= 1024 && v <= 65535) {
                      useSettingsStore.getState().setBrowserProxyPort(v);
                    }
                  }}
                  style={{ width: 100 }}
                />
              </div>
              <div className="st-hint">
                {t("settings.browserPortHint")}
              </div>
            </div>
          </div>
        );
      }

      case "notifications":
        return (
          <div className="st-section">
            <div className="st-section-title text-heading">{t("settings.notificationsTitle")}</div>
            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.notificationTypesTitle")}</div>
              <div className="st-option-row">
                <span className="st-option-label text-body">{t("settings.notifyMonitorAlerts")}</span>
                <input type="checkbox" className="st-checkbox" defaultChecked />
              </div>
              <div className="st-option-row">
                <span className="st-option-label text-body">{t("settings.notifyFileOps")}</span>
                <input type="checkbox" className="st-checkbox" defaultChecked />
              </div>
              <div className="st-option-row">
                <span className="st-option-label text-body">{t("settings.notifyConnChange")}</span>
                <input type="checkbox" className="st-checkbox" defaultChecked />
              </div>
            </div>
            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.alertThresholdsTitle")}</div>
              <div className="st-slider-row">
                <span className="st-slider-label text-body">{t("settings.cpuAlert")}</span>
                <input type="range" min="50" max="100" defaultValue="80" className="st-slider" />
                <span className="st-slider-value">80%</span>
              </div>
              <div className="st-slider-row">
                <span className="st-slider-label text-body">{t("settings.diskAlert")}</span>
                <input type="range" min="50" max="100" defaultValue="90" className="st-slider" />
                <span className="st-slider-value">90%</span>
              </div>
            </div>
          </div>
        );

      case "stats": {
        // 前端根据用户名判断是否为 root（后端会做真正的权限校验）
        const isRoot = activeServer?.auth.username === 'root';
        const serverId = activeServer?.id || '';
        return (
          <div className="st-section">
            <div className="st-section-title text-heading">{t("settings.section.stats")}</div>
            {serverId ? (
              <StatsPanel serverId={serverId} isRoot={isRoot} />
            ) : (
              <div className="st-card">
                <div className="st-card-header text-title">{t("settings.statsNotConnectedTitle")}</div>
                <div className="text-body" style={{ padding: '12px 0' }}>
                  {t("settings.statsConnectFirst")}
                </div>
              </div>
            )}
          </div>
        );
      }

      case "about":
        return (
          <div className="st-section">
            <div className="st-section-title text-heading">{t("settings.section.about")}</div>
            <div className="st-card st-about-card">
              <div className="st-about-logo">
                <img src="/favicon.svg" alt="Quirel logo" draggable={false} />
              </div>
              <div className="st-about-name">Quirel</div>
              <div className="st-about-version">{t("settings.aboutVersion", { version: appVersion || "…" })}</div>
              <div className="st-about-desc">
                {t("settings.aboutDesc")}
              </div>
              <div className="st-about-tech">
                <span>Tauri 2.x + React 18</span>
                <span>{t("settings.aboutTechQuic")}</span>
                <span>{t("settings.aboutTechDesign")}</span>
              </div>
            </div>
            <div className="st-card">
              <div className="st-card-header text-title">{t("settings.techStackTitle")}</div>
              <div className="st-tech-list">
                <div className="st-tech-item">
                  <span className="st-tech-name">{t("settings.techFrontend")}</span>
                  <span className="st-tech-value">React 19 + TypeScript</span>
                </div>
                <div className="st-tech-item">
                  <span className="st-tech-name">{t("settings.techDesktop")}</span>
                  <span className="st-tech-value">Tauri 2.x (Rust)</span>
                </div>
                <div className="st-tech-item">
                  <span className="st-tech-name text-label">{t("settings.techTerminal")}</span>
                  <span className="st-tech-value text-caption">xterm.js 5.5</span>
                </div>
                <div className="st-tech-item">
                  <span className="st-tech-name text-label">{t("settings.techDesignSystem")}</span>
                  <span className="st-tech-value">Adwaita</span>
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
    <div className="st" data-headerbar-bridge="glass-left">
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
              <span className="st-sb-label text-label">{t(item.labelKey)}</span>
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