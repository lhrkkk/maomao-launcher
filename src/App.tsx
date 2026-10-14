import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const CAT_LOGO = (
  <svg width="30" height="30" viewBox="0 0 44 44" fill="none" aria-hidden="true">
    <path d="M8 19 9 5l10 8c2-.5 4-.6 6 0l10-8 1 14c2 4 3 8 0 12-3 6-9 8-15 8S9 37 7 31c-2-4-1-8 1-12Z" fill="#fff7ec" stroke="#31283a" strokeWidth="2.5" strokeLinejoin="round"/>
    <path d="M13.5 26.5v1m17-1v1" stroke="#31283a" strokeWidth="3.5" strokeLinecap="round"/>
    <path d="m19 30 3 2 3-2" stroke="#31283a" strokeWidth="1.8" strokeLinecap="round"/>
    <path d="m9 30-6-1m6 5-5 2m31-6 6-1m-6 5 5 2" stroke="#31283a" strokeWidth="1.6" strokeLinecap="round"/>
    <ellipse cx="12" cy="31" rx="3.2" ry="2" fill="#ffb9ae"/>
    <ellipse cx="32" cy="31" rx="3.2" ry="2" fill="#ffb9ae"/>
  </svg>
);

const ICONS: Record<string, string> = {
  claude: "✳", codex: "⌘", grok: "𝕏", opencode: "{ }",
  openclaw: "🦞", gemini: "✦", hermes: "❖", pi: "π",
  "claude-desktop": "✳", "codex-desktop": "⌘",
};

interface Tool {
  id: string;
  name: string;
  kind: "Cli" | "Desktop";
  installed: boolean;
  configured: boolean;
  configurable: boolean;
}
interface LoginInfo { username: string; key_masked: string }

const openUrl = (url: string) => invoke("open_url", { url }).catch(() => {});
const openInApp = (url: string) => invoke("open_site_page", { url });

export default function App() {
  const [tab, setTab] = useState<"login" | "key">("login");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [tools, setTools] = useState<Tool[]>([]);
  const [account, setAccount] = useState<LoginInfo | null>(null);
  const [panelOpen, setPanelOpen] = useState(false);
  const [panelSrc, setPanelSrc] = useState("https://binaryworld.world/panel");
  const PANEL_HOME = "https://binaryworld.world/panel";
  const [toast, setToast] = useState("");
  const toastTimer = useRef<number | undefined>(undefined);

  const refresh = () =>
    invoke<{ tools: Tool[]; login: LoginInfo | null }>("get_state")
      .then((s) => { setTools(s.tools); setAccount(s.login); })
      .catch(() => {});
  useEffect(() => {
    (async () => {
      try {
        const st = await invoke<{ tools: Tool[]; login: LoginInfo | null }>("get_state");
        setTools(st.tools);
        setAccount(st.login);
        if (st.login?.key) {
          await invoke("save_key", { key: st.login.key }); // 重新接管
          const st2 = await invoke<{ tools: Tool[]; login: LoginInfo | null }>("get_state");
          setTools(st2.tools);
        }
      } catch {}
    })();
  }, []);
  useEffect(() => {
    const onMsg = (e: MessageEvent) => {
      if (e.data && e.data.type === "maomao-pay" && typeof e.data.url === "string") {
        setPanelSrc(e.data.url);
      }
    };
    window.addEventListener("message", onMsg);
    return () => window.removeEventListener("message", onMsg);
  }, []);

  const say = (t: string) => {
    setToast(t);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(""), 2600);
  };

  const login = async () => {
    if (!username.trim() || !password) { say("请输入账号和密码,喵"); return; }
    setBusy(true);
    try {
      const mask = await invoke<string>("login_and_setup", { username, password });
      setUsername(""); setPassword("");
      say(`登录成功,已创建并配置密钥 ${mask} 🐾`);
      await refresh();
    } catch (e) { say(`${e}`); }
    setBusy(false);
  };

  const doLogout = async () => {
    try {
      await invoke("logout");
      setAccount(null);
      say("已退出账号,本机配置已还原");
      await refresh();
    } catch (e) { say(`${e}`); }
  };

  const save = async () => {
    const k = key.trim();
    if (!k) { say("请先填入 API Key,喵"); return; }
    try {
      await invoke("save_key", { key: k });
      setKey("");
      say("配置已写入全部客户端 🐾");
      await refresh();
    } catch (e) { say(`${e}`); }
  };

  const launch = async (t: Tool) => {
    if (t.configurable && !t.configured) { say("请先登录或保存 API Key"); return; }
    try {
      await invoke("launch", { id: t.id });
      say(`正在启动 ${t.name}…`);
    } catch (e) { say(`${e}`); }
  };

  const anyConfigured = tools.some((t) => t.configured);
  const cli = tools.filter((t) => t.kind === "Cli");
  const desktop = tools.filter((t) => t.kind === "Desktop");

  const row = (t: Tool) => (
    <button
      key={t.id}
      className="tool-row"
      disabled={!t.installed}
      onClick={() => launch(t)}
    >
      <span className="tool-ico">{ICONS[t.id] ?? "✧"}</span>
      <span className="tool-name">
        {t.name}
        {t.configurable && (
          <span className={`cfg ${t.configured ? "cfg-ok" : ""}`}>
            {t.configured ? "已配置" : "待配置"}
          </span>
        )}
      </span>
      <span className={`tool-state ${t.installed ? "st-ok" : "st-warn"}`}>
        {t.installed ? "启动 ↗" : "未安装"}
      </span>
    </button>
  );

  return (
    <div className="shell">
      <div className="header">
        <span className="brand-icon">{CAT_LOGO}</span>
        <div>
          <div className="brand-name">猫猫 <em>Token</em></div>
          <div className="brand-sub">一个 Key,喵定所有 AI · 一键启动</div>
        </div>
      </div>

      <div className="keycard">
        <div className="tabs">
          <button className={`tab ${tab === "login" ? "on" : ""}`} onClick={() => setTab("login")}>账号登录</button>
          <button className={`tab ${tab === "key" ? "on" : ""}`} onClick={() => setTab("key")}>直接填 Key</button>
        </div>

        {tab === "login" ? (
          account ? (
            <>
              <div className="logged">
                <span className="logged-user">✓ 已登录 {account.username}</span>
                <span className="logged-key">{account.key_masked}</span>
                <button className="btn-logout" onClick={doLogout}>退出登录</button>
              </div>
              <div className="keyhint">关闭本应用时配置会自动还原;重新打开将自动恢复接管</div>
            </>
          ) : (
          <>
            <div className="loginrow">
              <input placeholder="用户名 / 邮箱" value={username} spellCheck={false}
                onChange={(e) => setUsername(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && login()} />
              <input type="password" placeholder="密码" value={password}
                onChange={(e) => setPassword(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && login()} />
              <button className="btn btn-save" disabled={busy} onClick={login}>
                {busy ? "登录中…" : "登录"}
              </button>
            </div>
            <div className="keyhint">
              登录后自动创建专属 API Key 并配置全部工具{anyConfigured ? "(已有配置会被更新)" : ""}
            </div>
          </>
          )
        ) : (
          <>
            <div className="keyrow">
              <input type="password" placeholder="sk-..." value={key} spellCheck={false}
                onChange={(e) => setKey(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && save()} />
              <button className="btn btn-save" onClick={save}>保存</button>
            </div>
            <div className="keyhint">已有 Key?粘贴保存即可</div>
          </>
        )}

        <div className="quicklinks">
          <a onClick={() => { setPanelSrc(PANEL_HOME); setPanelOpen(true); }}>充值 · 控制台</a>
          <span>·</span>
          <a onClick={() => openUrl("https://binaryworld.world/register")}>注册账号 ↗</a>
        </div>
      </div>

      <div className="listwrap">
        <div className="group-title">桌面应用</div>
        <div className="tools">{desktop.map(row)}</div>
        <div className="group-title">命令行工具</div>
        <div className="tools">{cli.map(row)}</div>
      </div>

      <div className="footer">
        <span>binaryworld.world</span>
        <span>猫猫 Token · Made with meow ♡</span>
      </div>

      {panelOpen && (
        <div className="modal-mask" onClick={() => setPanelOpen(false)}>
          <div className="panel-modal" onClick={(e) => e.stopPropagation()}>
            <div className="panel-modal-head">
              {panelSrc !== PANEL_HOME && (
                <button className="panel-modal-back" onClick={() => setPanelSrc(PANEL_HOME)}>← 返回控制台</button>
              )}
              <span className="panel-modal-title">🐾 {panelSrc === PANEL_HOME ? "猫猫 Token · 控制台" : "收银台"}</span>
              <button className="panel-modal-close" onClick={() => setPanelOpen(false)} aria-label="关闭">✕</button>
            </div>
            <iframe
              className="panel-modal-frame"
              src={panelSrc}
              title="猫猫 Token 控制台"
            />
          </div>
        </div>
      )}

      <div className={`toast ${toast ? "show" : ""}`} role="status">{toast}</div>
    </div>
  );
}
