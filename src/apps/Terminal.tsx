import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./Terminal.css";

/* ── Types ─────────────────────────────────────────────── */

interface TabInfo {
  id: string;
  label: string;
  ptyId: string | null;
  xtermReady: boolean;
}

/* ── GNOME Terminal Color Theme (CSS-friendly) ──────── */

const GNOME_THEME = {
  bg: "#1e1e1e",
  fg: "#ffffff",
  cursor: "#4ec9b0",
  green: "#4e9a06",
  blue: "#3465a4",
  yellow: "#c4a000",
  red: "#cc0000",
};

/* ── Demo command handler ────────────────────────────── */

function getPrompt(): string {
  return `\x1b[1;32muser@gnome-remote\x1b[0m:\x1b[1;34m~\x1b[0m$ `;
}

function processDemoCommand(cmd: string): string {
  const trimmed = cmd.trim().toLowerCase();
  if (!trimmed) return getPrompt();

  switch (trimmed) {
    case "help":
      return [
        "\x1b[1m可用命令 (演示模式):\x1b[0m",
        "  help      — 显示此帮助",
        "  uname     — 显示系统信息",
        "  whoami    — 显示当前用户",
        "  ls        — 列出文件",
        "  date      — 显示日期时间",
        "  uptime    — 显示运行时间",
        "  neofetch  — 系统信息概览",
        "  clear     — 清屏",
        "",
        getPrompt(),
      ].join("\r\n");

    case "uname":
    case "uname -a":
      return "GNOME-Remote 0.1.0 (demo) x86_64 GNU/Linux\r\n" + getPrompt();

    case "whoami":
      return "user\r\n" + getPrompt();

    case "ls":
      return [
        "\x1b[1;34mDocuments/\x1b[0m  \x1b[1;34mDownloads/\x1b[0m  \x1b[1;34mPictures/\x1b[0m",
        "\x1b[1;34m.config/\x1b[0m    \x1b[1;34m.ssh/\x1b[0m       note.txt",
        "config.toml  README.md",
        getPrompt(),
      ].join("\r\n");

    case "date":
      return new Date().toString() + "\r\n" + getPrompt();

    case "uptime":
      return " " + new Date().toLocaleTimeString() + " up 42 days, 3:17, 1 user\r\n" + getPrompt();

    case "clear":
      return "\x1b[2J\x1b[H" + getPrompt();

    case "neofetch":
      return [
        "\x1b[1;32m       _,met$$$$$gg.          \x1b[0m \x1b[1muser@gnome-remote\x1b[0m",
        "\x1b[1;32m    ,g$$$$$$$$$$$$$$$P.        \x1b[0m ──────────────────",
        "\x1b[1;32m  ,g$$P\"     \"\"\"Y$$.\"\".       \x1b[0m \x1b[1mOS:\x1b[0m GNOME Remote 0.1.0",
        "\x1b[1;32m ,$$P'               `$$$.      \x1b[0m \x1b[1mKernel:\x1b[0m Tauri 2.0 + React",
        "\x1b[1;32m',$$P       ,ggs.     `$$b:    \x1b[0m \x1b[1mShell:\x1b[0m xterm.js 5.5",
        "\x1b[1;32m`d$$'     ,$P\"'   .    $$$     \x1b[0m \x1b[1mTerminal:\x1b[0m GNOME Terminal",
        "\x1b[1;32m $$P      d$'     ,    $$P     \x1b[0m \x1b[1mCPU:\x1b[0m Rust (quinn QUIC)",
        "\x1b[1;32m $$;      Y$b._   _,d$P'       \x1b[0m \x1b[1mMemory:\x1b[0m ~8MB Agent",
        "\x1b[1;32m Y$$.    `.`\"Y$$$$P\"'          \x1b[0m",
        "\x1b[1;32m  `Y$b                        \x1b[0m \x1b[1;31m■\x1b[1;32m■\x1b[1;33m■\x1b[1;34m■\x1b[1;35m■\x1b[1;36m■\x1b[1;37m■\x1b[0m",
        "",
        getPrompt(),
      ].join("\r\n");

    default:
      return `\x1b[33m命令未找到: ${cmd}\x1b[0m\r\n输入 \x1b[1mhelp\x1b[0m 查看可用命令\r\n` + getPrompt();
  }
}

/* ── Component ───────────────────────────────────────── */

export function TerminalApp() {
  const [tabs, setTabs] = useState<TabInfo[]>([
    { id: "tab-0", label: "终端 1", ptyId: null, xtermReady: false },
  ]);
  const [activeTabId, setActiveTabId] = useState("tab-0");
  const [xtermAvailable, setXtermAvailable] = useState(false);
  const [xtermModules, setXtermModules] = useState<{
    Terminal: any;
    FitAddon: any;
    WebLinksAddon: any;
  } | null>(null);
  const xtermRefs = useRef<Map<string, any>>(new Map());
  const fitAddonRefs = useRef<Map<string, any>>(new Map());
  const containerRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const tabCounterRef = useRef(1);

  // ── Load xterm dynamically ──────────────────────
  useEffect(() => {
    (async () => {
      try {
        const xterm = await import("@xterm/xterm");
        const fit = await import("@xterm/addon-fit");
        const weblinks = await import("@xterm/addon-web-links");
        setXtermModules({
          Terminal: xterm.Terminal,
          FitAddon: fit.FitAddon,
          WebLinksAddon: weblinks.WebLinksAddon,
        });
        setXtermAvailable(true);
      } catch {
        setXtermAvailable(false);
        console.warn("[Terminal] xterm.js not available, using fallback");
      }
    })();
  }, []);

  // ── GNOME Terminal theme ──────────────────────
  const GNOME_TERMINAL_THEME = {
    background: GNOME_THEME.bg,
    foreground: GNOME_THEME.fg,
    cursor: GNOME_THEME.cursor,
    cursorAccent: GNOME_THEME.bg,
    selectionBackground: "rgba(78, 201, 176, 0.3)",
    selectionForeground: "#ffffff",
    black: "#1e1e1e",
    red: "#cc0000",
    green: "#4e9a06",
    yellow: "#c4a000",
    blue: "#3465a4",
    magenta: "#75507b",
    cyan: "#06989a",
    white: "#d3d7cf",
    brightBlack: "#555753",
    brightRed: "#ef2929",
    brightGreen: "#8ae234",
    brightYellow: "#fce94f",
    brightBlue: "#729fcf",
    brightMagenta: "#ad7fa8",
    brightCyan: "#34e2e2",
    brightWhite: "#eeeeec",
  };

  // ── Create xterm instance ──────────────────────
  const createTerminal = useCallback(
    (tabId: string, container: HTMLDivElement) => {
      if (!xtermModules) return;

      // Clean up existing
      const existing = xtermRefs.current.get(tabId);
      if (existing) {
        existing.dispose();
        xtermRefs.current.delete(tabId);
        fitAddonRefs.current.delete(tabId);
      }

      const term = new xtermModules.Terminal({
        theme: GNOME_TERMINAL_THEME,
        fontFamily: "'Source Code Pro', 'Cascadia Code', monospace",
        fontSize: 13,
        lineHeight: 1.2,
        cursorBlink: true,
        cursorStyle: "block",
        scrollback: 5000,
        allowProposedApi: true,
      });

      const fitAddon = new xtermModules.FitAddon();
      const webLinksAddon = new xtermModules.WebLinksAddon();

      term.loadAddon(fitAddon);
      term.loadAddon(webLinksAddon);
      term.open(container);
      fitAddon.fit();

      xtermRefs.current.set(tabId, term);
      fitAddonRefs.current.set(tabId, fitAddon);
      containerRefs.current.set(tabId, container);

      // ── Input ──────────────────────────────
      let currentLine = "";
      term.onData((data: string) => {
        if (data === "\r") {
          term.write("\r\n");
          const output = processDemoCommand(currentLine);
          if (output.startsWith("\x1b[2J")) {
            term.clear();
          }
          term.write(output);
          currentLine = "";
        } else if (data === "\x7f") {
          if (currentLine.length > 0) {
            currentLine = currentLine.slice(0, -1);
            term.write("\b \b");
          }
        } else if (data === "\x03") {
          term.write("^C\r\n");
          currentLine = "";
          term.write(getPrompt());
        } else if (data >= " ") {
          currentLine += data;
          term.write(data);
        }
      });

      // Welcome
      term.writeln("\x1b[1mGNOME Remote Terminal\x1b[0m");
      term.writeln("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
      term.writeln("");

      // Try PTY
      spawnPty(tabId, term);
    },
    [xtermModules]
  );

  // ── Spawn PTY via Tauri command ────────────────
  const spawnPty = async (tabId: string, term: any) => {
    try {
      const result = await invoke<{ pty_id: string }>("spawn_terminal", {
        shell: "",
        cols: 80,
        rows: 24,
      });
      setTabs((prev) =>
        prev.map((t) =>
          t.id === tabId ? { ...t, ptyId: result.pty_id, xtermReady: true } : t
        )
      );
      pollPtyOutput(result.pty_id, term);
    } catch {
      term.writeln("\x1b[33m[演示模式]\x1b[0m PTY 不可用，使用本地回显");
      term.writeln("输入 \x1b[1mhelp\x1b[0m 查看可用命令");
      term.writeln("");
      term.write(getPrompt());
      setTabs((prev) =>
        prev.map((t) => (t.id === tabId ? { ...t, xtermReady: true } : t))
      );
    }
  };

  // ── Poll PTY output ─────────────────────────────
  const pollPtyOutput = async (ptyId: string, term: any) => {
    const poll = async () => {
      try {
        const result = await invoke<{ data: string }>("terminal_read", {
          ptyId,
          timeoutMs: 100,
        });
        if (result.data) {
          const bytes = atob(result.data);
          term.write(bytes);
        }
      } catch {
        term.writeln("\r\n\x1b[31m[连接断开]\x1b[0m");
        return;
      }
      requestAnimationFrame(poll);
    };
    poll();
  };

  // ── Add tab ─────────────────────────────────────
  const addTab = useCallback(() => {
    tabCounterRef.current += 1;
    const newTab: TabInfo = {
      id: `tab-${Date.now()}`,
      label: `终端 ${tabCounterRef.current}`,
      ptyId: null,
      xtermReady: false,
    };
    setTabs((prev) => [...prev, newTab]);
    setActiveTabId(newTab.id);
  }, []);

  // ── Close tab ──────────────────────────────────
  const closeTab = useCallback(
    (tabId: string, e?: React.MouseEvent) => {
      e?.stopPropagation();
      const term = xtermRefs.current.get(tabId);
      if (term) {
        term.dispose();
        xtermRefs.current.delete(tabId);
        fitAddonRefs.current.delete(tabId);
        containerRefs.current.delete(tabId);
      }
      setTabs((prev) => {
        const next = prev.filter((t) => t.id !== tabId);
        if (activeTabId === tabId && next.length > 0) {
          setActiveTabId(next[next.length - 1].id);
        }
        if (next.length === 0) {
          const fresh: TabInfo = {
            id: `tab-${Date.now()}`,
            label: "终端 1",
            ptyId: null,
            xtermReady: false,
          };
          tabCounterRef.current = 1;
          setActiveTabId(fresh.id);
          return [fresh];
        }
        return next;
      });
    },
    [activeTabId]
  );

  // ── Resize ────────────────────────────────────
  useEffect(() => {
    const handleResize = () => {
      fitAddonRefs.current.forEach((addon) => {
        try { addon.fit(); } catch { /* ignore */ }
      });
    };
    window.addEventListener("resize", handleResize);
    return () => window.removeEventListener("resize", handleResize);
  }, []);

  // ── Cleanup ────────────────────────────────────
  useEffect(() => {
    return () => {
      xtermRefs.current.forEach((term) => term.dispose());
      xtermRefs.current.clear();
      fitAddonRefs.current.clear();
      containerRefs.current.clear();
    };
  }, []);

  const activeTab = tabs.find((t) => t.id === activeTabId);
  const isConnected = activeTab?.ptyId !== null && activeTab?.ptyId !== undefined;

  // ── Fallback: simple textarea terminal ──────────
  const renderFallbackTerminal = () => (
    <div className="terminal-fallback">
      <div className="terminal-fallback-output" />
      <div className="terminal-fallback-input">
        <span className="prompt">user@gnome-remote:~$</span>
        <input
          className="fallback-input"
          type="text"
          autoFocus
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              const target = e.target as HTMLInputElement;
              const cmd = target.value;
              target.value = "";
              const outputEl = document.querySelector(".terminal-fallback-output");
              if (outputEl) {
                const line = document.createElement("div");
                line.textContent = `$ ${cmd}`;
                outputEl.appendChild(line);
                outputEl.scrollTop = outputEl.scrollHeight;
              }
            }
          }}
        />
      </div>
    </div>
  );

  return (
    <div className="terminal-app">
      {/* Tab Bar */}
      <div className="terminal-tab-bar">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            className={`terminal-tab${tab.id === activeTabId ? " active" : ""}`}
            onClick={() => setActiveTabId(tab.id)}
          >
            <span className="tab-icon">⌘</span>
            <span className="tab-label">{tab.label}</span>
            {tabs.length > 1 && (
              <button className="tab-close" onClick={(e) => closeTab(tab.id, e)}>
                ×
              </button>
            )}
          </button>
        ))}
        <button className="terminal-new-tab" onClick={addTab} title="新建终端">
          +
        </button>
      </div>

      {/* Terminal Content */}
      <div className="terminal-container">
        {xtermAvailable && xtermModules ? (
          tabs.map((tab) => (
            <div
              key={tab.id}
              className="terminal-instance"
              ref={(el) => {
                if (el && !xtermRefs.current.has(tab.id) && xtermModules) {
                  createTerminal(tab.id, el);
                }
              }}
              style={{
                display: tab.id === activeTabId ? "block" : "none",
                height: "100%",
              }}
            />
          ))
        ) : (
          renderFallbackTerminal()
        )}
      </div>

      {/* Status Bar */}
      <div className="terminal-status-bar">
        <div className="status-item">
          <div className={`status-dot${isConnected ? "" : " disconnected"}`} />
          <span>{isConnected ? "已连接" : xtermAvailable ? "演示模式" : "xterm 未安装"}</span>
        </div>
        <div className="status-item">
          <span>{activeTab?.label}</span>
        </div>
        <div className="status-spacer" />
        <div className="status-item">
          <span>{xtermAvailable ? "xterm.js" : "fallback"}</span>
        </div>
      </div>
    </div>
  );
}
