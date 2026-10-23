use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

const BASE_CLAUDE: &str = "https://binaryworld.world";
const BASE_OPENAI: &str = "https://binaryworld.world/v1";
const SITE: &str = "https://binaryworld.world";
const TOKEN_NAME: &str = "默认 Key";

#[cfg(not(target_os = "windows"))]
fn home() -> Result<PathBuf, String> {
    std::env::var("HOME").map(PathBuf::from).map_err(|_| "无法确定用户主目录".into())
}

#[cfg(target_os = "windows")]
fn home() -> Result<PathBuf, String> {
    std::env::var("USERPROFILE").map(PathBuf::from).map_err(|_| "无法确定用户主目录".into())
}

fn state_path() -> Result<PathBuf, String> {
    p(&[".config", "maomao-launcher", "state.json"])
}

fn backup_dir() -> Result<PathBuf, String> {
    p(&[".config", "maomao-launcher", "backups"])
}

/// 接管文件前备份原始版本(仅首次，保留用户最初状态)
fn takeover_backup(orig: &PathBuf) {
    let Ok(bdir) = backup_dir() else { return };
    let _ = fs::create_dir_all(&bdir);
    let tag = orig.components().rev().take(3).collect::<Vec<_>>()
        .iter().rev().map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>().join("_");
    let bak = bdir.join(tag);
    if !bak.exists() && orig.exists() {
        let _ = fs::copy(orig, &bak);
    }
}

/// 恢复原始版本;无备份时仅删除能确认属于本启动器的文件(绝不碰用户自己的东西)
fn restore_or_remove(orig: &PathBuf) {
    let Ok(bdir) = backup_dir() else { return };
    let tag = orig.components().rev().take(3).collect::<Vec<_>>()
        .iter().rev().map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>().join("_");
    let bak = bdir.join(tag);
    if bak.exists() {
        let _ = fs::copy(&bak, orig);
        return;
    }
    // 无备份:只处理带明确"我们写的"特征的文件
    let Ok(content) = fs::read_to_string(orig) else { return };
    let ours = content.contains("猫猫 Token Launcher") // 我们管理的 TOML 标记
        || (content.contains("binaryworld.world") && orig.file_name().map(|n| n == "openclaw.config.json").unwrap_or(false))
        || (content.trim().starts_with('{') && {
            // auth.json:整个 JSON 只有我们写的单个键时才算我们的
            read_json(orig)
                .and_then(|v| v.as_object().map(|o| o.len() == 1 && o.contains_key("OPENAI_API_KEY")))
                .unwrap_or(false)
        });
    if ours {
        let _ = fs::remove_file(orig);
    }
}

#[derive(Serialize, Clone)]
pub struct LoginInfo {
    pub username: String,
    pub key_masked: String,
    pub key: String,
    pub can_manage: bool,      // 有 access_token，可管理服务端密钥
    pub token_id: Option<i64>, // 当前 key 对应的服务端令牌 id(手填 key 为 None)
    pub access_token: String,  // 传给控制台弹窗同步登录态用
}
fn p(rel: &[&str]) -> Result<PathBuf, String> {
    let mut path = home()?;
    for seg in rel { path.push(seg); }
    Ok(path)
}

#[cfg(target_os = "windows")]
fn run_quiet(cmd: &mut Command) -> std::io::Result<std::process::Output> {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000).output() // CREATE_NO_WINDOW:GUI 进程 spawn cmd/powershell 不闪黑窗
}

#[cfg(target_os = "macos")]
fn which_in_login_shell(cmd: &str) -> bool {
    // GUI 进程 PATH 受限:login shell(.zprofile)→ 交互 login shell(.zshrc,mise/nvm 常在此激活)
    // → 常见安装目录兜底(mise shims、官方 curl 脚本、homebrew 等)
    let probe = |interactive: bool| {
        let mut args: Vec<String> = vec!["-l".into()];
        if interactive { args.push("-i".into()); }
        args.extend(["-c".into(), format!("command -v {cmd}")]);
        Command::new("/bin/zsh")
            .args(&args)
            .output()
            .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false)
    };
    if probe(false) || probe(true) { return true; }
    if let Ok(home) = std::env::var("HOME") {
        let h = PathBuf::from(home);
        let cands: [PathBuf; 10] = [
            h.join(".local/share/mise/shims"),
            h.join(".local/bin"),
            h.join(".opencode/bin"),
            h.join(".bun/bin"),
            h.join(".grok/bin"),
            h.join(".volta/bin"),
            h.join(".npm-global/bin"),
            h.join("Library/pnpm"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ];
        for d in cands {
            if d.join(cmd).exists() { return true; }
        }
    }
    false
}

#[cfg(target_os = "windows")]
fn which_in_login_shell(cmd: &str) -> bool {
    use std::env;
    // GUI 进程 PATH 缺用户目录:where -> 常见安装位置 -> 注册表用户 PATH，三重查找
    let via_where = run_quiet(Command::new("cmd").args(["/C", &format!("where {cmd}")]))
        .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);
    if via_where { return true; }
    let mut dirs: Vec<PathBuf> = vec![];
    if let Ok(up) = env::var("USERPROFILE") {
        let up = PathBuf::from(up);
        dirs.push(up.join(".local").join("bin"));
        dirs.push(up.join(".codex").join("bin"));
        dirs.push(up.join(".claude").join("local"));
        dirs.push(up.join("scoop").join("shims"));
    }
    if let Ok(appdata) = env::var("APPDATA") { dirs.push(PathBuf::from(appdata).join("npm")); }
    if let Ok(local) = env::var("LOCALAPPDATA") {
        let l = PathBuf::from(local);
        dirs.push(l.join("Microsoft").join("WindowsApps"));
        dirs.push(l.join("Programs"));
    }
    for scope in ["User", "Machine"] {
        if let Ok(out) = run_quiet(Command::new("powershell")
            .args(["-NoProfile", "-Command", &format!("[Environment]::GetEnvironmentVariable('PATH','{scope}')")]))
        {
            if out.status.success() {
                for seg in String::from_utf8_lossy(&out.stdout).split(';') {
                    let seg = seg.trim().trim_matches('"');
                    if !seg.is_empty() { dirs.push(PathBuf::from(seg)); }
                }
            }
        }
    }
    dirs.iter().any(|d| {
        [format!("{cmd}.exe"), format!("{cmd}.cmd"), format!("{cmd}.bat")]
            .iter().any(|f| d.join(f).exists())
    })
}

fn shellexpand(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Ok(h) = std::env::var("HOME") { return format!("{h}/{rest}"); }
    }
    s.to_string()
}

#[cfg(target_os = "macos")]
fn find_app(candidates: &[&str]) -> Option<String> {
    for name in candidates {
        for base in ["/Applications", "~/Applications"] {
            let mut path = PathBuf::from(shellexpand(base));
            path.push(format!("{name}.app"));
            if path.exists() { return Some(name.to_string()); }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn find_app(candidates: &[&str]) -> Option<String> {
    use std::env;
    // 返回可启动的完整 exe 路径:MSIX 别名 / Programs / Squirrel(Electron 默认布局,app-版本子目录)
    // / 官方安装器 / Program Files,最后开始菜单快捷方式兜底
    let mut probes: Vec<PathBuf> = vec![];
    if let Ok(local) = env::var("LOCALAPPDATA") {
        let l = PathBuf::from(&local);
        for name in candidates {
            probes.push(l.join("Microsoft").join("WindowsApps").join(format!("{name}.exe")));
            probes.push(l.join("Programs").join(name).join(format!("{name}.exe")));
            probes.push(l.join("Programs").join(format!("{name}.exe")));
            probes.push(l.join(name).join(format!("{name}.exe"))); // Squirrel 顶层
            if let Ok(rd) = fs::read_dir(l.join(name)) {
                let mut vers: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path())
                    .filter(|p| p.file_name()
                        .map(|f| f.to_string_lossy().to_lowercase().starts_with("app-"))
                        .unwrap_or(false))
                    .filter(|p| p.is_dir())
                    .collect();
                vers.sort(); // app-0.x 版本号排序,取最新
                if let Some(v) = vers.last() {
                    probes.push(v.join(format!("{name}.exe")));
                }
            }
        }
        if candidates.iter().any(|c| *c == "Claude") {
            probes.push(l.join("AnthropicClaude").join("claude.exe"));
        }
    }
    for root in ["C:\\Program Files", "C:\\Program Files (x86)"] {
        for name in candidates {
            probes.push(PathBuf::from(root).join(name).join(format!("{name}.exe")));
            probes.push(PathBuf::from(root).join(format!("{name}.exe")));
        }
    }
    if let Some(p) = probes.into_iter().find(|p| p.exists()) {
        return Some(p.to_string_lossy().to_string());
    }
    // MSIX/Store 应用(ChatGPT=Codex 桌面版等):无别名、无 .lnk、目录被 ACL 锁,Get-StartApps 是唯一可靠探测
    // 正则只含 candidates,防跨工具误匹配(曾把 Claude 匹配给 Codex 行)
    if let Ok(out) = run_quiet(Command::new("powershell").args(["-NoProfile", "-Command", &format!(
        "Get-StartApps | Where-Object {{ $_.Name -match '{}' }} | Select-Object -First 1 -ExpandProperty AppID",
        candidates.join("|")
    )])) {
        if out.status.success() {
            let appid = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !appid.is_empty() {
                return Some(format!("shell:AppsFolder\\{appid}"));
            }
        }
    }
    start_menu_shortcut(candidates)
}

/// 开始菜单快捷方式兜底:找到含应用名的 .lnk(安装器几乎都会建);start 命令可直接运行 .lnk
#[cfg(target_os = "windows")]
fn start_menu_shortcut(candidates: &[&str]) -> Option<String> {
    use std::env;
    use std::path::Path;
    let mut roots: Vec<PathBuf> = vec![];
    if let Ok(appdata) = env::var("APPDATA") {
        roots.push(PathBuf::from(appdata).join("Microsoft").join("Windows").join("Start Menu").join("Programs"));
    }
    roots.push(PathBuf::from("C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs"));
    fn scan(dir: &Path, candidates: &[&str]) -> Option<PathBuf> {
        let rd = fs::read_dir(dir).ok()?;
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                if let Some(hit) = scan(&p, candidates) { return Some(hit); }
            } else if let Some(fname) = p.file_name().map(|f| f.to_string_lossy().to_lowercase()) {
                if fname.ends_with(".lnk")
                    && candidates.iter().any(|c| fname.contains(&c.to_lowercase())) {
                    return Some(p);
                }
            }
        }
        None
    }
    for r in roots {
        if let Some(lnk) = scan(&r, candidates) {
            return Some(lnk.to_string_lossy().to_string());
        }
    }
    None
}

// ---------- 状态 ----------

#[derive(Serialize, Clone, Copy, PartialEq)]
pub enum ToolKind { Cli, Desktop }

#[derive(Serialize, Clone)]
pub struct ToolInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: ToolKind,
    pub installed: bool,
    pub configured: bool,
    pub configurable: bool,
}

fn tools_spec() -> Vec<(&'static str, &'static str, ToolKind, bool)> {
    use ToolKind::*;
    vec![
        ("claude", "Claude Code", Cli, true),
        ("codex", "Codex CLI", Cli, true),
        ("grok", "Grok Build", Cli, true),
        ("opencode", "OpenCode", Cli, true),
        ("openclaw", "OpenClaw", Cli, true),
        ("gemini", "Gemini CLI", Cli, false),
        ("hermes", "Hermes Agent", Cli, true),
        ("pi", "Pi", Cli, false),
        ("claude-desktop", "Claude 桌面版", Desktop, false),
        // Codex 桌面功能集成在 ChatGPT.app(也兼容独立 Codex.app)
        ("codex-desktop", "Codex 桌面版", Desktop, false),
    ]
}

fn check_configured(id: &str) -> bool {
    match id {
        "claude" => read_json(&p(&[".claude", "settings.json"]).unwrap_or_default())
            .and_then(|v| v.pointer("/env/ANTHROPIC_AUTH_TOKEN").and_then(|t| t.as_str()).map(|t| !t.is_empty()))
            .unwrap_or(false),
        "codex" => read_json(&p(&[".codex", "auth.json"]).unwrap_or_default())
            .and_then(|v| v.get("OPENAI_API_KEY").and_then(|t| t.as_str()).map(|t| !t.is_empty()))
            .unwrap_or(false),
        "grok" => fs::read_to_string(p(&[".grok", "config.toml"]).unwrap_or_default())
            .map(|s| s.contains("binaryworld.world")).unwrap_or(false),
        "opencode" => fs::read_to_string(p(&[".config", "opencode", "opencode.json"]).unwrap_or_default())
            .map(|s| s.contains("binaryworld.world")).unwrap_or(false),
        "openclaw" => fs::read_to_string(p(&[".openclaw", "openclaw.config.json"]).unwrap_or_default())
            .map(|s| s.contains("binaryworld.world")).unwrap_or(false),
        "hermes" => fs::read_to_string(p(&[".hermes", "config.yaml"]).unwrap_or_default())
            .map(|s| s.contains("binaryworld.world")).unwrap_or(false),
        _ => false,
    }
}

fn read_json(path: &PathBuf) -> Option<serde_json::Value> {
    fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok())
}

#[derive(Serialize)]
pub struct AppState {
    pub tools: Vec<ToolInfo>,
    pub login: Option<LoginInfo>,
    pub takeover: bool,
}

#[tauri::command]
async fn get_state() -> AppState {
    let login = read_json(&state_path().unwrap_or_default()).and_then(|v| {
        let u = v.get("username")?.as_str()?.to_string();
        let k = v.get("key_masked")?.as_str()?.to_string();
        let full = v.get("key").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let can_manage = !full.is_empty();
        let token_id = v.get("token_id").and_then(|x| x.as_i64());
        let at = can_manage.then(|| v.get("access_token").and_then(|x| x.as_str()).unwrap_or("").to_string()).unwrap_or_default();
        Some(LoginInfo { username: u, key_masked: k, key: full, can_manage, token_id, access_token: at })
    });
    AppState {
        tools: tools_spec()
        .into_iter()
        .map(|(id, name, kind, configurable)| {
            let installed = match kind {
                ToolKind::Cli => which_in_login_shell(id),
                ToolKind::Desktop => match id {
                    "claude-desktop" => find_app(&["Claude"]).is_some(),
                    "codex-desktop" => find_app(&["Codex", "ChatGPT"]).is_some(),
                    _ => false,
                },
            };
            ToolInfo { id, name, kind, installed, configured: check_configured(id), configurable }
        })
        .collect(),
        login,
        takeover: read_json(&state_path().unwrap_or_default())
            .and_then(|v| v.get("takeover").and_then(|x| x.as_bool()))
            .unwrap_or(true),
    }
}

// ---------- 保存配置 ----------

fn merge_write_json(path: PathBuf, mutate: impl FnOnce(&mut serde_json::Value)) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("创建目录失败:{e}"))?;
    }
    let mut v = read_json(&path).unwrap_or_else(|| serde_json::json!({}));
    mutate(&mut v);
    let out = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    fs::write(&path, out).map_err(|e| format!("写入 {} 失败:{e}", path.display()))
}

fn write_toml(path: PathBuf, content: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("创建目录失败:{e}"))?;
    }
    fs::write(&path, content).map_err(|e| format!("写入 {} 失败:{e}", path.display()))
}

/// 拉取站点公开模型列表(失败时回退内置清单)
// 各客户端模型集:默认内置,运营后台(/admin)可下发覆盖 —— GET /app-config.json
#[derive(Clone)]
struct AppCfg {
    codex: Vec<String>, codex_default: String,
    grok: Vec<String>, grok_default: String,
    full: Vec<String>, full_default: String,
    claude_main: String, claude_small: String,
    claude: Vec<String>,
}

fn default_app_config() -> AppCfg {
    AppCfg {
        codex: ["gpt-6.1-sol", "grok-latest", "glm-5.3", "MiniMax-M3"].iter().map(|s| s.to_string()).collect(),
        codex_default: "gpt-6.1-sol".into(),
        grok: ["grok-latest"].iter().map(|s| s.to_string()).collect(),
        grok_default: "grok-latest".into(),
        full: ["gpt-6.1-sol", "claude-opus-5-5", "grok-latest", "glm-5.3", "MiniMax-M3"].iter().map(|s| s.to_string()).collect(),
        full_default: "gpt-6.1-sol".into(),
        claude_main: "claude-opus-5-5".into(),
        claude: ["claude-opus-5-5", "gpt-6.1-sol", "grok-latest", "glm-5.3"].iter().map(|s| s.to_string()).collect(),
        claude_small: "glm-5.3".into(),
    }
}

fn cfg_strings(v: &serde_json::Value, key: &str, fallback: &[String]) -> Vec<String> {
    match v.get(key).and_then(|x| x.as_array()) {
        Some(arr) => {
            let list: Vec<String> = arr.iter().filter_map(|m| m.as_str()).map(|s| s.to_string()).filter(|s| !s.is_empty()).collect();
            if list.is_empty() { fallback.to_vec() } else { list }
        }
        None => fallback.to_vec(),
    }
}

fn cfg_string(v: &serde_json::Value, key: &str, fallback: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).filter(|s| !s.is_empty()).unwrap_or(fallback).to_string()
}

fn fetch_app_config() -> AppCfg {
    let mut c = default_app_config();
    let Ok(client) = http_client() else { return c };
    let Ok(resp) = client.get(format!("{SITE}/app-config.json")).send() else { return c };
    let Ok(v) = resp.json::<serde_json::Value>() else { return c };
    c.codex = cfg_strings(&v, "codex", &c.codex);
    c.codex_default = cfg_string(&v, "codex_default", &c.codex_default);
    c.grok = cfg_strings(&v, "grok", &c.grok);
    c.grok_default = cfg_string(&v, "grok_default", &c.grok_default);
    c.full = cfg_strings(&v, "full", &c.full);
    c.full_default = cfg_string(&v, "full_default", &c.full_default);
    c.claude_main = cfg_string(&v, "claude_main", &c.claude_main);
    c.claude = cfg_strings(&v, "claude", &c.claude);
    c.claude_small = cfg_string(&v, "claude_small", &c.claude_small);
    c
}

fn fetch_models() -> Vec<String> {
    const FALLBACK: &[&str] = &["glm-5.3", "glm-5.3-flash", "grok-4.6", "MiniMax-M3", "gpt-5.6-sol"];
    let client = match http_client() { Ok(c) => c, Err(_) => return FALLBACK.iter().map(|s| s.to_string()).collect() };
    let resp: Result<serde_json::Value, _> = client
        .get(format!("{SITE}/api/pricing"))
        .send()
        .and_then(|r| r.json());
    match resp {
        Ok(v) if api_success(&v) => {
            let list: Vec<String> = v.pointer("/data")
                .and_then(|d| d.as_array())
                .map(|arr| arr.iter()
                    .filter_map(|m| m.get("model_name").and_then(|n| n.as_str()))
                    .filter(|n| !n.is_empty())
                    .map(|n| n.to_string())
                    .collect())
                .unwrap_or_default();
            if list.is_empty() { FALLBACK.iter().map(|s| s.to_string()).collect() } else { list }
        }
        _ => FALLBACK.iter().map(|s| s.to_string()).collect(),
    }
}

fn apply_configs(key: &str) -> Result<(), String> {
    let acfg = fetch_app_config();
    // Claude Code:默认主力模型 + 后台小模型
    merge_write_json({ let pp = p(&[".claude", "settings.json"])?; takeover_backup(&pp); pp }, |v| {
        if !v.get("env").map(|e| e.is_object()).unwrap_or(false) {
            v["env"] = serde_json::json!({});
        }
        v["env"]["ANTHROPIC_BASE_URL"] = serde_json::json!(BASE_CLAUDE);
        v["env"]["ANTHROPIC_AUTH_TOKEN"] = serde_json::json!(key);
        v["env"]["ANTHROPIC_MODEL"] = serde_json::json!(&acfg.claude_main);
        v["env"]["ANTHROPIC_SMALL_FAST_MODEL"] = serde_json::json!(&acfg.claude_small);
    })?;

    // Codex:auth.json 整文件覆盖(纯 API Key 模式，清除 ChatGPT OAuth 残留)
    // 残留 auth_mode=chatgpt + tokens 会让 Codex 优先用 OAuth 令牌导致 401
    let codex_auth = { let pp = p(&[".codex", "auth.json"])?; takeover_backup(&pp); pp };
    if let Some(dir) = codex_auth.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("创建 ~/.codex 失败:{e}"))?;
    }
    fs::write(&codex_auth, serde_json::to_string_pretty(&serde_json::json!({
        "OPENAI_API_KEY": key
    })).unwrap_or_default()).map_err(|e| format!("写入 auth.json 失败:{e}"))?;
    let codex_model = acfg.codex_default.clone();

    // 模型目录:用官方模板条目做骨架,每个模型生成一条(Codex 与 Grok 共用逻辑)
    let write_catalog = |dir_rel: &str, models: &[String]| -> Result<PathBuf, String> {
        let catalog_path = p(&[dir_rel, "maomao-model-catalog.json"])?;
        let template: serde_json::Value = serde_json::from_str(
            include_str!("resources/codex_deepseek_catalog_template.json")
        ).map_err(|e| format!("模板解析失败:{e}"))?;
        let skeleton = template.pointer("/models/0").cloned()
            .ok_or("模板缺少 models[0]")?;
        let entries: Vec<serde_json::Value> = models.iter().enumerate().map(|(i, m)| {
            let mut e = skeleton.clone();
            let obj = e.as_object_mut().unwrap();
            obj.insert("slug".into(), serde_json::json!(m));
            obj.insert("visibility".into(), serde_json::json!("list"));
            obj.insert("display_name".into(), serde_json::json!(m));
            obj.insert("description".into(), serde_json::json!(format!("{m} · 猫猫动力")));
            obj.insert("context_window".into(), serde_json::json!(131072));
            obj.insert("max_context_window".into(), serde_json::json!(131072));
            obj.insert("priority".into(), serde_json::json!(1000 + i));
            e
        }).collect();
        let catalog = serde_json::json!({ "models": entries });
        if let Some(dir) = catalog_path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("创建 ~/{dir_rel} 失败:{e}"))?;
        }
        fs::write(&catalog_path, serde_json::to_string_pretty(&catalog).unwrap_or_default())
            .map_err(|e| format!("写入模型目录失败:{e}"))?;
        Ok(catalog_path)
    };
    let codex_catalog = write_catalog(".codex", &acfg.codex)?;

    write_toml({ let pp = p(&[".codex", "config.toml"])?; takeover_backup(&pp); pp }, &format!(
        "# Managed by 猫猫 Token Launcher\n\
         model_provider = \"maomao\"\n\
         model = \"{codex_model}\"\n\
         model_catalog_json = {}\n\
         model_reasoning_effort = \"high\"\n\n\
         [model_providers.maomao]\n\
         name = \"maomao\"\n\
         base_url = \"{BASE_OPENAI}\"\n\
         wire_api = \"responses\"\n\
         requires_openai_auth = true\n",
        serde_json::to_string(codex_catalog.to_str().unwrap_or_default()).unwrap_or_default()
    ))?;

    // Grok Build:自有 schema([model.X] 表),不认 codex 的 model_providers;BYOK 认证免登录
    let mut grok_toml = format!(
        "# Managed by 猫猫 Token Launcher\n\
         [models]\n\
         default = \"maomao\"\n\n\
         [model.maomao]\n\
         model = \"{}\"\n\
         base_url = \"{BASE_OPENAI}\"\n\
         name = \"猫猫 {}\"\n\
         api_key = \"{}\"\n\
         context_window = 131072\n",
        &acfg.grok_default, &acfg.grok_default, key
    );
    for m in acfg.grok.iter().filter(|m| *m != &acfg.grok_default) {
        grok_toml.push_str(&format!(
            "\n[model.maomao-{m}]\nmodel = \"{m}\"\nbase_url = \"{BASE_OPENAI}\"\nname = \"猫猫 {m}\"\napi_key = \"{key}\"\ncontext_window = 131072\n"
        ));
    }
    write_toml({ let pp = p(&[".grok", "config.toml"])?; takeover_backup(&pp); pp }, &grok_toml)?;
    fs::write({ let pp = p(&[".grok", "auth.json"])?; takeover_backup(&pp); pp }, serde_json::to_string_pretty(&serde_json::json!({
        "OPENAI_API_KEY": key
    })).unwrap_or_default()).map_err(|e| format!("写入 auth.json 失败:{e}"))?;

    // OpenCode
    merge_write_json({ let pp = p(&[".config", "opencode", "opencode.json"])?; takeover_backup(&pp); pp }, |v| {
        if !v.get("provider").map(|e| e.is_object()).unwrap_or(false) {
            v["provider"] = serde_json::json!({});
        }
        let mut oc_models = serde_json::Map::new();
        for m in &acfg.full {
            oc_models.insert(m.to_string(), serde_json::json!({ "name": m }));
        }
        v["provider"]["maomao"] = serde_json::json!({
            "npm": "@ai-sdk/openai-compatible",
            "name": "猫猫动力",
            "options": { "baseURL": BASE_OPENAI, "apiKey": key },
            "models": oc_models,
        });
    })?;

    // OpenClaw
    merge_write_json({ let pp = p(&[".openclaw", "openclaw.config.json"])?; takeover_backup(&pp); pp }, |v| {
        v["baseUrl"] = serde_json::json!(BASE_OPENAI);
        v["apiKey"] = serde_json::json!(key);
        v["api"] = serde_json::json!("openai-completions");
        v["models"] = serde_json::Value::Array(
            acfg.full.iter().map(|m| serde_json::json!({ "id": m, "name": m })).collect(),
        );
    })?;

    // Hermes Agent:~/.hermes/config.yaml(整文件接管,退出还原)
    {
        let hp = p(&[".hermes", "config.yaml"])?;
        takeover_backup(&hp);
        if let Some(dir) = hp.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("创建 ~/.hermes 失败:{e}"))?;
        }
        let mut yaml = String::new();
        yaml.push_str("# Managed by 猫猫 Token Launcher\n");
        yaml.push_str("model:\n");
        yaml.push_str(&format!("  default: \"{}\"\n", acfg.full_default));
        yaml.push_str("  provider: \"maomao\"\n");
        yaml.push_str("  base_url: \"https://binaryworld.world/v1\"\n");
        yaml.push_str("agent:\n");
        yaml.push_str("  max_turns: 50\n");
        yaml.push_str("custom_providers:\n");
        yaml.push_str("  - name: maomao\n");
        yaml.push_str("    base_url: https://binaryworld.world/v1\n");
        yaml.push_str(&format!("    api_key: {key}\n"));
        yaml.push_str(&format!("    model: {}\n", acfg.full_default));
        yaml.push_str("    models:\n");
        for m in &acfg.full {
            yaml.push_str(&format!("      {m}:\n        context_length: 131072\n"));
        }
        fs::write(&hp, yaml).map_err(|e| format!("写入 config.yaml 失败:{e}"))?;
    }

    Ok(())
}

#[tauri::command]
async fn save_key(key: String) -> Result<(), String> {
    let key = key.trim().to_string();
    if key.is_empty() { return Err("API Key 不能为空".into()); }
    apply_configs(&key)?;
    // 与账号登录一致:记录接管状态，重开 app 自动接管;已登录账号则保留身份(密钥管理可用)
    let (username, at) = stored_identity();
    write_state(&username, &key, at.as_deref(), None)
}

// ---------- 密钥管理(账号登录后) ----------

fn stored_state() -> Option<serde_json::Value> {
    read_json(&state_path().ok()?)
}

/// (username, access_token):有账号登录记录则保留，否则视作纯 Key 模式
fn stored_identity() -> (String, Option<String>) {
    stored_state()
        .map(|v| (
            v.get("username").and_then(|x| x.as_str()).unwrap_or("API_KEY").to_string(),
            v.get("access_token").and_then(|x| x.as_str()).map(|s| s.to_string()),
        ))
        .unwrap_or_else(|| ("API_KEY".into(), None))
}

fn stored_key() -> Result<String, String> {
    stored_state()
        .and_then(|v| v.get("key").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .filter(|k| !k.is_empty())
        .ok_or_else(|| "尚未配置密钥,请先登录".to_string())
}

fn mm_get(path: &str) -> Result<reqwest::blocking::Response, String> {
    let key = stored_key()?;
    let client = http_client().map_err(|e| format!("网络初始化失败:{e}"))?;
    client.get(format!("{SITE}{path}"))
        .header("Authorization", format!("Bearer {key}"))
        .send().map_err(|e| format!("连接站点失败:{e}"))
}

fn stored_access_token() -> Result<String, String> {
    stored_identity().1
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "请先在「账号登录」页登录后再管理密钥".to_string())
}

fn write_state(username: &str, key: &str, access_token: Option<&str>, token_id: Option<i64>) -> Result<(), String> {
    let mask = if key.len() > 13 { format!("{}…{}", &key[..10], &key[key.len()-4..]) } else { key.to_string() };
    let sp = state_path()?;
    if let Some(dir) = sp.parent() { fs::create_dir_all(dir).ok(); }
    let mut obj = serde_json::json!({ "username": username, "key_masked": mask, "key": key });
    if let Some(t) = access_token { obj["access_token"] = serde_json::json!(t); }
    if let Some(id) = token_id { obj["token_id"] = serde_json::json!(id); }
    fs::write(&sp, serde_json::to_string_pretty(&obj).unwrap_or_default())
        .map_err(|e| format!("写入记录失败:{e}"))
}

#[derive(Serialize, Clone)]
pub struct TokenEntry {
    pub id: i64,
    pub name: String,
    pub key_masked: String,
    pub status: i64,
    pub model_limits_enabled: bool,
    pub model_limits: String,
}

#[tauri::command]
async fn list_tokens() -> Result<Vec<TokenEntry>, String> {
    let resp = mm_get("/mm-admin/tokens")?;
    if resp.status() == 403 { return Err("Key 无效或已停用,请重新登录".into()); }
    let v: serde_json::Value = resp.json().map_err(|_| "响应异常".to_string())?;
    if !api_success(&v) { return Err(format!("获取密钥列表失败:{}", api_error(&v))); }
    Ok(v.pointer("/data")
        .and_then(|i| i.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|t| TokenEntry {
            id: t.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
            name: t.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            key_masked: t.get("key_masked").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            status: t.get("status").and_then(|v| v.as_i64()).unwrap_or(1),
            model_limits_enabled: t.get("model_limits_enabled").and_then(|v| v.as_bool()).unwrap_or(false),
            model_limits: t.get("model_limits").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        })
        .collect())
}

// ---------- 接管开关(翻墙软件式:关=立即还原配置,登录记录保留) ----------

#[tauri::command]
async fn takeover(app: tauri::AppHandle, on: bool) -> Result<(), String> {
    let st = stored_state().ok_or("尚未登录或保存过 Key")?;
    let key = st.get("key").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if key.is_empty() { return Err("尚未配置密钥,请先登录".into()); }
    if on {
        apply_configs(&key)?;
    } else {
        restore_files()?;
    }
    let mut obj = st.clone();
    obj["takeover"] = serde_json::json!(on);
    if let Ok(sp) = state_path() {
        let _ = fs::write(&sp, serde_json::to_string_pretty(&obj).unwrap_or_default());
    }
    use tauri::Emitter;
    let _ = app.emit("mm-takeover", on);
    Ok(())
}

// ---------- 最近调用 ----------

#[derive(Serialize, Clone)]
pub struct RecentCall {
    pub created_at: i64,
    pub model_name: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub usd: f64,
}

#[derive(Serialize, Clone)]
struct SeriesPoint { name: String, tokens: i64, points: Vec<i64> }
#[derive(Serialize, Clone)]
struct AxisPoint { label: String, tip: String }
#[derive(Serialize, Clone)]
struct UsageData {
    series: Vec<SeriesPoint>,
    axis: Vec<AxisPoint>,
    granularity: String,
    week_calls: i64, week_usd: f64,
    today_calls: i64, today_usd: f64,
}

/// 近 7 天调用量(sk-key 鉴权),app「更多」页图表用;series/axis 与控制台同构
#[tauri::command]
async fn usage_daily() -> Result<UsageData, String> {
    let resp = mm_get("/mm-admin/usage?days=7")?;
    if resp.status() == 403 { return Err("Key 无效或已停用,请重新登录".into()); }
    let v: serde_json::Value = resp.json().map_err(|_| "响应异常".to_string())?;
    if !api_success(&v) { return Err(format!("获取调用量失败:{}", api_error(&v))); }
    let series = v.pointer("/data/series")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|x| SeriesPoint {
            name: x.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            tokens: x.get("tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            points: x.get("points").and_then(|v| v.as_array()).map(|a| a.iter().map(|p| p.as_i64().unwrap_or(0)).collect()).unwrap_or_default(),
        })
        .collect();
    let axis = v.pointer("/data/axis")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|x| AxisPoint {
            label: x.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            tip: x.get("tip").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        })
        .collect();
    Ok(UsageData {
        series,
        axis,
        granularity: v.pointer("/data/granularity").and_then(|v| v.as_str()).unwrap_or("day").to_string(),
        week_calls: v.pointer("/data/week/calls").and_then(|v| v.as_i64()).unwrap_or(0),
        week_usd: v.pointer("/data/week/usd").and_then(|v| v.as_f64()).unwrap_or(0.0),
        today_calls: v.pointer("/data/today/calls").and_then(|v| v.as_i64()).unwrap_or(0),
        today_usd: v.pointer("/data/today/usd").and_then(|v| v.as_f64()).unwrap_or(0.0),
    })
}

#[tauri::command]
async fn recent_calls() -> Result<Vec<RecentCall>, String> {
    let resp = mm_get("/mm-admin/log-key")?;
    if resp.status() == 403 { return Err("Key 无效或已停用,请重新登录".into()); }
    let v: serde_json::Value = resp.json().map_err(|_| "响应异常".to_string())?;
    if !api_success(&v) { return Err(format!("获取调用记录失败:{}", api_error(&v))); }
    Ok(v.pointer("/data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|x| RecentCall {
            created_at: x.get("created_at").and_then(|v| v.as_i64()).unwrap_or(0),
            model_name: x.get("model_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            prompt_tokens: x.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            completion_tokens: x.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            usd: x.get("usd").and_then(|v| v.as_f64()).unwrap_or(0.0),
        })
        .collect())
}

// ---------- 账户余额 ----------

#[derive(Serialize)]
pub struct Balance {
    pub quota_usd: f64,  // 剩余额度(美元)
    pub used_usd: f64,   // 累计已用(美元)
}

#[derive(Serialize, Clone)]
pub struct InviteInfo {
    pub aff_code: String,
    pub aff_count: i64,
    pub aff_usd: f64, // 待结提成(美元)
}

#[tauri::command]
async fn invite_claim() -> Result<Balance, String> {
    let key = stored_key()?;
    let client = http_client().map_err(|e| format!("网络初始化失败:{e}"))?;
    let resp = client.post(format!("{SITE}/mm-admin/invite-claim"))
        .header("Authorization", format!("Bearer {key}"))
        .send().map_err(|e| format!("请求失败:{e}"))?;
    if resp.status() == 400 {
        let v: serde_json::Value = resp.json().unwrap_or_default();
        return Err(v.get("message").and_then(|m| m.as_str()).unwrap_or("待结提成不足 $1").to_string());
    }
    if resp.status() != 200 { return Err("转入失败,请稍后再试".into()); }
    let v: serde_json::Value = resp.json().map_err(|_| "响应异常".to_string())?;
    if !api_success(&v) { return Err(format!("转入失败:{}", api_error(&v))); }
    Ok(Balance {
        quota_usd: v.pointer("/data/quota_usd").and_then(|x| x.as_f64()).unwrap_or(0.0),
        used_usd: 0.0,
    })
}

#[tauri::command]
async fn invite_info() -> Result<InviteInfo, String> {
    let resp = mm_get("/mm-admin/invite-key")?;
    if resp.status() == 403 { return Err("Key 无效或已停用,请重新登录".into()); }
    let v: serde_json::Value = resp.json().map_err(|_| "响应异常".to_string())?;
    if !api_success(&v) { return Err(format!("获取邀请信息失败:{}", api_error(&v))); }
    Ok(InviteInfo {
        aff_code: v.pointer("/data/aff_code").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        aff_count: v.pointer("/data/aff_count").and_then(|x| x.as_i64()).unwrap_or(0),
        aff_usd: v.pointer("/data/aff_quota").and_then(|x| x.as_f64()).unwrap_or(0.0) / 500000.0,
    })
}

#[tauri::command]
async fn get_balance() -> Result<Balance, String> {
    let resp = mm_get("/mm-admin/balance")?;
    if resp.status() == 403 { return Err("Key 无效或已停用,请重新登录".into()); }
    let v: serde_json::Value = resp.json().map_err(|_| "响应异常".to_string())?;
    if !api_success(&v) { return Err(format!("查询余额失败:{}", api_error(&v))); }
    Ok(Balance {
        quota_usd: v.pointer("/data/quota_usd").and_then(|x| x.as_f64()).unwrap_or(0.0),
        used_usd: v.pointer("/data/used_usd").and_then(|x| x.as_f64()).unwrap_or(0.0),
    })
}

#[tauri::command]
async fn create_token(name: String) -> Result<(), String> {
    let token = stored_access_token()?;
    let name = name.trim().to_string();
    if name.is_empty() { return Err("密钥名称不能为空".into()); }
    let client = http_client().map_err(|e| format!("网络初始化失败:{e}"))?;
    let resp: serde_json::Value = client
        .post(format!("{SITE}/api/token/"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "name": name,
            "expired_time": -1,
            "unlimited_quota": true,
            "remain_quota": 0,
            "model_limits_enabled": false,
        }))
        .send().map_err(|e| format!("创建密钥失败:{e}"))?
        .json().map_err(|_| "创建响应异常".to_string())?;
    if !api_success(&resp) { return Err(format!("创建密钥失败:{}", api_error(&resp))); }
    Ok(())
}

#[tauri::command]
async fn use_token(id: i64) -> Result<String, String> {
    let key0 = stored_key()?;
    let client = http_client().map_err(|e| format!("网络初始化失败:{e}"))?;
    let resp = client.post(format!("{SITE}/mm-admin/token-key"))
        .header("Authorization", format!("Bearer {key0}"))
        .json(&serde_json::json!({ "id": id }))
        .send().map_err(|e| format!("获取 Key 失败:{e}"))?;
    if resp.status() == 403 { return Err("Key 无效或该令牌不可用".into()); }
    let v: serde_json::Value = resp.json().map_err(|_| "响应异常".to_string())?;
    if !api_success(&v) { return Err(format!("获取 Key 失败:{}", api_error(&v))); }
    let key = v.pointer("/data/key").and_then(|x| x.as_str()).unwrap_or("").to_string();
    if !key.starts_with("sk-") { return Err("站点未返回有效 Key".into()); }
    apply_configs(&key)?;
    let (username, at) = stored_identity();
    let mask = if key.len() > 13 { format!("{}…{}", &key[..10], &key[key.len()-4..]) } else { key.clone() };
    write_state(&username, &key, at.as_deref(), Some(id))?;
    Ok(mask)
}

// ---------- 账号登录(方案①) ----------

fn http_client() -> reqwest::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .cookie_store(true)
        .timeout(Duration::from_secs(15))
        .user_agent("MaomaoLauncher/1.2")
        .build()
}

fn api_success(v: &serde_json::Value) -> bool {
    v.get("success").and_then(|s| s.as_bool()).unwrap_or(false)
}

fn api_error(v: &serde_json::Value) -> String {
    v.get("message").and_then(|m| m.as_str()).unwrap_or("未知错误").to_string()
}

/// 浏览器深链授权:网页端确认后由前端调用写入(deep_link_setup),或旧深链路径处理
fn deep_link_setup_impl(key: &str, username: &str, at: Option<&str>, token_id: Option<i64>) -> Result<String, String> {
    let key = key.trim().to_string();
    let username = username.trim().to_string();
    if !key.starts_with("sk-") || key.len() < 12 {
        return Err("授权数据无效，请在网页端重试".into());
    }
    apply_configs(&key)?;
    let mask = if key.len() > 13 { format!("{}…{}", &key[..10], &key[key.len()-4..]) } else { key.clone() };
    // 深链带回的 access_token 优先(密钥管理可用),否则沿用本地已有身份
    let (_, old_at) = stored_identity();
    let at = at.filter(|s| !s.is_empty()).map(|s| s.to_string()).or(old_at);
    write_state(if username.is_empty() { "API_KEY" } else { &username }, &key, at.as_deref(), token_id)?;
    Ok(mask)
}

#[tauri::command]
async fn deep_link_setup(key: String, username: String, at: Option<String>, id: Option<i64>) -> Result<String, String> {
    deep_link_setup_impl(&key, &username, at.as_deref(), id)
}

/// 处理 maomao://auth?key=…&user=…&at=…&id=… 深链:弹确认由用户放行后才写入
/// (任意网页都能发起该协议,不确认就接管等于允许第三方静默改写本机配置)
fn handle_auth_urls(app: &tauri::AppHandle, urls: Vec<tauri::Url>) {
    use tauri::Emitter;
    for u in urls {
        if u.host_str() != Some("auth") { continue; }
        let get = |k: &str| u.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.to_string()).unwrap_or_default();
        let key = get("key");
        if !key.starts_with("sk-") || key.len() < 12 {
            let _ = app.emit("mm-auth-error", "授权数据无效，请回到网页重试".to_string());
            continue;
        }
        let mask = if key.len() > 13 { format!("{}…{}", &key[..10], &key[key.len()-4..]) } else { key.clone() };
        let _ = app.emit("mm-auth-request", serde_json::json!({
            "key": key,
            "user": get("user"),
            "at": get("at"),
            "id": get("id").parse::<i64>().ok(),
            "mask": mask,
        }));
    }
}

#[tauri::command]
async fn login_and_setup(username: String, password: String, turnstile: Option<String>) -> Result<String, String> {
    if username.trim().is_empty() || password.is_empty() {
        return Err("请输入账号和密码".into());
    }
    let client = http_client().map_err(|e| format!("网络初始化失败:{e}"))?;

    // 1) 登录:该版本 new-api 在响应 JSON 里返回 access_token(非 cookie 会话)
    let ts = turnstile.unwrap_or_default();
    let login_url = if ts.is_empty() {
        format!("{SITE}/api/user/login")
    } else {
        format!("{SITE}/api/user/login?turnstile={ts}")
    };
    let resp: serde_json::Value = client
        .post(login_url)
        .json(&serde_json::json!({ "username": username.trim(), "password": password }))
        .send().map_err(|e| format!("连接站点失败:{e}"))?
        .json().map_err(|_| "站点响应异常".to_string())?;
    if !api_success(&resp) {
        return Err(format!("登录失败:{}", api_error(&resp)));
    }
    let token = resp.pointer("/data/access_token")
        .and_then(|t| t.as_str())
        .filter(|t| !t.is_empty())
        .ok_or("账号开启了二次验证或需要额外验证，请改用「直接填 Key」方式")?
        .to_string();
    let auth_header = format!("Bearer {token}");

    // 2) 查询已有同名令牌:有则复用，没有才创建
    let list_tokens = |client: &reqwest::blocking::Client| -> Result<Vec<serde_json::Value>, String> {
        let resp: serde_json::Value = client
            .get(format!("{SITE}/api/token/?p=1&size=100"))
            .header("Authorization", &auth_header)
            .send().map_err(|e| format!("查询令牌失败:{e}"))?
            .json().map_err(|_| "令牌列表异常".to_string())?;
        if !api_success(&resp) {
            return Err(format!("查询令牌失败:{}", api_error(&resp)));
        }
        Ok(resp.pointer("/data/items")
            .and_then(|items| items.as_array())
            .cloned()
            .unwrap_or_default())
    };

    let find_id = |items: &Vec<serde_json::Value>| -> Option<i64> {
        items.iter()
            .find(|t| t.get("name").and_then(|n| n.as_str()) == Some(TOKEN_NAME))
            .and_then(|t| t.get("id"))
            .and_then(|i| i.as_i64())
    };

    let mut id = find_id(&list_tokens(&client)?);
    if id.is_none() {
        let resp: serde_json::Value = client
            .post(format!("{SITE}/api/token/"))
            .header("Authorization", &auth_header)
            .json(&serde_json::json!({
                "name": TOKEN_NAME,
                "expired_time": -1,
                "unlimited_quota": true,
                "remain_quota": 0,
                "model_limits_enabled": false,
            }))
            .send().map_err(|e| format!("创建令牌失败:{e}"))?
            .json().map_err(|_| "令牌响应异常".to_string())?;
        if !api_success(&resp) {
            return Err(format!("创建令牌失败:{}", api_error(&resp)));
        }
        id = find_id(&list_tokens(&client)?);
    }
    let id = id.ok_or("未找到令牌，请到控制台手动复制 Key")?;

    // 4) 取完整 key
    let resp: serde_json::Value = client
        .post(format!("{SITE}/api/token/{id}/key"))
        .header("Authorization", &auth_header)
        .send().map_err(|e| format!("获取 Key 失败:{e}"))?
        .json().map_err(|_| "Key 响应异常".to_string())?;
    if !api_success(&resp) {
        return Err(format!("获取 Key 失败:{}", api_error(&resp)));
    }
    let raw = match &resp["data"] {
        serde_json::Value::String(s) => s.clone(),
        v => v.get("key").and_then(|k| k.as_str()).unwrap_or("").to_string(),
    };
    if raw.is_empty() { return Err("站点未返回 Key".into()); }
    let key = if raw.starts_with("sk-") { raw } else { format!("sk-{raw}") };

    apply_configs(&key)?;
    let mask = if key.len() > 13 { format!("{}…{}", &key[..10], &key[key.len()-4..]) } else { key.clone() };
    write_state(username.trim(), &key, Some(&token), Some(id))?;
    Ok(mask)
}

/// 移除 JSON 文件的指定键;删空对象/空文件则整个删除
fn json_remove(path: PathBuf, keys: &[&str], nested: Option<&str>) -> Result<(), String> {
    if !path.exists() { return Ok(()); }
    let mut v = match read_json(&path) { Some(v) => v, None => { let _ = fs::remove_file(&path); return Ok(()); } };
    {
        let target = match nested { Some(k) => v.get_mut(k), None => Some(&mut v) };
        if let Some(obj) = target {
            if obj.is_object() {
                for key in keys { obj.as_object_mut().unwrap().remove(*key); }
            }
        }
    }
    if v.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        let _ = fs::remove_file(&path);
    } else {
        let out = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
        fs::write(&path, out).map_err(|e| format!("写回失败:{e}"))?;
    }
    Ok(())
}

/// 本启动器管理的 TOML(含标记注释)整文件删除
fn remove_managed_toml(path: PathBuf) {
    if let Ok(content) = fs::read_to_string(&path) {
        if content.contains("猫猫 Token Launcher") {
            let _ = fs::remove_file(&path);
        }
    }
}

/// 恢复所有被接管的文件到最初状态(有备份还原，无备份删除)
fn restore_files() -> Result<(), String> {
    for rel in [
        (".claude", "settings.json"),
        (".codex", "auth.json"),
        (".codex", "config.toml"),
        (".grok", "auth.json"),
        (".grok", "config.toml"),
        (".openclaw", "openclaw.config.json"),
        (".hermes", "config.yaml"),
    ] {
        if let Ok(pp) = p(&[rel.0, rel.1]) { restore_or_remove(&pp); }
    }
    restore_or_remove(&p(&[".config", "opencode", "opencode.json"])?);
    let _ = fs::remove_file(p(&[".codex", "maomao-model-catalog.json"])?);
    Ok(())
}

/// 退出账号:还原配置 + 忘掉登录记录
#[tauri::command]
async fn logout() -> Result<(), String> {
    restore_files()?;
    let _ = fs::remove_file(state_path()?);
    Ok(())
}

// ---------- 启动与打开 ----------

// ---------- 安装 agent ----------

fn open_external(url: &str) {
    #[cfg(target_os = "macos")]
    { let _ = Command::new("open").arg(url).output(); }
    #[cfg(target_os = "windows")]
    { use std::os::windows::process::CommandExt; let mut c = Command::new("cmd"); let _ = c.raw_arg(format!("/C start \"\" \"{url}\"")); let _ = run_quiet(&mut c); }
}

fn run_in_terminal(script: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let out = Command::new("osascript").args(["-e", &format!(
        "tell application \"Terminal\" to activate\ntell application \"Terminal\" to do script \"{}\"",
        script
    )]).output().map_err(|e| format!("打开终端失败:{e}"))?;
    #[cfg(target_os = "windows")]
    let out = {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd");
        c.raw_arg(format!("/C start \"猫猫安装\" cmd /K {script}"));
        c.output()
    }.map_err(|e| format!("打开终端失败:{e}"))?;
    if !out.status.success() {
        return Err(format!("启动失败:{}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(())
}

/// 安装/升级工具:桌面版走自托管安装包;CLI 按官方推荐方式(curl 脚本/npm;缺 Node 先引导,npmmirror 源)
/// upgrade=true 时 npm 命令带 @latest;官方 curl 脚本幂等,重跑即升级
#[tauri::command]
async fn install_tool(app: tauri::AppHandle, id: String, upgrade: Option<bool>) -> Result<String, String> {
    let arch = std::env::consts::ARCH;
    let win = cfg!(target_os = "windows");
    let upgrading = upgrade.unwrap_or(false);
    let verb = if upgrading { "升级" } else { "安装" };
    // 桌面版:自托管安装包直链(升级=重新下载覆盖安装)
    let desktop_url: Option<String> = match (id.as_str(), win, arch) {
        ("claude-desktop", false, "aarch64") => Some("/Claude-mac-arm64.dmg".into()),
        ("claude-desktop", false, "x86_64") => Some("/Claude-mac-x64.dmg".into()),
        ("claude-desktop", true, "x86_64") => Some("/Claude-win-x64.exe".into()),
        ("codex-desktop", false, "aarch64") => Some("/Codex-mac-arm64.dmg".into()),
        ("codex-desktop", false, "x86_64") => Some("/Codex-mac-x64.dmg".into()),
        ("codex-desktop", true, "x86_64") => Some("/Codex-win-x64.msix".into()),
        ("codex-desktop", true, "aarch64") => Some("/Codex-win-arm64.msix".into()),
        _ => None,
    };
    if let Some(path) = desktop_url {
        // 桌面版:后台下载(带进度)→ 自动静默安装 → 完成事件
        let url = format!("{SITE}{path}");
        let idc = id.clone();
        std::thread::spawn(move || {
            use std::io::Read;
            use tauri::Emitter;
            let verb = if upgrading { "升级" } else { "安装" };
            let fail = |t: String| {
                let _ = app.emit("mm-install", serde_json::json!({ "id": idc, "ok": false, "tail": t, "verb": verb }));
            };
            let client = match http_client() { Ok(c) => c, Err(e) => return fail(format!("网络初始化失败:{e}")) };
            let mut resp = match client.get(&url).send() {
                Ok(r) if r.status().is_success() => r,
                Ok(r) => return fail(format!("下载失败 HTTP {}", r.status())),
                Err(e) => return fail(format!("下载失败:{e}")),
            };
            let total = resp.content_length().unwrap_or(0);
            let ext = if url.ends_with(".msix") { "msix" } else if url.ends_with(".exe") { "exe" } else { "dmg" };
            let dest = std::env::temp_dir().join(format!("maomao-setup-{idc}.{ext}"));
            let mut buf: Vec<u8> = Vec::with_capacity(total as usize);
            let mut chunk = [0u8; 262144];
            let mut last_pct: i64 = -1;
            loop {
                match resp.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&chunk[..n]);
                        if total > 0 {
                            let pct = (buf.len() as f64 / total as f64 * 100.0) as i64;
                            if pct >= last_pct + 5 {
                                last_pct = pct;
                                let _ = app.emit("mm-install", serde_json::json!({ "id": idc, "ok": true, "pct": pct, "progress": true, "tail": "", "verb": verb }));
                            }
                        }
                    }
                    Err(e) => return fail(format!("下载中断:{e}")),
                }
            }
            if buf.len() < 1024 { return fail("下载内容异常(过小)".into()); }
            if let Err(e) = fs::write(&dest, &buf) { return fail(format!("写入临时文件失败:{e}")); }
            drop(buf);
            #[cfg(target_os = "macos")]
            let install_res: Result<(), String> = (|| {
                let mnt = std::env::temp_dir().join(format!("maomao-mnt-{idc}"));
                let _ = fs::remove_dir_all(&mnt);
                let _ = fs::create_dir_all(&mnt);
                let out = Command::new("hdiutil").args(["attach", "-nobrowse", "-quiet"]).arg(&dest).arg("-mountpoint").arg(&mnt).output().map_err(|e| format!("挂载失败:{e}"))?;
                if !out.status.success() { return Err(format!("挂载失败:{}", String::from_utf8_lossy(&out.stderr))); }
                let appsrc = fs::read_dir(&mnt).ok().and_then(|rd| {
                    rd.filter_map(|e| e.ok()).map(|e| e.path())
                        .find(|p| p.extension().map(|x| x == "app").unwrap_or(false))
                }).ok_or_else(|| "卷内未找到 .app".to_string())?;
                let dst = PathBuf::from("/Applications").join(appsrc.file_name().unwrap_or_default());
                let _ = fs::remove_dir_all(&dst);
                let out = Command::new("cp").args(["-R"]).arg(&appsrc).arg(&dst).output().map_err(|e| format!("拷贝失败:{e}"))?;
                let _ = Command::new("hdiutil").args(["detach", "-quiet"]).arg(&mnt).output();
                if !out.status.success() { return Err(format!("拷贝失败:{}", String::from_utf8_lossy(&out.stderr))); }
                Ok(())
            })();
            #[cfg(target_os = "windows")]
            let install_res: Result<(), String> = {
                use std::os::windows::process::CommandExt;
                if ext == "msix" {
                    let mut c = Command::new("powershell");
                    c.args(["-NoProfile", "-Command", &format!("Add-AppxPackage -Path '{}'", dest.to_string_lossy())]);
                    match run_quiet(&mut c) {
                        Ok(o) if o.status.success() => Ok(()),
                        Ok(o) => Err(format!("安装失败:{}", String::from_utf8_lossy(&o.stderr))),
                        Err(e) => Err(format!("安装失败:{e}")),
                    }
                } else {
                    let mut c = Command::new("cmd");
                    c.raw_arg(format!("/C start /wait "" "{}" /S", dest.to_string_lossy()));
                    match run_quiet(&mut c) {
                        Ok(o) if o.status.success() => Ok(()),
                        Ok(o) => Err(format!("安装失败:{}", String::from_utf8_lossy(&o.stderr))),
                        Err(e) => Err(format!("安装失败:{e}")),
                    }
                }
            };
            let _ = fs::remove_file(&dest);
            match install_res {
                Ok(()) => { let _ = app.emit("mm-install", serde_json::json!({ "id": idc, "ok": true, "tail": "", "verb": verb })); }
                Err(e) => fail(format!("{verb}失败:{e}")),
            }
        });
        return Ok(format!("正在后台下载并自动{verb},完成后自动刷新"));
    }
    // Windows:Grok 官方暂无原生安装(建议 WSL),只能开文档
    if win && id == "grok" {
        open_external("https://x.ai/build");
        return Ok("Grok 暂无 Windows 原生安装,已打开官方文档(可用 WSL)".into());
    }
    let npm_pkg: Option<&str> = match id.as_str() {
        "claude" => Some("@anthropic-ai/claude-code"),
        "codex" => Some("@openai/codex"),
        "gemini" => Some("@google/gemini-cli"),
        "opencode" if win => Some("opencode-ai"),
        "openclaw" => Some("openclaw"),
        "pi" => Some("@mariozechner/pi-coding-agent"),
        _ => None,
    };
    let registry = "https://registry.npmmirror.com";
    let latest = if upgrading { "@latest" } else { "" };
    // 静默后台安装脚本:macOS 用官方 curl 脚本或 npm(交互登录 shell,拿 mise 等环境);Windows 走 cmd
    let shell_script: Option<String> = if win {
        match id.as_str() {
            "hermes" => Some("powershell -NoProfile -Command \"iex (irm https://hermes-agent.nousresearch.com/install.ps1)\"".into()),
            _ => npm_pkg.map(|pkg| format!("npm install -g {pkg}{latest} --registry={registry}")),
        }
    } else {
        match id.as_str() {
            "opencode" => Some("curl -fsSL https://opencode.ai/install | bash".into()),
            "grok" => Some("curl -fsSL https://x.ai/cli/install.sh | bash".into()),
            "hermes" => Some("curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash".into()),
            "openclaw" => Some(format!("command -v bun >/dev/null || curl -fsSL https://bun.sh/install | bash; npm install -g openclaw{latest} --registry={registry}")),
            _ => npm_pkg.map(|pkg| format!("npm install -g {pkg}{latest} --registry={registry}")),
        }
    };
    if let Some(script) = shell_script {
        // npm 系需要 Node;curl 脚本自带运行时。GUI 进程 PATH 不可靠,走登录 shell 检测链
        if npm_pkg.is_some() && !which_in_login_shell("node") {
            let node_url = if win {
                "https://cdn.npmmirror.com/binaries/node/v22.14.0/node-v22.14.0-x64.msi"
            } else {
                "https://cdn.npmmirror.com/binaries/node/v22.14.0/node-v22.14.0.pkg"
            };
            open_external(node_url);
            return Ok("未检测到 Node,已打开 Node 安装器(装完后回来再点安装)".into());
        }
        run_silent_bg(app.clone(), &id, script, verb);
        return Ok(format!("正在后台{verb}…完成后自动刷新"));
    }
    Err("该工具暂不支持一键安装".into())
}

/// 后台静默执行脚本,完成/失败 emit mm-install{ id, ok, tail, verb }
fn run_silent_bg(app: tauri::AppHandle, id: &str, script: String, verb: &str) {
    let idc = id.to_string();
    let verbc = verb.to_string();
    std::thread::spawn(move || {
        use tauri::Emitter;
        let res = if cfg!(target_os = "windows") {
            #[cfg(target_os = "windows")]
            { run_quiet(Command::new("cmd").args(["/C", &script])) }
            #[cfg(not(target_os = "windows"))]
            { Err(std::io::Error::other("windows only")) }
        } else {
            Command::new("/bin/zsh").args(["-i", "-l", "-c", &script]).output()
        };
        let (ok, tail) = match res {
            Ok(o) => {
                let mut t = String::from_utf8_lossy(&o.stdout).to_string();
                t.push_str(&String::from_utf8_lossy(&o.stderr));
                (o.status.success(), t)
            }
            Err(e) => (false, e.to_string()),
        };
        let chars: Vec<char> = tail.chars().collect();
        let tail: String = chars[chars.len().saturating_sub(400)..].iter().collect();
        let _ = app.emit("mm-install", serde_json::json!({ "id": idc, "ok": ok, "tail": tail, "verb": verbc }));
    });
}

/// 卸载工具:静默后台执行(npm uninstall / 删目录);grok/hermes 打开官方文档
#[tauri::command]
async fn uninstall_tool(app: tauri::AppHandle, id: String) -> Result<String, String> {
    let win = cfg!(target_os = "windows");
    let npm_pkg: Option<&str> = match (id.as_str(), win) {
        ("claude", _) => Some("@anthropic-ai/claude-code"),
        ("codex", _) => Some("@openai/codex"),
        ("gemini", _) => Some("@google/gemini-cli"),
        ("opencode", true) => Some("opencode-ai"),
        ("openclaw", _) => Some("openclaw"),
        ("pi", _) => Some("@mariozechner/pi-coding-agent"),
        _ => None,
    };
    if let Some(pkg) = npm_pkg {
        run_silent_bg(app.clone(), &id, format!("npm uninstall -g {pkg}"), "卸载");
        return Ok("正在后台卸载…完成后自动刷新".into());
    }
    match id.as_str() {
        "opencode" if !win => {
            let bin = std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".opencode/bin"))
                .ok()
                .filter(|p| p.exists());
            if let Some(p) = bin {
                fs::remove_dir_all(&p).map_err(|e| format!("删除失败:{e}"))?;
                return Ok("已删除 opencode 程序(配置保留;若由 mise/brew 安装请用对应方式卸载)".into());
            }
            Ok("未找到 ~/.opencode/bin(可能由 mise/brew 安装,请用对应方式卸载)".into())
        }
        "claude-desktop" | "codex-desktop" => {
            #[cfg(target_os = "macos")]
            {
                let names: &[&str] = if id == "claude-desktop" { &["Claude"] } else { &["Codex", "ChatGPT"] };
                match find_app(names) {
                    Some(app) => {
                        let _ = Command::new("open").args(["-R", &app]).output();
                        Ok("已在 Finder 定位,拖入废纸篓即可卸载".into())
                    }
                    None => Ok("未找到应用".into()),
                }
            }
            #[cfg(target_os = "windows")]
            {
                // MSIX(ChatGPT/Codex)直接移除;传统安装器(Claude)调其卸载程序静默跑;都找不到才开应用列表
                let pat = if id == "claude-desktop" { "Claude" } else { "ChatGPT|Codex" };
                let ps = format!(
                    r#"$p = Get-AppxPackage | Where-Object {{ $_.Name -match '{pat}' }} | Select-Object -First 1; if ($p) {{ $p | Remove-AppxPackage; Write-Output 'MSIX' }} else {{ $u = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue | Where-Object {{ $_.DisplayName -match '{pat}' }} | Select-Object -First 1; if ($u -and $u.UninstallString) {{ $c = $u.UninstallString -replace '"',''; Start-Process $c -ArgumentList '/S' -Wait; Write-Output 'UNINST' }} else {{ Write-Output 'NONE' }}}}"#
                );
                match run_quiet(Command::new("powershell").args(["-NoProfile", "-Command", &ps])) {
                    Ok(o) if o.status.success() => {
                        match String::from_utf8_lossy(&o.stdout).trim() {
                            "MSIX" => Ok("已卸载完成".into()),
                            "UNINST" => Ok("已调用卸载程序(如弹出窗口请按提示完成)".into()),
                            _ => { open_external("ms-settings:appsfeatures"); Ok("未找到可直接卸载的入口,已打开系统应用管理".into()) }
                        }
                    }
                    _ => { open_external("ms-settings:appsfeatures"); Ok("已打开系统应用管理,在那里卸载".into()) }
                }
            }
        }
        "grok" => {
            open_external("https://x.ai/build");
            Ok("Grok 卸载请按官方文档操作(已打开)".into())
        }
        "hermes" => {
            open_external("https://hermes-agent.nousresearch.com/docs");
            Ok("Hermes 卸载请按官方文档操作(已打开)".into())
        }
        _ => Err("该工具暂不支持一键卸载".into()),
    }
}

/// Claude Code 模型映射说明用:主模型/快速模型(后台 app-config 下发)
#[tauri::command]
async fn claude_model_map() -> Result<serde_json::Value, String> {
    let c = fetch_app_config();
    Ok(serde_json::json!({ "models": c.claude }))
}

/// 查工具版本:(本机版本, 最新版)。最新版走 npm registry;curl 脚本系拿不到则 latest=None
#[tauri::command]
async fn tool_versions(id: String) -> Result<Option<(Option<String>, Option<String>)>, String> {
    let win = cfg!(target_os = "windows");
    let cmd = match id.as_str() {
        "claude" => "claude",
        "codex" => "codex",
        "gemini" => "gemini",
        "opencode" => "opencode",
        "openclaw" => "openclaw",
        "pi" => "pi",
        "grok" | "hermes" => return Ok(Some((None, None))), // 无稳定版本渠道,菜单保持显示升级
        _ => return Ok(None), // 桌面版等不支持
    };
    let run = |script: String| -> Option<String> {
        #[cfg(target_os = "windows")]
        let out = run_quiet(Command::new("cmd").args(["/C", &script])).ok();
        #[cfg(not(target_os = "windows"))]
        let out = Command::new("/bin/zsh").args(["-i", "-l", "-c", &script]).output().ok();
        let o = out?;
        let txt = String::from_utf8_lossy(&o.stdout).to_string();
        // 取输出里第一个 x.y.z 形式的版本号
        let mut it = txt.split(|c: char| !c.is_ascii_digit() && c != '.');
        it.find(|s| {
            let parts: Vec<&str> = s.split('.').collect();
            parts.len() >= 2 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        })
        .map(|s| s.to_string())
    };
    let current = run(format!("{cmd} --version 2>/dev/null | head -2"));
    let latest = if win {
        run(format!("npm view opencode-ai version 2>nul", ).replace("opencode-ai",
            match id.as_str() {
                "claude" => "@anthropic-ai/claude-code",
                "codex" => "@openai/codex",
                "gemini" => "@google/gemini-cli",
                "opencode" => "opencode-ai",
                "openclaw" => "openclaw",
                _ => "pi",
            }))
    } else {
        let pkg = match id.as_str() {
            "claude" => "@anthropic-ai/claude-code",
            "codex" => "@openai/codex",
            "gemini" => "@google/gemini-cli",
            "opencode" => "opencode-ai",
            "openclaw" => "openclaw",
            _ => "@mariozechner/pi-coding-agent",
        };
        run(format!("npm view {pkg} version --registry=https://registry.npmmirror.com 2>/dev/null"))
    };
    Ok(Some((current, latest)))
}

#[tauri::command]
async fn launch(id: String) -> Result<(), String> {
    let spec = tools_spec().into_iter().find(|t| t.0 == id)
        .ok_or_else(|| format!("未知工具:{id}"))?;
    match spec.2 {
        ToolKind::Desktop => {
            let app = match id.as_str() {
                "claude-desktop" => find_app(&["Claude"]).ok_or("未找到 Claude 桌面版")?,
                "codex-desktop" => find_app(&["Codex", "ChatGPT"]).ok_or("未找到 ChatGPT / Codex 桌面版")?,
                _ => return Err("未知应用".into()),
            };
            #[cfg(target_os = "macos")]
            let out = Command::new("open").args(["-a", app.as_str()]).output()
                .map_err(|e| format!("启动失败:{e}"))?;
            #[cfg(target_os = "windows")]
            let out = {
                use std::os::windows::process::CommandExt;
                let mut c = if app.starts_with("shell:AppsFolder") {
                    // MSIX/Store 应用:explorer 按 AppID 启动
                    let mut c = Command::new("explorer");
                    c.arg(&app);
                    c
                } else {
                    let mut c = Command::new("cmd");
                    c.raw_arg(format!("/C start \"\" \"{app}\""));
                    c
                };
                run_quiet(&mut c)
            }.map_err(|e| format!("启动失败:{e}"))?;
            if !out.status.success() {
                return Err(format!("启动 {app} 失败:{}", String::from_utf8_lossy(&out.stderr)));
            }
            Ok(())
        }
        ToolKind::Cli => {
            if !which_in_login_shell(spec.0) {
                return Err(format!("{} 未安装，请先参考官方文档安装", spec.1));
            }
            #[cfg(target_os = "macos")]
            let out = Command::new("osascript").args(["-e", &format!(
                "tell application \"Terminal\" to activate\ntell application \"Terminal\" to do script \"{}\"",
                spec.0
            )]).output().map_err(|e| format!("打开终端失败:{e}"))?;
            #[cfg(target_os = "windows")]
            let cmd_name = spec.0;
            #[cfg(target_os = "windows")]
            let out = {
                use std::os::windows::process::CommandExt;
                let mut c = Command::new("cmd");
                // raw_arg 原样传给 cmd，避免 Rust 二次加引号把 start 的标题/命令切碎
                c.raw_arg(format!("/C start \"猫猫 {cmd_name}\" cmd /K {cmd_name}"));
                c.output()
            }.map_err(|e| format!("打开终端失败:{e}"))?;
            if !out.status.success() {
                return Err(format!("启动失败:{}", String::from_utf8_lossy(&out.stderr)));
            }
            Ok(())
        }
    }
}

#[tauri::command]
async fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    Command::new("open").arg(&url).output().map_err(|e| format!("打开失败:{e}"))?;
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd");
        c.raw_arg(format!("/C start \"\" \"{url}\""));
        run_quiet(&mut c).map_err(|e| format!("打开失败:{e}"))?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .on_window_event(|window, event| {
            // 关窗 = 隐藏到托盘(保持接管);还原只在退出时发生
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // 二次启动(Windows/Linux 常见):把 argv 里的深链转交主实例逻辑
            use tauri::Manager;
            let urls: Vec<tauri::Url> = argv.iter()
                .filter_map(|s| tauri::Url::parse(s).ok())
                .filter(|u| u.scheme() == "maomao")
                .collect();
            if !urls.is_empty() { handle_auth_urls(app, urls); }
            if let Some(w) = app.get_webview_window("main") { let _ = w.set_focus(); }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            use tauri_plugin_deep_link::DeepLinkExt;
            let handle = app.handle().clone();
            // macOS 冷启动 + 运行中唤起统一走这里
            app.deep_link().on_open_url(move |e| handle_auth_urls(&handle, e.urls()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_state, save_key, login_and_setup, deep_link_setup, logout, launch, open_url, list_tokens, create_token, use_token, get_balance, takeover, recent_calls, invite_info, invite_claim, install_tool, uninstall_tool, tool_versions, usage_daily, claude_model_map])
        .build(tauri::generate_context!())
        .expect("error while running maomao launcher");

    // 系统托盘:显示窗口 / 快捷启动 / 退出(还原)
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{TrayIconBuilder, TrayIconEvent};
    let show = MenuItem::with_id(&app, "show", "显示主窗口", true, None::<&str>).expect("menu");
    let claude = MenuItem::with_id(&app, "claude", "启动 Claude Code", true, None::<&str>).expect("menu");
    let codex = MenuItem::with_id(&app, "codex", "启动 Codex", true, None::<&str>).expect("menu");
    let quit = MenuItem::with_id(&app, "quit", "退出(还原配置)", true, None::<&str>).expect("menu");
    let menu = Menu::with_items(&app, &[&show, &claude, &codex, &quit]).expect("menu");
    let _tray = TrayIconBuilder::with_id("maomao-tray")
        .icon(app.default_window_icon().expect("icon").clone())
        .tooltip("猫猫动力")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                use tauri::Manager;
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "claude" => { let _ = launch("claude".into()); }
            "codex" => { let _ = launch("codex".into()); }
            "quit" => {
                let _ = restore_files();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, button_state: tauri::tray::MouseButtonState::Up, .. } = event {
                let app = tray.app_handle();
                use tauri::Manager;
                if let Some(w) = app.get_webview_window("main") {
                    if w.is_visible().unwrap_or(false) { let _ = w.hide(); }
                    else { let _ = w.show(); let _ = w.set_focus(); }
                }
            }
        })
        .build(&app)
        .expect("tray");

    // 真正退出(Cmd+Q / 托盘退出)时还原配置;登录记录保留，下次启动自动重新接管
    // ExitRequested 与 Exit 双挂:Cmd+Q 在 macOS 上可能只触发其中之一
    app.run(|_handle, event| {
        match event {
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => {
                let _ = restore_files();
            }
            _ => {}
        }
    });
}
