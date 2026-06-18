// src/components/terminal/HeaderBar.tsx
import { useTerminalStore } from '../../stores/terminalStore';

export function HeaderBar() {
  const { toggleSearchBar, toggleSettingsPanel, addTab } = useTerminalStore();

  const handleNewTab = () => {
    const newTab = {
      id: `tab-${Date.now()}`,
      label: `终端 ${useTerminalStore.getState().tabs.length + 1}`,
      sessionId: null,
    };
    addTab(newTab);
  };

  return (
    <div className="terminal-header-bar">
      <button className="header-button" onClick={handleNewTab} title="新建标签页">
        +
      </button>
      <button className="header-button" onClick={toggleSearchBar} title="搜索">
        🔍
      </button>
      <button className="header-button" onClick={toggleSettingsPanel} title="设置">
        ⚙
      </button>
      <button className="header-button" title="菜单">
        ⋮
      </button>
    </div>
  );
}