import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

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
interface LoginInfo { username: string; key_masked: string; key?: string; can_manage?: boolean; token_id?: number | null; access_token?: string }
interface TokenEntry { id: number; name: string; key_masked: string; status: number; model_limits_enabled: boolean; model_limits: string }
interface PendingAuth { key: string; user: string; at: string; id: number | null; mask: string }

// 模型标签:无限制 = 全部;有限制 = 前 3 个 + 剩余数
function ModelTags({ t }: { t: TokenEntry }) {
  const list = t.model_limits_enabled
    ? t.model_limits.split(",").map((s) => s.trim()).filter(Boolean)
    : [];
  return (
    <div className="krow-models" title={t.model_limits_enabled ? list.join("、") : "该密钥可调用全部模型"}>
      {list.length === 0
        ? <span className="mtag all">全部模型</span>
        : <>
            {list.slice(0, 3).map((m) => <span key={m} className="mtag">{m}</span>)}
            {list.length > 3 && <span className="mtag more">+{list.length - 3}</span>}
          </>}
    </div>
  );
}

const openUrl = (url: string) => invoke("open_url", { url }).catch(() => {});

// 密钥管理面板:列出账号下所有 API Key，创建新 Key、切换当前使用的 Key
function KeysPanel({ account, onUsed, onManage, say }: {
  account: LoginInfo | null;
  onUsed: () => Promise<void>;
  onManage: () => void;
  say: (t: string) => void;
}) {
  const [tokens, setTokens] = useState<TokenEntry[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    document.addEventListener("click", close);
    return () => document.removeEventListener("click", close);
  }, [open]);

  const load = async () => {
    setBusy(true);
    try { setTokens(await invoke<TokenEntry[]>("list_tokens")); }
    catch (e) { say(`${e}`); setTokens([]); }
    setBusy(false);
  };
  useEffect(() => { if (account?.can_manage) load(); }, []);

  if (!account?.can_manage) {
    return <div className="keyhint">密钥管理需要账号登录，请先在「账号登录」页登录，或直接在「直接填 Key」页粘贴现有 Key</div>;
  }
  const use = async (id: number, name: string) => {
    setBusy(true);
    try { const m = await invoke<string>("use_token", { id }); say(`已切换到「${name}」${m} 🐾`); await onUsed(); }
    catch (e) { say(`${e}`); }
    setBusy(false);
  };
  return (
    <div className="keysbox">
      {tokens === null ? <div className="keyhint">加载中…</div>
        : tokens.length === 0 ? <div className="keyhint">还没有密钥，点「在网站上管理」创建一个 🐾</div>
        : (() => {
          const cur = tokens.find((t) => t.id === account.token_id) || tokens[0];
          return (
            <div className={`ksel ${open ? "open" : ""}`} onClick={(e) => { e.stopPropagation(); setOpen(!open); }}>
              <div className="ksel-head">
                <span className="krow-name">{cur.name}{cur.id === account.token_id ? " · 使用中" : ""}</span>
                <ModelTags t={cur} />
                <span className="ksel-caret" aria-hidden="true">⌄</span>
              </div>
              {open && (
                <div className="ksel-list" onClick={(e) => e.stopPropagation()}>
                  {tokens.map((t) => (
                    <div key={t.id} className={`ksel-item ${t.id === account.token_id ? "cur" : ""}`}
                      onClick={() => { setOpen(false); if (t.id !== account.token_id) use(t.id, t.name); }}>
                      <span className="krow-name">{t.id === account.token_id ? "✓ " : ""}{t.name}</span>
                      <span className="krow-key">{t.key_masked}</span>
                      <ModelTags t={t} />
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        })()}
      <div className="keysops">
        <button className="btn ghost-btn" onClick={load} disabled={busy}>刷新</button>
        <button className="btn ghost-btn" onClick={onManage}>在网站上管理 ↗</button>
      </div>
    </div>
  );
}

export default function App() {
  const [tab, setTab] = useState<"login" | "keys" | "key">("login");
  const [pendingAuth, setPendingAuth] = useState<PendingAuth | null>(null);
  const [upd, setUpd] = useState<Update | null>(null);
  const [bal, setBal] = useState<{ quota_usd: number; used_usd: number } | null>(null);
  const [updPct, setUpdPct] = useState(-1);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [tools, setTools] = useState<Tool[]>([]);
  const [account, setAccount] = useState<LoginInfo | null>(null);
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
    // 深链授权:先弹确认(任意网页都能发 maomao:// 链接),用户放行才写入配置
    const unsubs: Array<() => void> = [];
    (async () => {
      try {
        unsubs.push(await listen<PendingAuth | null>("mm-auth-request", (e) => {
          if (e.payload?.key) setPendingAuth(e.payload);
        }));
        unsubs.push(await listen<string>("mm-auth-error", (e) => say(e.payload)));
      } catch {}
    })();
    return () => { unsubs.forEach((u) => u()); };
  }, []);

  const say = (t: string) => {
    setToast(t);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(""), 2600);
  };

  const loadBalance = () =>
    invoke<{ quota_usd: number; used_usd: number }>("get_balance")
      .then(setBal)
      .catch(() => setBal(null));
  useEffect(() => { if (account?.can_manage) loadBalance(); else setBal(null); }, [account?.can_manage]);

  const checkUpdate = async (silent = true) => {
    try {
      const u = await check();
      if (u) setUpd(u);
      else if (!silent) say("已是最新版本 🐾");
    } catch { if (!silent) say("检查更新失败,请稍后再试"); }
  };
  useEffect(() => { checkUpdate(); }, []);
  const applyUpdate = async () => {
    if (!upd) return;
    setUpdPct(0);
    try {
      await upd.downloadAndInstall((ev) => {
        if (ev.event === "Started" && ev.data.contentLength) setUpdPct(1);
        else if (ev.event === "Progress") setUpdPct((p) => Math.max(p, 1));
        else if (ev.event === "Finished") setUpdPct(100);
      });
      await relaunch();
    } catch (e) { setUpdPct(-1); say(`更新失败:${e}`); }
  };

  const login = async () => {
    if (!username.trim() || !password) { say("请输入账号和密码，喵"); return; }
    setBusy(true);
    try {
      const mask = await invoke<string>("login_and_setup", { username, password, turnstile: "" });
      setUsername(""); setPassword("");
      say(`登录成功，已创建并配置密钥 ${mask} 🐾`);
      await refresh();
    } catch (e) { say(`${e}`); }
    setBusy(false);
  };

  const confirmAuth = async () => {
    const a = pendingAuth;
    if (!a) return;
    setPendingAuth(null);
    setBusy(true);
    try {
      const mask = await invoke<string>("deep_link_setup", { key: a.key, username: a.user, at: a.at || null, id: a.id ?? null });
      say(`授权成功，已配置密钥 ${mask} 🐾`);
      await refresh();
    } catch (e) { say(`${e}`); }
    setBusy(false);
  };

  const doLogout = async () => {
    try {
      await invoke("logout");
      setAccount(null);
      say("已退出，本机配置已还原");
      await refresh();
    } catch (e) { say(`${e}`); }
  };

  const save = async () => {
    const k = key.trim();
    if (!k) { say("请先填入 API Key，喵"); return; }
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
          <div className="brand-sub">一个 Key，喵定所有 AI · 一键启动</div>
          {upd && (
            <div className="updrow">
              <span className="upd-text">🎉 新版本 v{upd.version}</span>
              {updPct < 0
                ? <button className="btn krow-use" onClick={applyUpdate}>立即更新</button>
                : <span className="upd-pct">{updPct >= 100 ? "安装中…" : `下载中 ${updPct}%`}</span>}
            </div>
          )}
        </div>
      </div>

      <div className="keycard">
        <div className="tabs">
          <button className={`tab ${tab === "login" ? "on" : ""}`} onClick={() => setTab("login")}>账号登录</button>
          <button className={`tab ${tab === "keys" ? "on" : ""}`} onClick={() => setTab("keys")}>密钥</button>
          <button className={`tab ${tab === "key" ? "on" : ""}`} onClick={() => setTab("key")}>直接填 Key</button>
        </div>

        {tab === "keys" ? (
          <>
            {account && (
              <div className="logged">
                <span className="logged-user">{account.username === "API_KEY" ? "✓ 已配置 API Key" : `✓ 已登录 ${account.username}`}</span>
                <span className="logged-key">{account.key_masked}</span>
                <button className="btn-logout" onClick={doLogout}>退出并还原</button>
              </div>
              {bal && (
                <div className={`balrow ${bal.quota_usd < 1 ? "low" : ""}`}>
                  <span>余额 <b>${bal.quota_usd.toFixed(2)}</b></span>
                  {bal.quota_usd < 1
                    ? <a onClick={() => openUrl("https://binaryworld.world/panel")}>⚠ 余额不足,去充值 ↗</a>
                    : <a onClick={() => openUrl("https://binaryworld.world/panel")}>充值 ↗</a>}
                </div>
              )}
            )}
            <KeysPanel account={account} onUsed={refresh} onManage={() => openUrl("https://binaryworld.world/panel")} say={say} />
          </>
        ) : tab === "login" ? (
          account ? (
            <>
              <div className="logged">
                <span className="logged-user">{account.username === "API_KEY" ? "✓ 已配置 API Key" : `✓ 已登录 ${account.username}`}</span>
                <span className="logged-key">{account.key_masked}</span>
                <button className="btn-logout" onClick={doLogout}>退出并还原</button>
              </div>
              {bal && (
                <div className={`balrow ${bal.quota_usd < 1 ? "low" : ""}`}>
                  <span>余额 <b>${bal.quota_usd.toFixed(2)}</b></span>
                  {bal.quota_usd < 1
                    ? <a onClick={() => openUrl("https://binaryworld.world/panel")}>⚠ 余额不足,去充值 ↗</a>
                    : <a onClick={() => openUrl("https://binaryworld.world/panel")}>充值 ↗</a>}
                </div>
              )}
              <div className="keyhint">关闭本应用时配置会自动还原;重新打开将自动恢复接管;切换密钥请到「密钥」页</div>
            </>
          ) : (
          <>
            <button
              className="btn btn-save"
              style={{ width: "100%", marginBottom: 12, padding: "14px 16px", fontSize: 14 }}
              onClick={() => openUrl("https://binaryworld.world/launcher-auth")}
            >
              浏览器一键登录 ↗
            </button>
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

        {tab !== "keys" && (
          <div className="quicklinks">
            <a onClick={() => openUrl("https://binaryworld.world/panel")}>充值 · 控制台 ↗</a>
            <span>·</span>
            <a onClick={() => openUrl("https://binaryworld.world/register")}>注册账号 ↗</a>
          </div>
        )}
      </div>

      <div className="listwrap">
        <div className="group-title">桌面应用</div>
        <div className="tools">{desktop.map(row)}</div>
        <div className="group-title">命令行工具</div>
        <div className="tools">{cli.map(row)}</div>
      </div>

      <div className="footer">
        <span>binaryworld.world · <a onClick={() => checkUpdate(false)}>检查更新</a></span>
        <span>猫猫 Token · Made with meow ♡</span>
      </div>

      {pendingAuth && (
        <div className="authmask" onClick={() => setPendingAuth(null)}>
          <div className="authcard" onClick={(e) => e.stopPropagation()}>
            <div className="authcard-title">🐾 网页授权请求</div>
            <div className="authcard-body">
              <b>{pendingAuth.user || "未知用户"}</b> 请求授权本机接管<br />
              密钥 <code>{pendingAuth.mask}</code><br />
              <span className="authcard-note">确认后写入全部客户端配置；非本人操作请点取消</span>
            </div>
            <div className="authcard-acts">
              <button className="btn ghost-btn" onClick={() => setPendingAuth(null)}>取消</button>
              <button className="btn btn-save" onClick={confirmAuth} disabled={busy}>确认接管</button>
            </div>
          </div>
        </div>
      )}

      <div className={`toast ${toast ? "show" : ""}`} role="status">{toast}</div>
    </div>
  );
}
