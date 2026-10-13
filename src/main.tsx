import "./i18n";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { isTauri, suppressTauriBridgeErrors } from "./utils/environment";

// 浏览器调试模式（无 Tauri 运行时）：静默桥接类报错，桌面会显示「浏览器模式」横幅
if (!isTauri()) {
  suppressTauriBridgeErrors();
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
