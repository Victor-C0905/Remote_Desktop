---
name: quirel-redlines-browser
description: Quirel 浏览器会话所有权红线。触碰浏览器会话生命周期（host/TabBook/tab webview）、init.ts browser 注册项、webview 可见性/心跳（browser_heartbeat）、data-browser-overlay 浮层时必须先加载本技能。React effect/cleanup 中的会话销毁是历史事故源。
---

# 浏览器会话所有权红线（AGENTS.md §5 红线表第 1 行）

## 改动前（强制）
1. 通读 `docs/browser-session-rules.md`（全文）。
2. 自查：本次改动是否在 React 组件 effect/cleanup 中新增或改动 browser_close_window /
   proxy_stop_session 调用？→ 禁止。会话销毁唯一入口是 registry 的
   lifecycle.onAfterClose（init.ts browser 注册项）。

## 改动后（强制回归）
- 运行契约单测：`npx vitest run src/window-system/core/WindowManager.session.test.ts`
- 新增 portal 浮层必须带 data-browser-overlay（webview 子窗口是独立 HWND，DOM
  z-index 挡不住）。
- 可见性变更不得依赖前端一次性指令：1s 心跳（browser_heartbeat）中断 >3s 自动隐藏，
  恢复后按命令态重.assert。

## 背景速览
浏览器会话生命周期唯一归属窗口系统，与 React 树解耦；页面刷新/死亡对会话零影响。
2026-10 曾因 cleanup 触碰销毁导致多 tab 会话反复丢失（三轮返工）。
