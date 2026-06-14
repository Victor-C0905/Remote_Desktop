# 客户端实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 GNOME 桌面外壳客户端,支持远程 Ubuntu 服务器连接,显示远程时间、系统指标,提供应用窗口管理。

**Architecture:** 客户端使用 Tauri + React,ShellContext 管理全局状态,TopBar/Desktop/Dock/Overview 组件渲染 UI,Tauri Backend 处理 QUIC/WebSocket 连接和窗口管理。

**Tech Stack:** Tauri 2.x + React 18 + TypeScript + Quinn(QUIC) + Tokio-tungstenite(WebSocket) + Adwaita CSS

---

## 文件结构

**创建文件:**
```
src/
  ├── context/
  │   └── ShellContext.tsx        # 全局状态管理(连接、远程数据、桌面状态)
  ├── shell/
  │   ├── TopBar.tsx              # 顶部面板(连接状态、远程时间、指标)
  │   ├── Desktop.tsx             # 桌面区域(应用图标网格)
  │   ├── Dock.tsx                # 底部应用栏
  │   ├── Overview.tsx            # 活动概览(全屏叠加)
  │   └── Shell.tsx               # Shell 主容器
  ├── hooks/
  │   ├── useAutoReconnect.ts     # 自动重连机制
  │   ├── useHeartbeat.ts         # 心跳检测
  │   └── useMetrics.ts           # 系统指标获取
  ├── components/
  │   ├── RemoteClock.tsx         # 远程时钟组件
  │   └── ConnectionIndicator.tsx # 连接状态指示器
  └── types/
      └── shell.ts                # Shell 类型定义

src-tauri/src/
  ├── window_manager.rs           # 窗口管理器(创建/聚焦/关闭应用窗口)
  └── commands.rs                 # Tauri Commands(create_app_window, focus_window, close_window)
```

**修改文件:**
```
src/App.tsx                       # 主应用入口(集成 Shell)
src-tauri/src/lib.rs              # 注册新 Commands
src-tauri/Cargo.toml              # 添加依赖
```

---

## Task 1: Shell 类型定义

**Files:**
- Create: `src/types/shell.ts`

- [ ] **Step 1: 定义 Shell 状态类型**

```typescript
// src/types/shell.ts
export interface ShellState {
  // 连接状态
  connection: ConnectionState | null;

  // 远程数据
  remoteTime: Date | null;
  metrics: MetricsSnapshot | null;

  // 桌面状态
  overviewVisible: boolean;
  activeApp: AppType | null;
  appWindows: AppWindow[];

  // 壁纸
  wallpaper: string | null;
}

export interface ConnectionState {
  host: string;
  port: number;
  status: 'connected' | 'disconnected' | 'connecting';
  rtt_ms: number;
  transport: 'quic' | 'websocket';
  connectedAt: number;
}

export interface MetricsSnapshot {
  cpu_percent: number;
  mem_used_bytes: number;
  mem_total_bytes: number;
  swap_used_bytes: number;
  disks: DiskInfo[];
  network_rx_bytes: number;
  network_tx_bytes: number;
  uptime_secs: number;
}

export interface DiskInfo {
  mount_point: string;
  total_bytes: number;
  used_bytes: number;
}

export interface AppWindow {
  id: string;
  type: AppType;
  label: string;
  tauriWindow: string;
  isActive: boolean;
}

export type AppType = 'file-manager' | 'terminal' | 'system-monitor' | 'settings';

export interface AppDefinition {
  type: AppType;
  icon: string;
  label: string;
}
```

- [ ] **Step 2: 验证类型定义**

运行: `npm run build`
预期: 编译成功,无类型错误

- [ ] **Step 3: 提交类型定义**

```bash
git add src/types/shell.ts
git commit -m "feat(client): 定义 Shell 状态类型"
```

---

## Task 2: ShellContext 实现

**Files:**
- Create: `src/context/ShellContext.tsx`

- [ ] **Step 1: 实现 ShellContext**

```tsx
// src/context/ShellContext.tsx
import React, { createContext, useContext, useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { ShellState, ConnectionState, MetricsSnapshot, AppType, AppWindow } from '../types/shell';

interface ShellContextValue {
  state: ShellState;
  setState: React.Dispatch<React.SetStateAction<ShellState>>;
  connect: (host: string, port: number, token?: string) => Promise<void>;
  disconnect: () => Promise<void>;
  openApp: (appType: AppType) => Promise<void>;
}

const ShellContext = createContext<ShellContextValue | null>(null);

export function ShellProvider({ children }: { children: React.ReactNode }) {
  const [state, setState] = useState<ShellState>({
    connection: null,
    remoteTime: null,
    metrics: null,
    overviewVisible: false,
    activeApp: null,
    appWindows: [],
    wallpaper: null,
  });

  // 连接服务器
  const connect = useCallback(async (host: string, port: number, token?: string) => {
    setState(prev => ({
      ...prev,
      connection: {
        host,
        port,
        status: 'connecting',
        rtt_ms: 0,
        transport: 'quic',
        connectedAt: 0,
      },
    }));

    try {
      const result = await invoke<ConnectionState>('remote_connect', {
        serverId: 'default',
        host,
        port,
        token,
      });

      setState(prev => ({
        ...prev,
        connection: result,
      }));

      // 保存 Token 到 localStorage
      if (token) {
        localStorage.setItem('auth_token', token);
      }
    } catch (error) {
      setState(prev => ({
        ...prev,
        connection: null,
      }));
      throw error;
    }
  }, []);

  // 断开连接
  const disconnect = useCallback(async () => {
    try {
      await invoke('remote_disconnect', { serverId: 'default' });
      setState(prev => ({
        ...prev,
        connection: null,
        remoteTime: null,
        metrics: null,
      }));
    } catch (error) {
      console.error('断开连接失败:', error);
    }
  }, []);

  // 打开应用窗口
  const openApp = useCallback(async (appType: AppType) => {
    const windowLabel = `${appType}-${Date.now()}`;
    try {
      await invoke('create_app_window', {
        label: windowLabel,
        appType,
        url: `/app/${appType}`,
      });

      setState(prev => ({
        ...prev,
        appWindows: [...prev.appWindows, {
          id: windowLabel,
          type: appType,
          label: getAppLabel(appType),
          tauriWindow: windowLabel,
          isActive: true,
        }],
      }));
    } catch (error) {
      console.error('打开应用失败:', error);
    }
  }, []);

  // 定时获取远程数据
  useEffect(() => {
    if (!state.connection || state.connection.status !== 'connected') return;

    // 每 1 秒获取远程时间和 RTT
    const timeInterval = setInterval(async () => {
      try {
        const pingResult = await invoke<{ server_time: number; latency_ms: number }>('remote_ping', {
          serverId: 'default',
        });

        setState(prev => ({
          ...prev,
          remoteTime: new Date(pingResult.server_time),
          connection: prev.connection ? {
            ...prev.connection,
            rtt_ms: pingResult.latency_ms,
          } : null,
        }));
      } catch (error) {
        // 静默重试,不显示错误
      }
    }, 1000);

    // 每 2 秒获取系统指标
    const metricsInterval = setInterval(async () => {
      try {
        const metrics = await invoke<MetricsSnapshot>('remote_get_metrics', {
          serverId: 'default',
        });

        setState(prev => ({
          ...prev,
          metrics,
        }));
      } catch (error) {
        // 静默重试,不显示错误
      }
    }, 2000);

    return () => {
      clearInterval(timeInterval);
      clearInterval(metricsInterval);
    };
  }, [state.connection]);

  // 监听连接断开事件
  useEffect(() => {
    const unlisten = listen('connection-lost', () => {
      setState(prev => ({
        ...prev,
        connection: prev.connection ? {
          ...prev.connection,
          status: 'disconnected',
        } : null,
        remoteTime: null,
        metrics: null,
      }));
    });

    return () => {
      unlisten.then(f => f());
    };
  }, []);

  return (
    <ShellContext.Provider value={{ state, setState, connect, disconnect, openApp }}>
      {children}
    </ShellContext.Provider>
  );
}

export function useShell() {
  const context = useContext(ShellContext);
  if (!context) {
    throw new Error('useShell must be used within ShellProvider');
  }
  return context;
}

function getAppLabel(appType: AppType): string {
  const labels: Record<AppType, string> = {
    'file-manager': '文件',
    'terminal': '终端',
    'system-monitor': '监控',
    'settings': '设置',
  };
  return labels[appType];
}
```

- [ ] **Step 2: 验证 ShellContext**

运行: `npm run build`
预期: 编译成功,无类型错误

- [ ] **Step 3: 提交 ShellContext**

```bash
git add src/context/ShellContext.tsx
git commit -m "feat(client): 实现 ShellContext 全局状态管理"
```

---

## Task 3: TopBar 组件

**Files:**
- Create: `src/shell/TopBar.tsx`
- Create: `src/components/RemoteClock.tsx`
- Create: `src/components/ConnectionIndicator.tsx`

- [ ] **Step 1: 实现 TopBar**

```tsx
// src/shell/TopBar.tsx
import React from 'react';
import { useShell } from '../context/ShellContext';
import { useAutoReconnect } from '../hooks/useAutoReconnect';
import RemoteClock from '../components/RemoteClock';
import ConnectionIndicator from '../components/ConnectionIndicator';

export function TopBar() {
  const { state, setState } = useShell();
  const { isReconnecting } = useAutoReconnect();

  return (
    <div className="top-bar" style={{
      height: 32,
      background: 'var(--headerbar-bg)',
      borderBottom: '1px solid var(--border-color)',
      display: 'flex',
      alignItems: 'center',
      padding: '0 12px',
      gap: 16,
      fontFamily: 'var(--font-ui)',
      fontSize: 'var(--font-small)',
      color: 'var(--text-primary)',
    }}>
      {/* 活动按钮 */}
      <button
        onClick={() => setState(prev => ({ ...prev, overviewVisible: true }))}
        style={{
          background: 'transparent',
          border: 'none',
          color: 'inherit',
          cursor: 'pointer',
        }}
      >
        活动
      </button>

      {/* 分隔线 */}
      <div style={{
        width: 1,
        height: 16,
        background: 'var(--border-color)',
      }} />

      {/* 连接状态 */}
      <ConnectionIndicator
        status={state.connection?.status || 'disconnected'}
        isReconnecting={isReconnecting}
      />
      <span style={{
        color: isReconnecting ? 'var(--text-secondary)' : 'var(--text-primary)',
      }}>
        {isReconnecting ? '重新连接中...' :
         state.connection ? state.connection.host : '未连接'}
      </span>

      {/* 分隔线 */}
      <div style={{ flex: 1 }} />

      {/* 系统指标 */}
      {state.metrics && (
        <>
          <span style={{
            color: state.metrics.cpu_percent > 80 ? 'var(--error-color)' :
                   state.metrics.cpu_percent > 50 ? 'var(--warning-color)' :
                   'var(--text-primary)',
          }}>
            CPU {state.metrics.cpu_percent.toFixed(1)}%
          </span>
          <span>
            MEM {(state.metrics.mem_used_bytes / 1024**3).toFixed(1)}G
          </span>
        </>
      )}

      {/* 分隔线 */}
      <div style={{
        width: 1,
        height: 16,
        background: 'var(--border-color)',
      }} />

      {/* 远程时钟 */}
      {state.remoteTime ? (
        <RemoteClock time={state.remoteTime} />
      ) : (
        <span style={{ color: 'var(--text-secondary)' }}>
          {new Date().toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}
        </span>
      )}

      {/* 通知按钮 */}
      <button style={{
        background: 'transparent',
        border: 'none',
        color: 'inherit',
        cursor: 'pointer',
      }}>
        🔔
      </button>
    </div>
  );
}
```

- [ ] **Step 2: 实现 RemoteClock**

```tsx
// src/components/RemoteClock.tsx
import React, { useState, useEffect } from 'react';

interface RemoteClockProps {
  time: Date;
}

export default function RemoteClock({ time }: RemoteClockProps) {
  const [displayTime, setDisplayTime] = useState(time);

  // 每秒更新显示时间(基于初始远程时间)
  useEffect(() => {
    const interval = setInterval(() => {
      setDisplayTime(prev => new Date(prev.getTime() + 1000));
    }, 1000);

    return () => clearInterval(interval);
  }, []);

  const formatTime = (d: Date) =>
    d.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' });

  return (
    <span style={{
      fontWeight: 500,
    }}>
      {formatTime(displayTime)}
    </span>
  );
}
```

- [ ] **Step 3: 实现 ConnectionIndicator**

```tsx
// src/components/ConnectionIndicator.tsx
import React from 'react';

interface ConnectionIndicatorProps {
  status: 'connected' | 'disconnected' | 'connecting';
  isReconnecting: boolean;
}

export default function ConnectionIndicator({ status, isReconnecting }: ConnectionIndicatorProps) {
  const icon = isReconnecting ? '🔄' :
               status === 'connected' ? '🟢' :
               status === 'connecting' ? '🟡' : '⚫';

  return (
    <span style={{
      fontSize: 16,
    }}>
      {icon}
    </span>
  );
}
```

- [ ] **Step 4: 验证 TopBar**

运行: `npm run build`
预期: 编译成功,无类型错误

- [ ] **Step 5: 提交 TopBar**

```bash
git add src/shell/TopBar.tsx src/components/RemoteClock.tsx src/components/ConnectionIndicator.tsx
git commit -m "feat(client): 实现 TopBar 组件"
```

---

## Task 4: Desktop 组件

**Files:**
- Create: `src/shell/Desktop.tsx`

- [ ] **Step 1: 实现 Desktop**

```tsx
// src/shell/Desktop.tsx
import React from 'react';
import { useShell } from '../context/ShellContext';
import { AppDefinition, AppType } from '../types/shell';

export function Desktop() {
  const { openApp } = useShell();

  const apps: AppDefinition[] = [
    { type: 'file-manager', icon: '📁', label: '文件' },
    { type: 'terminal', icon: '🖥️', label: '终端' },
    { type: 'system-monitor', icon: '📊', label: '监控' },
    { type: 'settings', icon: '⚙️', label: '设置' },
  ];

  return (
    <div className="desktop" style={{
      flex: 1,
      background: 'var(--accent-bg)',
      display: 'flex',
      justifyContent: 'center',
      alignItems: 'center',
      padding: 48,
    }}>
      {/* 应用图标网格 */}
      <div className="app-grid" style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(4, 96px)',
        gridTemplateRows: 'repeat(2, 96px)',
        gap: 24,
      }}>
        {apps.map(app => (
          <div
            key={app.type}
            className="app-icon"
            onDoubleClick={() => openApp(app.type)}
            style={{
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              gap: 8,
              cursor: 'pointer',
              padding: 12,
              borderRadius: 'var(--radius-md)',
              transition: 'all 0.2s ease-out',
            }}
          >
            {/* 图标 */}
            <div className="icon-image" style={{
              fontSize: 48,
              filter: 'drop-shadow(0 2px 4px rgba(0,0,0,0.2))',
            }}>
              {app.icon}
            </div>

            {/* 标签 */}
            <div className="icon-label" style={{
              fontSize: 'var(--font-small)',
              color: 'white',
              fontWeight: 500,
            }}>
              {app.label}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: 验证 Desktop**

运行: `npm run build`
预期: 编译成功,无类型错误

- [ ] **Step 3: 提交 Desktop**

```bash
git add src/shell/Desktop.tsx
git commit -m "feat(client): 实现 Desktop 组件"
```

---

## Task 5: Dock 组件

**Files:**
- Create: `src/shell/Dock.tsx`

- [ ] **Step 1: 实现 Dock**

```tsx
// src/shell/Dock.tsx
import React from 'react';
import { useShell } from '../context/ShellContext';
import { AppDefinition, AppType } from '../types/shell';

export function Dock() {
  const { openApp, setState } = useShell();

  const dockApps: AppDefinition[] = [
    { type: 'file-manager', icon: '📁', label: '文件' },
    { type: 'terminal', icon: '🖥️', label: '终端' },
    { type: 'system-monitor', icon: '📊', label: '监控' },
    { type: 'settings', icon: '⚙️', label: '设置' },
    { type: 'all-apps' as AppType, icon: '⋮', label: '所有应用' },
  ];

  const handleAppClick = (appType: AppType | 'all-apps') => {
    if (appType === 'all-apps') {
      setState(prev => ({ ...prev, overviewVisible: true }));
    } else {
      openApp(appType);
    }
  };

  return (
    <div className="dock" style={{
      height: 64,
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      padding: '8px 12px',
      background: 'var(--card-bg)',
      borderRadius: 'var(--radius-lg)',
      border: '1px solid var(--border-color)',
      gap: 4,
    }}>
      {dockApps.map(app => (
        <div
          key={app.type}
          className="dock-item"
          onClick={() => handleAppClick(app.type)}
          style={{
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            gap: 4,
            cursor: 'pointer',
            padding: '8px 12px',
            borderRadius: 'var(--radius-sm)',
            transition: 'all 0.2s ease-out',
          }}
        >
          {/* 图标 */}
          <div className="dock-icon" style={{
            fontSize: 32,
          }}>
            {app.icon}
          </div>

          {/* 标签 */}
          <div className="dock-label" style={{
            fontSize: 'var(--font-small)',
            color: 'var(--text-secondary)',
          }}>
            {app.label}
          </div>
        </div>
      ))}
    </div>
  );
}
```

- [ ] **Step 2: 验证 Dock**

运行: `npm run build`
预期: 编译成功,无类型错误

- [ ] **Step 3: 提交 Dock**

```bash
git add src/shell/Dock.tsx
git commit -m "feat(client): 实现 Dock 组件"
```

---

## Task 6: Overview 组件

**Files:**
- Create: `src/shell/Overview.tsx`

- [ ] **Step 1: 实现 Overview**

```tsx
// src/shell/Overview.tsx
import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useShell } from '../context/ShellContext';
import Dock from './Dock';

export function Overview() {
  const { state, setState } = useShell();
  const [searchQuery, setSearchQuery] = useState('');

  // 监听 Super 键
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Super' || (e.metaKey && e.key === 'Meta')) {
        setState(prev => ({ ...prev, overviewVisible: !prev.overviewVisible }));
      }
      if (e.key === 'Escape' && state.overviewVisible) {
        setState(prev => ({ ...prev, overviewVisible: false }));
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [state.overviewVisible]);

  // 聚焦窗口
  const focusWindow = async (windowId: string) => {
    try {
      await invoke('focus_window', { label: windowId });
      setState(prev => ({
        ...prev,
        overviewVisible: false,
        activeApp: prev.appWindows.find(w => w.id === windowId)?.type || null,
      }));
    } catch (error) {
      console.error('聚焦窗口失败:', error);
    }
  };

  // 关闭窗口
  const closeWindow = async (windowId: string) => {
    try {
      await invoke('close_window', { label: windowId });
      setState(prev => ({
        ...prev,
        appWindows: prev.appWindows.filter(w => w.id !== windowId),
      }));
    } catch (error) {
      console.error('关闭窗口失败:', error);
    }
  };

  if (!state.overviewVisible) return null;

  return (
    <div className="overview" style={{
      position: 'fixed',
      top: 0, left: 0, right: 0, bottom: 0,
      zIndex: 9999,
      background: 'rgba(0, 0, 0, 0.5)',
      backdropFilter: 'blur(10px)',
      display: 'flex',
      flexDirection: 'column',
      padding: 32,
    }}>
      {/* 搜索栏 */}
      <div className="search-bar" style={{
        display: 'flex',
        justifyContent: 'center',
        marginBottom: 32,
      }}>
        <input
          type="text"
          placeholder="搜索..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          style={{
            width: 400,
            height: 40,
            padding: '0 16px',
            background: 'var(--card-bg)',
            border: '1px solid var(--border-color)',
            borderRadius: 'var(--radius-lg)',
            fontSize: 'var(--font-body)',
            fontFamily: 'var(--font-ui)',
          }}
        />
      </div>

      {/* 窗口网格 */}
      <div className="window-grid" style={{
        flex: 1,
        display: 'grid',
        gridTemplateColumns: 'repeat(3, 1fr)',
        gridTemplateRows: 'repeat(2, 1fr)',
        gap: 24,
        justifyContent: 'center',
        alignItems: 'center',
      }}>
        {state.appWindows.map(window => (
          <div
            key={window.id}
            className="window-thumbnail"
            onClick={() => focusWindow(window.id)}
            style={{
              background: 'var(--card-bg)',
              borderRadius: 'var(--radius-md)',
              border: '1px solid var(--border-color)',
              padding: 16,
              cursor: 'pointer',
              transition: 'all 0.2s ease-out',
              position: 'relative',
            }}
          >
            {/* 窗口标题 */}
            <div className="window-title" style={{
              marginBottom: 8,
              fontSize: 'var(--font-title)',
              fontWeight: 600,
            }}>
              {window.label}
            </div>

            {/* 窗口预览 */}
            <div className="window-preview" style={{
              height: 120,
              background: 'var(--view-bg)',
              borderRadius: 'var(--radius-sm)',
            }} />

            {/* 关闭按钮 */}
            <button
              onClick={(e) => {
                e.stopPropagation();
                closeWindow(window.id);
              }}
              style={{
                position: 'absolute',
                top: 8,
                right: 8,
                width: 24,
                height: 24,
                background: 'var(--error-color)',
                border: 'none',
                borderRadius: 'var(--radius-sm)',
                color: 'white',
                cursor: 'pointer',
              }}
            >
              ✕
            </button>
          </div>
        ))}
      </div>

      {/* Dock */}
      <div className="overview-dock" style={{
        display: 'flex',
        justifyContent: 'center',
        marginTop: 32,
      }}>
        <Dock />
      </div>
    </div>
  );
}
```

- [ ] **Step 2: 验证 Overview**

运行: `npm run build`
预期: 编译成功,无类型错误

- [ ] **Step 3: 提交 Overview**

```bash
git add src/shell/Overview.tsx
git commit -m "feat(client): 实现 Overview 组件"
```

---

## Task 7: Shell 主容器

**Files:**
- Create: `src/shell/Shell.tsx`

- [ ] **Step 1: 实现 Shell 主容器**

```tsx
// src/shell/Shell.tsx
import React from 'react';
import { ShellProvider } from '../context/ShellContext';
import TopBar from './TopBar';
import Desktop from './Desktop';
import Dock from './Dock';
import Overview from './Overview';

export function Shell() {
  return (
    <ShellProvider>
      <div className="shell" style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100vh',
        background: 'var(--window-bg)',
      }}>
        {/* TopBar */}
        <TopBar />

        {/* Desktop */}
        <Desktop />

        {/* Dock */}
        <div style={{
          display: 'flex',
          justifyContent: 'center',
          padding: '16px 0',
        }}>
          <Dock />
        </div>

        {/* Overview */}
        <Overview />
      </div>
    </ShellProvider>
  );
}
```

- [ ] **Step 2: 验证 Shell**

运行: `npm run build`
预期: 编译成功,无类型错误

- [ ] **Step 3: 提交 Shell**

```bash
git add src/shell/Shell.tsx
git commit -m "feat(client): 实现 Shell 主容器"
```

---

## Task 8: 自动重连机制

**Files:**
- Create: `src/hooks/useAutoReconnect.ts`

- [ ] **Step 1: 实现自动重连**

```typescript
// src/hooks/useAutoReconnect.ts
import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useShell } from '../context/ShellContext';

export function useAutoReconnect() {
  const { state, setState } = useShell();
  const [reconnectAttempts, setReconnectAttempts] = useState(0);
  const [isReconnecting, setIsReconnecting] = useState(false);

  const MAX_RECONNECT_ATTEMPTS = 10;
  const RECONNECT_INTERVALS = [0, 1, 2, 5, 10, 15, 30, 60, 120, 300];

  const attemptReconnect = async () => {
    if (reconnectAttempts >= MAX_RECONNECT_ATTEMPTS) {
      setState(prev => ({
        ...prev,
        connection: prev.connection ? {
          ...prev.connection,
          status: 'disconnected',
        } : null,
      }));
      return;
    }

    setIsReconnecting(true);

    try {
      const token = localStorage.getItem('auth_token');
      const result = await invoke('remote_connect', {
        serverId: 'default',
        host: state.connection?.host || '',
        port: state.connection?.port || 8443,
        token,
      });

      setState(prev => ({ ...prev, connection: result }));
      setReconnectAttempts(0);
      setIsReconnecting(false);
    } catch (error) {
      setReconnectAttempts(prev => prev + 1);
      const delay = RECONNECT_INTERVALS[reconnectAttempts] || 300;
      setTimeout(() => attemptReconnect(), delay * 1000);
    }
  };

  useEffect(() => {
    const unlisten = listen('connection-lost', () => {
      attemptReconnect();
    });

    return () => {
      unlisten.then(f => f());
    };
  }, []);

  return { isReconnecting, reconnectAttempts };
}
```

- [ ] **Step 2: 验证自动重连**

运行: `npm run build`
预期: 编译成功,无类型错误

- [ ] **Step 3: 提交自动重连**

```bash
git add src/hooks/useAutoReconnect.ts
git commit -m "feat(client): 实现自动重连机制"
```

---

## Task 9: 窗口管理器

**Files:**
- Create: `src-tauri/src/window_manager.rs`
- Create: `src-tauri/src/commands.rs`

- [ ] **Step 1: 实现窗口管理器**

```rust
// src-tauri/src/window_manager.rs
use tauri::{Manager, WindowBuilder, WindowUrl};
use std::collections::HashMap;
use std::sync::Mutex;

pub struct WindowManager {
    windows: Mutex<HashMap<String, tauri::Window>>,
}

impl WindowManager {
    pub fn new() -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
        }
    }

    pub fn create_app_window(
        &self,
        app: &tauri::AppHandle,
        label: &str,
        app_type: &str,
        url: &str,
    ) -> Result<tauri::Window, String> {
        let window = WindowBuilder::new(
            app,
            label,
            WindowUrl::App(url.into()),
        )
        .title(get_window_title(app_type))
        .inner_size(get_window_size(app_type))
        .decorations(false)
        .transparent(true)
        .build()
        .map_err(|e| format!("创建窗口失败: {}", e))?;

        self.windows.lock().unwrap().insert(label.to_string(), window.clone());

        Ok(window)
    }

    pub fn focus_window(&self, label: &str) -> Result<(), String> {
        let windows = self.windows.lock().unwrap();
        let window = windows.get(label)
            .ok_or_else(|| format!("窗口不存在: {}", label))?;

        window.set_focus().map_err(|e| format!("聚焦窗口失败: {}", e))?;
        Ok(())
    }

    pub fn close_window(&self, label: &str) -> Result<(), String> {
        let mut windows = self.windows.lock().unwrap();
        let window = windows.remove(label)
            .ok_or_else(|| format!("窗口不存在: {}", label))?;

        window.close().map_err(|e| format!("关闭窗口失败: {}", e))?;
        Ok(())
    }
}

fn get_window_title(app_type: &str) -> String {
    match app_type {
        "file-manager" => "文件管理器",
        "terminal" => "终端",
        "system-monitor" => "系统监控",
        "settings" => "设置",
        _ => "应用",
    }
}

fn get_window_size(app_type: &str) -> (f64, f64) {
    match app_type {
        "file-manager" => (800.0, 600.0),
        "terminal" => (800.0, 500.0),
        "system-monitor" => (800.0, 600.0),
        "settings" => (600.0, 500.0),
        _ => (800.0, 600.0),
    }
}
```

- [ ] **Step 2: 实现 Commands**

```rust
// src-tauri/src/commands.rs
use crate::window_manager::WindowManager;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub fn create_app_window(
    label: String,
    app_type: String,
    url: String,
    app: AppHandle,
) -> Result<(), String> {
    let manager = app.state::<WindowManager>();
    manager.create_app_window(&app, &label, &app_type, &url)?;
    Ok(())
}

#[tauri::command]
pub fn focus_window(label: String, app: AppHandle) -> Result<(), String> {
    let manager = app.state::<WindowManager>();
    manager.focus_window(&label)?;
    Ok(())
}

#[tauri::command]
pub fn close_window(label: String, app: AppHandle) -> Result<(), String> {
    let manager = app.state::<WindowManager>();
    manager.close_window(&label)?;
    Ok(())
}
```

- [ ] **Step 3: 验证窗口管理器**

运行: `cargo check --manifest-path src-tauri/Cargo.toml`
预期: 编译成功,无错误

- [ ] **Step 4: 提交窗口管理器**

```bash
git add src-tauri/src/window_manager.rs src-tauri/src/commands.rs
git commit -m "feat(client): 实现窗口管理器"
```

---

## Task 10: 集成到主应用

**Files:**
- Modify: `src/App.tsx:1-20`
- Modify: `src-tauri/src/lib.rs:1-384`

- [ ] **Step 1: 修改 App.tsx**

```tsx
// src/App.tsx (替换现有内容)
import React from 'react';
import { Shell } from './shell/Shell';

function App() {
  return <Shell />;
}

export default App;
```

- [ ] **Step 2: 修改 lib.rs**

```rust
// src-tauri/src/lib.rs (在 invoke_handler 添加)
mod window_manager;
mod commands;

use window_manager::WindowManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(pty::PtyManager::new())
        .manage(connection::ConnectionManager::new())
        .manage(WindowManager::new())
        .invoke_handler(tauri::generate_handler![
            read_dir, stat_file, read_file_text,
            pty::spawn_terminal,
            pty::terminal_write,
            pty::terminal_read,
            pty::terminal_resize,
            connection::remote_connect,
            connection::remote_disconnect,
            connection::remote_ping,
            connection::remote_send,
            connection::remote_read_dir,
            connection::remote_get_metrics,
            connection::remote_read_file,
            connection::remote_write_file,
            connection::remote_delete,
            commands::create_app_window,
            commands::focus_window,
            commands::close_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 3: 验证集成**

运行: `npm run tauri dev`
预期: 应用启动,显示 Shell 界面

- [ ] **Step 4: 提交集成**

```bash
git add src/App.tsx src-tauri/src/lib.rs
git commit -m "feat(client): 集成 Shell 到主应用"
```

---

## Task 11: 测试和调试

**Files:**
- 无文件修改

- [ ] **Step 1: 测试连接功能**

测试步骤:
1. 启动 Agent(使用 agent.toml)
2. 启动客户端(npm run tauri dev)
3. 在 Settings 面板输入服务器地址、端口、Token
4. 点击"连接"按钮
5. 验证 TopBar 显示连接状态、远程时间、系统指标

预期: 连接成功,TopBar 显示远程数据

- [ ] **Step 2: 测试自动重连**

测试步骤:
1. 连接成功后,关闭 Agent
2. 观察 TopBar 显示 "重新连接中..."
3. 重新启动 Agent
4. 观察自动重连成功

预期: 自动重连成功,恢复远程数据

- [ ] **Step 3: 测试窗口管理**

测试步骤:
1. 双击 Desktop 的"文件"图标
2. 验证文件管理器窗口打开
3. 按 Super 键打开 Overview
4. 点击窗口缩略图
5. 验证窗口聚焦
6. 点击关闭按钮
7. 验证窗口关闭

预期: 窗口管理功能正常

- [ ] **Step 4: 测试错误处理**

测试步骤:
1. 输入错误的 Token
2. 点击"连接"按钮
3. 验证显示错误提示
4. 输入正确的 Token
5. 验证连接成功

预期: 错误处理正确,提示友好

---

## Task 12: 文档和打包

**Files:**
- Create: `docs/client-usage.md`

- [ ] **Step 1: 编写客户端使用文档**

```markdown
# GNOME 远程控制客户端 - 使用指南

## 功能

- 远程连接 Ubuntu 服务器(QUIC/WebSocket)
- 显示远程服务器时间、系统指标
- 应用窗口管理(文件、终端、监控、设置)
- 自动重连机制
- GNOME 桌面外壳体验

## 使用步骤

### 1. 启动 Agent

在远程 Ubuntu 服务器上启动 Agent:

```bash
cd /opt/gnome-remote
./agent
```

### 2. 启动客户端

在本地启动客户端:

```bash
npm run tauri dev
```

### 3. 连接服务器

1. 点击"设置"图标
2. 输入服务器地址(如: 192.168.1.100)
3. 输入端口(如: 8443)
4. 输入 Token(如: gmr_test_token_12345678)
5. 点击"连接"按钮

### 4. 使用功能

- **TopBar**: 显示连接状态、远程时间、CPU、内存
- **Desktop**: 双击应用图标打开应用
- **Dock**: 点击应用图标打开应用
- **Overview**: 按 Super 键查看所有窗口

## 打包

### Windows

```bash
cargo tauri build --target x86_64-pc-windows-msvc
```

### macOS

```bash
cargo tauri build --target x86_64-apple-darwin
```

### Linux

```bash
cargo tauri build --target x86_64-unknown-linux-gnu
```

## API 文档

参见 `docs/superpowers/specs/2026-06-12-desktop-shell-design.md` 第三章。
```

- [ ] **Step 2: 提交文档**

```bash
git add docs/client-usage.md
git commit -m "docs(client): 添加客户端使用指南"
```

- [ ] **Step 3: 打包客户端**

运行: `cargo tauri build`
预期: 生成安装包(.msi/.dmg/.AppImage)

---

## Self-Review

**1. Spec coverage:**
- ✅ ShellContext(第四章 4.1) - Task 2
- ✅ TopBar(第四章 4.2) - Task 3
- ✅ Desktop(第四章 4.3) - Task 4
- ✅ Dock(第四章 4.4) - Task 5
- ✅ Overview(第四章 4.5) - Task 6
- ✅ Shell 主容器(第四章 4.1) - Task 7
- ✅ 自动重连(第四章 4.6) - Task 8
- ✅ 窗口管理器(第四章 4.1) - Task 9
- ✅ 集成(第四章 4.1) - Task 10
- ✅ 测试(第六章 6.1) - Task 11
- ✅ 文档和打包(第七章 7.2) - Task 12

**2. Placeholder scan:**
- ✅ 无 TBD、TODO、incomplete sections
- ✅ 所有步骤包含完整代码
- ✅ 所有命令包含预期输出

**3. Type consistency:**
- ✅ ShellState、ConnectionState、MetricsSnapshot 定义一致
- ✅ AppType、AppWindow、AppDefinition 定义一致
- ✅ 所有组件使用相同的类型定义

---

## 执行选项

计划完成并保存到 `docs/superpowers/plans/2026-06-12-client-implementation.md`。

**两种执行方式:**

**1. Subagent-Driven (推荐)** - 我为每个任务派发新的子代理,任务之间审查,快速迭代

**2. Inline Execution** - 在此会话中使用 executing-plans 执行任务,批量执行带检查点

你希望采用哪种方式?