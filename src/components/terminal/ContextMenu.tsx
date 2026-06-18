// src/components/terminal/ContextMenu.tsx
import { useTerminalStore } from '../../stores/terminalStore';

interface ContextMenuProps {
  terminal: any;
}

export function ContextMenu({ terminal }: ContextMenuProps) {
  const { hideContextMenu, contextMenuPosition, addTab, removeTab, activeTabId, toggleSearchBar } = useTerminalStore();

  const handleCopy = () => {
    if (terminal) {
      const selection = terminal.getSelection();
      if (selection) {
        navigator.clipboard.writeText(selection);
      }
    }
    hideContextMenu();
  };

  const handlePaste = async () => {
    const text = await navigator.clipboard.readText();
    if (terminal && text) {
      // 发送粘贴内容到终端
      terminal.write(text);
    }
    hideContextMenu();
  };

  const handleSearchSelection = () => {
    if (terminal) {
      const selection = terminal.getSelection();
      if (selection) {
        // 设置搜索文本并显示搜索栏
        toggleSearchBar();
      }
    }
    hideContextMenu();
  };

  const handleNewTab = () => {
    const newTab = {
      id: `tab-${Date.now()}`,
      label: `终端 ${useTerminalStore.getState().tabs.length + 1}`,
      sessionId: null,
    };
    addTab(newTab);
    hideContextMenu();
  };

  const handleCloseTab = () => {
    removeTab(activeTabId);
    hideContextMenu();
  };

  return (
    <div
      className="terminal-context-menu"
      style={{
        position: 'fixed',
        left: contextMenuPosition.x,
        top: contextMenuPosition.y,
      }}
    >
      <button className="menu-item" onClick={handleCopy}>
        复制
      </button>
      <button className="menu-item" onClick={handlePaste}>
        粘贴
      </button>
      <button className="menu-item" onClick={handleSearchSelection}>
        搜索选中
      </button>
      <hr className="menu-divider" />
      <button className="menu-item" onClick={handleNewTab}>
        新建标签页
      </button>
      <button className="menu-item" onClick={handleCloseTab}>
        关闭标签页
      </button>
    </div>
  );
}