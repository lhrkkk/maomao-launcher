import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
interface RecentCall { created_at: number; model_name: string; prompt_tokens: number; completion_tokens: number; usd: number }
interface InviteInfo { aff_code: string; aff_count: number; aff_usd: number }
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


// 邀请卡:默认折叠,展示邀请码 + 已邀人数 + 待结提成(可转入余额)
function InviteCard({ say, onClaimed, ak }: { say: (t: string) => void; onClaimed: () => Promise<void>; ak?: string }) {
  const [info, setInfo] = useState<InviteInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const lastLoad = useRef(0);
  const load = async () => {
    if (!ak) return;
    if (Date.now() - lastLoad.current < 60_000) return;
    lastLoad.current = Date.now();
    try { setInfo(await invoke<InviteInfo>("invite_info")); }
    catch { setInfo(null); }
  };
  // 挂载与换账号时都拉(节流归零,新账号立即出新数据)
  useEffect(() => { lastLoad.current = 0; load(); }, [ak]);
  const claim = async () => {
    setBusy(true);
    try {
      await invoke("invite_claim");
      say("提成已转入余额 🐾");
      lastLoad.current = 0;
      await load();
      await onClaimed();
    } catch (e) { say(`${e}`); }
    setBusy(false);
  };
  return (
    <details className="rcfold" open onToggle={(e) => { if ((e.target as HTMLDetailsElement).open) load(); }}>
      <summary>邀请好友 <span className="rc-caret">▾</span></summary>
      {info === null ? <div className="keyhint">加载中…</div> : !info.aff_code ? <div className="keyhint">暂无邀请信息</div> : (
        <div className="invbox">
          <div className="invcode" onClick={() => {
            const link = `https://binaryworld.world/register?aff=${info.aff_code}`;
            navigator.clipboard?.writeText(link).then(
              () => say("邀请链接已复制 🐾"),
              () => say(link),
            );
          }} title="点击复制邀请链接">{info.aff_code}</div>
          <div className="keyhint">已邀请 <b>{info.aff_count}</b> 人 · 待结提成 <b>${info.aff_usd.toFixed(2)}</b><br />点击邀请码复制完整链接,好友注册消费提成累计为待结,满 $1 可转入余额</div>
          {info.aff_usd >= 1 && (
            <button className="btn btn-save" style={{ width: "100%", height: 32, marginTop: 8 }} disabled={busy} onClick={claim}>
              {busy ? "转入中…" : `转入余额($${info.aff_usd.toFixed(2)})`}
            </button>
          )}
        </div>
      )}
    </details>
  );
}

// 调用量图表(近 8 天):挂载即预取,展开秒出(60s 节流)
interface DailyPoint { day: string; calls: number; usd: number }
interface UsageData { daily: DailyPoint[]; today_calls: number; today_usd: number }
function UsageChart({ cny, ak }: { cny: number; ak?: string }) {
  const [data, setData] = useState<UsageData | null>(null);
  const lastLoad = useRef(0);
  const load = async () => {
    if (!ak) return;
    if (Date.now() - lastLoad.current < 60_000) return;
    lastLoad.current = Date.now();
    try { setData(await invoke<UsageData>("usage_daily")); }
    catch { setData(null); }
  };
  useEffect(() => { lastLoad.current = 0; load(); }, [ak]);
  const chart = (pick: (d: DailyPoint) => number, fmt: (n: number) => string) => {
    if (!data) return null;
    const vals = data.daily.map(pick);
    const mx = Math.max(...vals, 1);
    return (
      <div className="ucbars">
        {data.daily.map((d, i) => (
          <div key={i} className="ucbar" title={`${d.day}:${fmt(vals[i])}`}>
            <span className="ucval">{vals[i] ? fmt(vals[i]) : ""}</span>
            <i style={{ height: Math.max(2, Math.round(vals[i] / mx * 40)) }} />
            <span className="ucday">{d.day.slice(3)}</span>
          </div>
        ))}
      </div>
    );
  };
  return (
    <details className="rcfold" onToggle={(e) => { if ((e.target as HTMLDetailsElement).open) load(); }}>
      <summary>调用量{data && data.today_calls >= 0 ? <span className="uc-today">今日 {data.today_calls} 次 · ¥{(data.today_usd * cny).toFixed(2)}</span> : null}<span className="rc-caret">▾</span></summary>
      {data === null ? <div className="keyhint">加载中…</div> : data.daily.length === 0 ? <div className="keyhint">暂无数据</div> : (
        <div className="ucwrap">
          <div className="uclabel">调用次数</div>
          {chart((d) => d.calls, (n) => `${n}`)}
          <div className="uclabel" style={{ marginTop: 8 }}>花费</div>
          {chart((d) => d.usd, (n) => `¥${(n * cny).toFixed(n * cny < 10 ? 1 : 0)}`)}
        </div>
      )}
    </details>
  );
}

// 最近调用:默认折叠,展开时拉取(60s 节流)
function RecentCalls({ cny, ak }: { cny: number; ak?: string }) {
  const [rows, setRows] = useState<RecentCall[] | null>(null);
  const lastLoad = useRef(0);
  const load = async () => {
    if (!ak) return;
    if (Date.now() - lastLoad.current < 60_000) return;
    lastLoad.current = Date.now();
    try { setRows(await invoke<RecentCall[]>("recent_calls")); }
    catch { setRows([]); }
  };
  useEffect(() => { lastLoad.current = 0; load(); }, [ak]);
  return (
    <details className="rcfold" open onToggle={(e) => { if ((e.target as HTMLDetailsElement).open) load(); }}>
      <summary>最近调用 <span className="rc-caret">▾</span></summary>
      {rows === null ? <div className="keyhint">加载中…</div>
        : rows.length === 0 ? <div className="keyhint">还没有调用记录</div>
        : <div className="rclist">
            {rows.map((r, i) => {
              const dt = new Date(r.created_at * 1000);
              const t = `${dt.getMonth() + 1}-${dt.getDate()} ${String(dt.getHours()).padStart(2, "0")}:${String(dt.getMinutes()).padStart(2, "0")}`;
              return (
                <div key={i} className="rcrow">
                  <span className="rc-time">{t}</span>
                  <span className="rc-model" title={r.model_name}>{r.model_name || "—"}</span>
                  <span className="rc-tok">{r.prompt_tokens}/{r.completion_tokens}</span>
                  <span className="rc-usd">¥{(r.usd * cny).toFixed(3)}</span>
                </div>
              );
            })}
          </div>}
    </details>
  );
}

// 密钥管理面板:列出账号下所有 API Key，创建新 Key、切换当前使用的 Key
function KeysPanel({ account, onUsed, say }: {
  account: LoginInfo | null;
  onUsed: () => Promise<void>;
  say: (t: string) => void;
}) {
  const [tokens, setTokens] = useState<TokenEntry[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const lastLoad = useRef(0);

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    document.addEventListener("click", close);
    return () => document.removeEventListener("click", close);
  }, [open]);

  const load = async () => {
    setBusy(true);
    try { setTokens(await invoke<TokenEntry[]>("list_tokens")); lastLoad.current = Date.now(); }
    catch (e) {
      const msg = `${e}`;
      say(msg);
      setTokens([]);
      if (msg.includes("重新登录") || msg.includes("Key 无效")) {
        say("密钥已失效,已打开浏览器重新授权…");
        openUrl("https://binaryworld.world/launcher-auth");
      }
    }
    setBusy(false);
  };
  useEffect(() => { if (account?.can_manage) load(); }, []);

  if (!account?.can_manage) {
    return <div className="keyhint">密钥管理需要账号登录，请先在「账号登录」页登录，重新登录后即可管理密钥</div>;
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
            <div className={`ksel ${open ? "open" : ""}`} onClick={(e) => { e.stopPropagation(); if (!open && Date.now() - lastLoad.current > 60_000) load(); setOpen(!open); }}>
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
    </div>
  );
}

export default function App() {
  const [pendingAuth, setPendingAuth] = useState<PendingAuth | null>(null);
  const [upd, setUpd] = useState<Update | null>(null);
  const [bal, setBal] = useState<{ quota_usd: number; used_usd: number } | null>(null);
  const [cny, setCny] = useState(7.2);
  const rmb = (usd: number) => "¥" + (usd * cny).toFixed(2);
  useEffect(() => {
    fetch("https://binaryworld.world/mm-admin/rate")
      .then((r) => r.json())
      .then((d) => { if (d?.data?.value) setCny(d.data.value); })
      .catch(() => {});
  }, []);
  const [tk, setTk] = useState(true);
  const [view, setView] = useState<"home" | "invite">("home");
  const [updPct, setUpdPct] = useState(-1);
  const [busy, setBusy] = useState(false);
  const [tools, setTools] = useState<Tool[]>([]);
  const [account, setAccount] = useState<LoginInfo | null>(null);
  // 邀请码终身不变:登录后预取一次,缓存永久有效(登出才清)
  const topCopyCache = useRef<string | null>(null);
  // 邀请码注册时生成、终身不变:登录后预取,点右上角复制零等待
  useEffect(() => {
    if (!account?.key) { topCopyCache.current = null; return; }
    invoke<InviteInfo>("invite_info")
      .then((info) => { if (info.aff_code) topCopyCache.current = info.aff_code; })
      .catch(() => {});
  }, [account?.key]);
  const [ready, setReady] = useState(false);
  const PANEL_HOME = "https://binaryworld.world/panel";
  const [toast, setToast] = useState("");
  const toastTimer = useRef<number | undefined>(undefined);

  const refresh = () =>
    invoke<{ tools: Tool[]; login: LoginInfo | null; takeover: boolean }>("get_state")
      .then((s) => { setTools(s.tools); setAccount(s.login); setTk(s.takeover !== false); loadBalance(); })
      .catch(() => {});
  useEffect(() => {
    (async () => {
      try {
        const st = await invoke<{ tools: Tool[]; login: LoginInfo | null; takeover: boolean }>("get_state");
        setTools(st.tools);
        setAccount(st.login);
        setTk(st.takeover !== false);
        if (st.login?.key && st.takeover !== false) {
          await invoke("save_key", { key: st.login.key }); // 重新接管
          const st2 = await invoke<{ tools: Tool[]; login: LoginInfo | null; takeover: boolean }>("get_state");
          setTools(st2.tools);
        }
      } catch {}
      setReady(true);
    })();
    // 深链授权:先弹确认(任意网页都能发 maomao:// 链接),用户放行才写入配置
    const unsubs: Array<() => void> = [];
    (async () => {
      try {
        unsubs.push(await listen<PendingAuth | null>("mm-auth-request", (e) => {
          if (e.payload?.key) setPendingAuth(e.payload);
        }));
        unsubs.push(await listen<string>("mm-auth-error", (e) => say(e.payload)));
        // 后台静默安装/卸载结果(Rust 线程跑完 emit)
        unsubs.push(await listen<{ id: string; ok: boolean; tail: string; verb?: string }>("mm-install", (e) => {
          const verb = e.payload.verb || "安装/升级";
          setInstalling((s) => { const n = new Set(s); n.delete(e.payload.id); return n; });
          if (e.payload.ok) {
            say(`${verb}完成 🐾`);
            refresh();
            // 升级后重查版本,否则「升级」按钮因旧比对结果一直显示
            invoke<[string | null, string | null] | null>("tool_versions", { id: e.payload.id })
              .then((v) => {
                if (v) setVerInfo((s) => ({ ...s, [e.payload.id]: { cur: v[0] ?? undefined, latest: v[1] ?? undefined } }));
              })
              .catch(() => {});
          }
          else say(`后台${verb}失败:${e.payload.tail.slice(-160) || "未知错误"}`);
        }));
      } catch {}
    })();
    // 终端装完工具切回本窗口时,状态自动刷新(3 秒节流)
    let lastFocus = 0;
    const unFocus = getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (!focused) { setMenuPos(null); return; } // 失焦收起 ⋯ 菜单
      const now = Date.now();
      if (now - lastFocus < 3000) return;
      lastFocus = now;
      refresh();
    });
    unsubs.push(() => { unFocus.then((f) => f()); });
    return () => { unsubs.forEach((u) => u()); };
  }, []);

  const say = (t: string) => {
    setToast(t);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(""), 2600);
  };

  const toggleTk = async () => {
    const next = !tk;
    setTk(next);
    try {
      await invoke("takeover", { on: next });
      say(next ? "已接管,全部客户端已配置 🐾" : "已还原,本机配置恢复原样");
      await refresh();
    } catch (e) { setTk(!next); say(`${e}`); }
  };

  const lastBal = useRef(0);
  const loadBalance = (force = false) => {
    if (!force && Date.now() - lastBal.current < 60_000) return;
    lastBal.current = Date.now();
    invoke<{ quota_usd: number; used_usd: number }>("get_balance")
      .then(setBal)
      .catch(() => setBal(null));
  };
  useEffect(() => {
    const onFocus = () => loadBalance();
    window.addEventListener("focus", onFocus);
    const timer = window.setInterval(() => loadBalance(), 5 * 60_000);
    return () => { window.removeEventListener("focus", onFocus); window.clearInterval(timer); };
  }, []);
  useEffect(() => { if (account?.can_manage) loadBalance(true); else setBal(null); }, [account?.can_manage]);

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

  const launch = async (t: Tool) => {
    if (t.configurable && !t.configured) { say("请先登录或保存 API Key"); return; }
    try {
      await invoke("launch", { id: t.id });
      say(`正在启动 ${t.name}…`);
    } catch (e) { say(`${e}`); }
  };

  const cli = tools.filter((t) => t.kind === "Cli");
  const desktop = tools.filter((t) => t.kind === "Desktop");

  const INSTALLABLE = new Set(["claude", "codex", "gemini", "opencode", "grok", "openclaw", "hermes", "pi", "claude-desktop", "codex-desktop"]);
  // ⋯ 菜单对全部已安装工具可用(桌面版卸载=定位/Finder/系统设置)
  const UNINSTALLABLE = new Set(["claude", "codex", "gemini", "opencode", "grok", "openclaw", "hermes", "pi", "claude-desktop", "codex-desktop"]);
  const [menuPos, setMenuPos] = useState<{ id: string; top: number; left: number } | null>(null);
  const [installing, setInstalling] = useState<Set<string>>(new Set());
  // 工具版本信息(启动后懒检查):{cur, latest};可升级的行内直接亮"升级"按钮
  const [verInfo, setVerInfo] = useState<Record<string, { cur?: string; latest?: string }>>({});
  const verChecked = useRef(false);
  useEffect(() => {
    if (verChecked.current) return;
    const installed = tools.filter((t) => t.installed && t.kind === "Cli" && t.id !== "grok" && t.id !== "hermes");
    if (!installed.length) return;
    verChecked.current = true;
    installed.forEach((t) => {
      invoke<[string | null, string | null] | null>("tool_versions", { id: t.id })
        .then((v) => {
          if (v) setVerInfo((s) => ({ ...s, [t.id]: { cur: v[0] ?? undefined, latest: v[1] ?? undefined } }));
        })
        .catch(() => {});
    });
  }, [tools]);
  const upgradable = (t: Tool) => {
    if (t.id === "grok" || t.id === "hermes") return true; // 无版本渠道,行内常显(重跑官方脚本幂等)
    const v = verInfo[t.id];
    return !!(v?.cur && v?.latest && v.cur !== v.latest);
  };
  const doUninstall = async (t: Tool) => {
    setMenuPos(null);
    try { say(await invoke<string>("uninstall_tool", { id: t.id })); await refresh(); }
    catch (e) { say(`${e}`); }
    pollTool(t, false, "已卸载");
  };
  const install = async (t: Tool) => {
    setInstalling((s) => new Set(s).add(t.id));
    try { say(await invoke<string>("install_tool", { id: t.id })); await refresh(); }
    catch (e) { say(`${e}`); setInstalling((s) => { const n = new Set(s); n.delete(t.id); return n; }); }
    pollTool(t, true, "安装完成");
  };
  const upgrade = async (t: Tool) => {
    setMenuPos(null);
    setInstalling((s) => new Set(s).add(t.id));
    try { say(await invoke<string>("install_tool", { id: t.id, upgrade: true })); }
    catch (e) { say(`${e}`); setInstalling((s) => { const n = new Set(s); n.delete(t.id); return n; }); }
  };
  // 终端里的安装/卸载要跑一阵:轮询 get_state,状态翻转即提示(上限 15 分钟)
  const pollTool = (t: Tool, wantInstalled: boolean, label: string) => {
    const started = Date.now();
    const poll = setInterval(async () => {
      try {
        const s = await invoke<{ tools: Tool[] }>("get_state");
        setTools(s.tools ?? []);
        const cur = (s.tools ?? []).find((x) => x.id === t.id);
        if (!cur || cur.installed === wantInstalled) { clearInterval(poll); say(`${t.name} ${label}`); }
        else if (Date.now() - started > 15 * 60_000) clearInterval(poll);
      } catch {}
    }, 8000);
  };
  const row = (t: Tool) => (
    <div
      key={t.id}
      className="tool-row"
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
      {t.installed ? (
        <span className="tool-acts">
          {upgradable(t) && (
            installing.has(t.id)
              ? <span className="tool-state st-install busy">升级中…</span>
              : <span className="tool-state st-install" onClick={() => upgrade(t)}>升级</span>
          )}
          <span className={`tool-state st-ok`} onClick={() => launch(t)}>启动 ↗</span>
          {UNINSTALLABLE.has(t.id) && (
            <span className="tool-dots" onClick={(e) => {
              e.stopPropagation();
              if (menuPos?.id === t.id) { setMenuPos(null); return; }
              const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
              setMenuPos({ id: t.id, top: r.bottom + 5, left: Math.max(8, r.right - 96) });
            }}>⋯</span>
          )}
        </span>
      ) : INSTALLABLE.has(t.id) ? (
        installing.has(t.id)
          ? <span className="tool-state st-install busy">安装中…</span>
          : <span className="tool-state st-install" onClick={() => install(t)}>安装</span>
      ) : (
        <span className={`tool-state st-warn`}>未安装</span>
      )}
    </div>
  );

  const menuTool = menuPos ? tools.find((x) => x.id === menuPos.id) : null;

  return (
    <div className="shell">
      <div className="header">
        <span className="brand-icon">{CAT_LOGO}</span>
        <div>
          <div className="brand-name">猫猫 <em>动力</em></div>
          <div className="brand-sub">一个 Key,喵定所有 AI · 一键启动</div>
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
        {!ready ? (
          <div className="loadrow"><div className="spin" /></div>
        ) : account ? (
          <>
            <div className="logged">
              <span className="logged-user">{account.username === "API_KEY" ? "✓ 已配置 API Key" : `✓ 已登录 ${account.username}`}</span>
              <span className="logged-key">{account.key_masked}</span>
              <button className="btn-logout" onClick={doLogout}>退出并还原</button>
            </div>
            <div className="statgrid">
              <div className={`statcard balcard ${bal && bal.quota_usd * cny < 7 ? "low" : ""}`}>
                <span className="lbl">余额</span>
                {bal
                  ? <span className="val"><b>{rmb(bal.quota_usd)}</b><i className="bal-usd">${bal.quota_usd.toFixed(2)}</i>
                      {bal.quota_usd * cny < 7 && <a onClick={() => openUrl("https://binaryworld.world/panel")}>⚠ 充值</a>}</span>
                  : <span className="val">—</span>}
              </div>
              <div className="statcard tkcard">
                <span className="lbl"><span className={`tk-dot ${tk ? "" : "off"}`} />{tk ? "接管中" : "已还原"}</span>
                <label className="sw">
                  <input type="checkbox" checked={tk} onChange={toggleTk} /><i />
                </label>
              </div>
            </div>
            <KeysPanel account={account} onUsed={refresh} say={say} />

          </>
        ) : (
          <button
            className="btn btn-hero"
            onClick={() => openUrl("https://binaryworld.world/launcher-auth")}
          >
            <span>一键登录 · 自动配置全部工具</span>
          </button>
        )}

        <div className="quicklinks">
          <span className="ql-left">
            <a onClick={() => openUrl("https://binaryworld.world/panel")}>充值 · 管理 ↗</a>
            <span>·</span>
            <a onClick={() => openUrl("https://binaryworld.world/register")}>注册账号 ↗</a>
          </span>
          {account && <span className="ql-hint">退出应用自动还原</span>}
        </div>
      </div>

      <button className="topcopy" onClick={async () => {
        const copy = (code: string) => {
          const link = `https://binaryworld.world/register?aff=${code}`;
          navigator.clipboard?.writeText(link).then(() => say("邀请链接已复制 🐾"), () => say(link));
        };
        if (topCopyCache.current) { copy(topCopyCache.current); return; }
        try {
          const info = await invoke<InviteInfo>("invite_info");
          if (!info.aff_code) { say("暂无邀请码,请先登录"); return; }
          topCopyCache.current = info.aff_code;
          copy(info.aff_code);
        } catch { say("请先登录后再复制邀请链接"); }
      }}>⧉</button>

      <div className="viewtabs">
        <button className={`tab ${view === "home" ? "on" : ""}`} onClick={() => { setView("home"); refresh(); }}>启动</button>
        <button className={`tab ${view === "invite" ? "on" : ""}`} onClick={() => setView("invite")}>更多</button>
      </div>

      {/* 两视图常驻(非活动只隐藏):「更多」页的邀请/最近调用从启动就后台预取,切换零等待 */}
      <div className="listwrap" hidden={view !== "home"}>
        <div className="group-title">桌面应用</div>
        <div className="tools">{desktop.map(row)}</div>
        <div className="group-title">命令行工具</div>
        <div className="tools">{cli.map(row)}</div>
      </div>

      <div className="listwrap" hidden={view !== "invite"}>
        <InviteCard say={say} onClaimed={refresh} ak={account?.key} />
        <UsageChart cny={cny} ak={account?.key} />
        <RecentCalls cny={cny} ak={account?.key} />
      </div>

      <div className="footer">
        <span>binaryworld.world · <a onClick={() => checkUpdate(false)}>检查更新</a></span>
        <span>猫猫动力 · Made with meow ♡</span>
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

      {/* ⋯ 菜单:fixed 顶层渲染,不受滚动容器裁剪,z-index 拉满 */}
      {menuTool && menuPos && (
        <>
          <div className="menu-backdrop" onClick={() => setMenuPos(null)} />
          <div className="tool-menu-pop" style={{ top: menuPos.top, left: menuPos.left }}>
            <span className="tool-menu-item danger" onClick={() => doUninstall(menuTool)}>卸载 {menuTool.name}</span>
          </div>
        </>
      )}
    </div>
  );
}
