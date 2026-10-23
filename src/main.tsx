import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./style.css";

// 前端崩溃兜底:白屏无任何提示难排查,这里把未捕获错误直接钉在页面底部
const showFatal = (msg: string) => {
  const d = document.createElement("div");
  d.style.cssText =
    "position:fixed;left:0;right:0;bottom:0;background:#b0563f;color:#fff;padding:10px 14px;font:12px/1.6 ui-monospace,monospace;z-index:99999;white-space:pre-wrap;word-break:break-all";
  d.textContent = `前端错误:${msg}(请截图反馈,重启 app 可恢复)`;
  document.body.appendChild(d);
};
window.addEventListener("error", (e) => showFatal(e.message));
window.addEventListener("unhandledrejection", (e) => showFatal(String(e.reason)));

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
