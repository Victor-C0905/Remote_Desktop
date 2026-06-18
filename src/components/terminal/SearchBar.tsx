// src/components/terminal/SearchBar.tsx
import { useState } from 'react';
import { useTerminalStore } from '../../stores/terminalStore';

interface SearchBarProps {
  terminal: any;
}

export function SearchBar({ terminal }: SearchBarProps) {
  const { toggleSearchBar } = useTerminalStore();
  const [searchText, setSearchText] = useState('');

  const handleFindNext = () => {
    if (terminal && searchText) {
      terminal.findNext(searchText);
    }
  };

  const handleFindPrevious = () => {
    if (terminal && searchText) {
      terminal.findPrevious(searchText);
    }
  };

  const handleClose = () => {
    toggleSearchBar();
  };

  return (
    <div className="terminal-search-bar">
      <input
        type="text"
        value={searchText}
        onChange={(e) => setSearchText(e.target.value)}
        placeholder="搜索文本..."
        className="search-input"
      />
      <button className="search-button" onClick={handleFindPrevious}>
        ↑
      </button>
      <button className="search-button" onClick={handleFindNext}>
        ↓
      </button>
      <button className="search-button" onClick={handleClose}>
        ✕
      </button>
    </div>
  );
}