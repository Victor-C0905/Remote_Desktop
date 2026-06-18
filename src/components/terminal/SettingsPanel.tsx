// src/components/terminal/SettingsPanel.tsx
import { useTerminalStore } from '../../stores/terminalStore';

export function SettingsPanel() {
  const { settings, updateSettings, toggleSettingsPanel } = useTerminalStore();

  const handleClose = () => {
    toggleSettingsPanel();
  };

  return (
    <div className="terminal-settings-panel">
      <h3>终端设置</h3>

      <label className="settings-label">
        字体大小:
        <input
          type="range"
          min="10"
          max="24"
          value={settings.fontSize}
          onChange={(e) => updateSettings({ fontSize: parseInt(e.target.value) })}
          className="settings-slider"
        />
        <span className="settings-value">{settings.fontSize}</span>
      </label>

      <label className="settings-label">
        字体类型:
        <select
          value={settings.fontFamily}
          onChange={(e) => updateSettings({ fontFamily: e.target.value })}
          className="settings-select"
        >
          <option value="Consolas">Consolas</option>
          <option value="'Cascadia Code'">Cascadia Code</option>
          <option value="'Source Code Pro'">Source Code Pro</option>
        </select>
      </label>

      <label className="settings-label">
        配色方案:
        <select
          value={settings.theme}
          onChange={(e) => updateSettings({ theme: e.target.value })}
          className="settings-select"
        >
          <option value="gnome-dark">Dark</option>
          <option value="gnome-light">Light</option>
          <option value="gnome-white">White</option>
        </select>
      </label>

      <label className="settings-label">
        光标样式:
        <select
          value={settings.cursorStyle}
          onChange={(e) => updateSettings({ cursorStyle: e.target.value as 'block' | 'underline' | 'bar' })}
          className="settings-select"
        >
          <option value="block">Block</option>
          <option value="underline">Underline</option>
          <option value="bar">Bar</option>
        </select>
      </label>

      <label className="settings-label">
        光标闪烁:
        <input
          type="checkbox"
          checked={settings.cursorBlink}
          onChange={(e) => updateSettings({ cursorBlink: e.target.checked })}
          className="settings-checkbox"
        />
      </label>

      <button className="settings-close-button" onClick={handleClose}>
        关闭
      </button>
    </div>
  );
}