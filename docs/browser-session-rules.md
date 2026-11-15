# 浏览器会话所有权红线（原子性）

> 本文承接 AGENTS.md §5 红线总表第 1 行；改动涉及本文范围时先读全文。
> 回归：`src/window-system/core/WindowManager.session.test.ts`（原子性契约单测）。

浏览器会话（host + TabBook + 全部 tab webview）的生命周期**唯一归属窗口系统**，与宿主前端页面（React 树）完全解耦——页面刷新/重载/死亡对会话零影响，新页面经 browser_open（幂等）+ browser_refresh_tabs（全量回灌）重新挂载视图。

- 会话销毁**唯一入口**：registry 的 `lifecycle.onAfterClose`（init.ts browser 注册项）→ `proxy_stop_session` 全量停止。React 组件的 effect/cleanup **禁止**触碰会话销毁（browser_close_window / proxy_stop_session）——页面刷新会执行 cleanup，曾致多 tab 会话反复丢失（2026-10 三轮返工）。
- webview 可见性由持有方（runtime）强制执行：前端 1s 心跳（browser_heartbeat）中断 >3s 自动隐藏全部 webview，恢复后按命令态重.assert——禁止依赖前端指令的"一次性"可见性变更作为唯一手段。
- 所有 portal 到 body 的浮层必须带 `data-browser-overlay`（webview 子窗口为独立 HWND，DOM z-index 无法遮挡；缺失标记 = 浮层被网页盖住）。
- 新增浮层/改动可见性链路后，回归 `src/window-system/core/WindowManager.session.test.ts`（原子性契约单测）。
