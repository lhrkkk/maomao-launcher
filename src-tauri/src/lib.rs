use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

const BASE_CLAUDE: &str = "https://binaryworld.world";
const BASE_OPENAI: &str = "https://binaryworld.world/v1";
const SITE: &str = "https://binaryworld.world";
const TOKEN_NAME: &str = "猫猫启动器";

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

/// 接管文件前备份原始版本(仅首次,保留用户最初状态)
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
}
fn p(rel: &[&str]) -> Result<PathBuf, String> {
    let mut path = home()?;
    for seg in rel { path.push(seg); }
    Ok(path)
}

#[cfg(target_os = "macos")]
fn which_in_login_shell(cmd: &str) -> bool {
    Command::new("zsh")
        .args(["-l", "-c", &format!("command -v {cmd}")])
        .output()
        .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn which_in_login_shell(cmd: &str) -> bool {
    use std::env;
    // GUI 进程 PATH 缺用户目录:where -> 常见安装位置 -> 注册表用户 PATH,三重查找
    let via_where = Command::new("cmd").args(["/C", &format!("where {cmd}")])
        .output()
        .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);
    if via_where { return true; }
    let mut dirs: Vec<PathBuf> = vec![];
    if let Ok(up) = env::var("USERPROFILE") {
        let up = PathBuf::from(up);
        dirs.push(up.join(".local").join("bin"));
        dirs.push(up.join(".codex").join("bin"));
        dirs.push(up.join(".claude").join("local"));
    }
    if let Ok(appdata) = env::var("APPDATA") { dirs.push(PathBuf::from(appdata).join("npm")); }
    if let Ok(local) = env::var("LOCALAPPDATA") {
        let l = PathBuf::from(local);
        dirs.push(l.join("Microsoft").join("WindowsApps"));
        dirs.push(l.join("Programs"));
    }
    if let Ok(out) = Command::new("powershell")
        .args(["-NoProfile", "-Command", "[Environment]::GetEnvironmentVariable('PATH','User')"])
        .output()
    {
        if out.status.success() {
            for seg in String::from_utf8_lossy(&out.stdout).split(';') {
                let seg = seg.trim();
                if !seg.is_empty() { dirs.push(PathBuf::from(seg)); }
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
    // 返回可启动的完整 exe 路径:覆盖 MSIX/NSIS/官方安装器三类位置
    let mut probes: Vec<PathBuf> = vec![];
    if let Ok(local) = env::var("LOCALAPPDATA") {
        let l = PathBuf::from(local);
        for name in candidates {
            probes.push(l.join("Microsoft").join("WindowsApps").join(format!("{name}.exe")));
            probes.push(l.join("Programs").join(name).join(format!("{name}.exe")));
            probes.push(l.join("Programs").join(format!("{name}.exe")));
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
    probes.into_iter().find(|p| p.exists()).map(|p| p.to_string_lossy().to_string())
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
        ("hermes", "Hermes Agent", Cli, false),
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
}

#[tauri::command]
fn get_state() -> AppState {
    let login = read_json(&state_path().unwrap_or_default()).and_then(|v| {
        let u = v.get("username")?.as_str()?.to_string();
        let k = v.get("key_masked")?.as_str()?.to_string();
        let full = v.get("key").and_then(|x| x.as_str()).unwrap_or("").to_string();
        Some(LoginInfo { username: u, key_masked: k, key: full })
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

fn apply_configs(key: &str, models: &[String]) -> Result<(), String> {
    // Claude Code
    merge_write_json({ let pp = p(&[".claude", "settings.json"])?; takeover_backup(&pp); pp }, |v| {
        if !v.get("env").map(|e| e.is_object()).unwrap_or(false) {
            v["env"] = serde_json::json!({});
        }
        v["env"]["ANTHROPIC_BASE_URL"] = serde_json::json!(BASE_CLAUDE);
        v["env"]["ANTHROPIC_AUTH_TOKEN"] = serde_json::json!(key);
    })?;

    // Codex:auth.json 整文件覆盖(纯 API Key 模式,清除 ChatGPT OAuth 残留)
    // 残留 auth_mode=chatgpt + tokens 会让 Codex 优先用 OAuth 令牌导致 401
    let codex_auth = { let pp = p(&[".codex", "auth.json"])?; takeover_backup(&pp); pp };
    if let Some(dir) = codex_auth.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("创建 ~/.codex 失败:{e}"))?;
    }
    fs::write(&codex_auth, serde_json::to_string_pretty(&serde_json::json!({
        "OPENAI_API_KEY": key
    })).unwrap_or_default()).map_err(|e| format!("写入 auth.json 失败:{e}"))?;
    let codex_model = models.iter().find(|m| m.as_str() == "glm-5.3")
        .or_else(|| models.first())
        .map(|s| s.as_str())
        .unwrap_or("glm-5.3");

    // 模型目录:用官方模板条目做骨架,每个模型生成一条
    let catalog_path = p(&[".codex", "maomao-model-catalog.json"])?;
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
        obj.insert("description".into(), serde_json::json!(format!("{m} · 猫猫 Token")));
        obj.insert("context_window".into(), serde_json::json!(131072));
        obj.insert("max_context_window".into(), serde_json::json!(131072));
        obj.insert("priority".into(), serde_json::json!(1000 + i));
        e
    }).collect();
    let catalog = serde_json::json!({ "models": entries });
    if let Some(dir) = catalog_path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("创建 ~/.codex 失败:{e}"))?;
    }
    fs::write(&catalog_path, serde_json::to_string_pretty(&catalog).unwrap_or_default())
        .map_err(|e| format!("写入模型目录失败:{e}"))?;

    write_toml({ let pp = p(&[".codex", "config.toml"])?; takeover_backup(&pp); pp }, &format!(
        "# Managed by 猫猫 Token Launcher\n\
         model_provider = \"maomao\"\n\
         model = \"{codex_model}\"\n\
         model_catalog_json = {}\n\
         model_reasoning_effort = \"high\"\n\
         disable_response_storage = true\n\n\
         [model_providers.maomao]\n\
         name = \"maomao\"\n\
         base_url = \"{BASE_OPENAI}\"\n\
         wire_api = \"responses\"\n\
         requires_openai_auth = true\n",
        serde_json::to_string(catalog_path.to_str().unwrap_or_default()).unwrap_or_default()
    ))?;

    // Grok Build
    write_toml({ let pp = p(&[".grok", "config.toml"])?; takeover_backup(&pp); pp }, &format!(
        "# Managed by 猫猫 Token Launcher\n\
         model_provider = \"maomao\"\n\
         model = \"grok-4.6\"\n\n\
         [model_providers.maomao]\n\
         name = \"maomao\"\n\
         base_url = \"{BASE_OPENAI}\"\n\
         wire_api = \"responses\"\n\
         requires_openai_auth = true\n"
    ))?;
    fs::write({ let pp = p(&[".grok", "auth.json"])?; takeover_backup(&pp); pp }, serde_json::to_string_pretty(&serde_json::json!({
        "OPENAI_API_KEY": key
    })).unwrap_or_default()).map_err(|e| format!("写入 auth.json 失败:{e}"))?;

    // OpenCode
    merge_write_json({ let pp = p(&[".config", "opencode", "opencode.json"])?; takeover_backup(&pp); pp }, |v| {
        if !v.get("provider").map(|e| e.is_object()).unwrap_or(false) {
            v["provider"] = serde_json::json!({});
        }
        let mut oc_models = serde_json::Map::new();
        for m in models {
            oc_models.insert(m.clone(), serde_json::json!({ "name": m }));
        }
        v["provider"]["maomao"] = serde_json::json!({
            "npm": "@ai-sdk/openai-compatible",
            "name": "猫猫 Token",
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
            models.iter().map(|m| serde_json::json!({ "id": m, "name": m })).collect(),
        );
    })?;

    Ok(())
}

#[tauri::command]
fn save_key(key: String) -> Result<(), String> {
    let key = key.trim().to_string();
    if key.is_empty() { return Err("API Key 不能为空".into()); }
    let models = fetch_models();
    apply_configs(&key, &models)
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

#[tauri::command]
fn login_and_setup(username: String, password: String) -> Result<String, String> {
    if username.trim().is_empty() || password.is_empty() {
        return Err("请输入账号和密码".into());
    }
    let client = http_client().map_err(|e| format!("网络初始化失败:{e}"))?;

    // 1) 登录:该版本 new-api 在响应 JSON 里返回 access_token(非 cookie 会话)
    let resp: serde_json::Value = client
        .post(format!("{SITE}/api/user/login"))
        .json(&serde_json::json!({ "username": username.trim(), "password": password }))
        .send().map_err(|e| format!("连接站点失败:{e}"))?
        .json().map_err(|_| "站点响应异常".to_string())?;
    if !api_success(&resp) {
        return Err(format!("登录失败:{}", api_error(&resp)));
    }
    let token = resp.pointer("/data/access_token")
        .and_then(|t| t.as_str())
        .filter(|t| !t.is_empty())
        .ok_or("账号开启了二次验证或需要额外验证,请改用「直接填 Key」方式")?
        .to_string();
    let auth_header = format!("Bearer {token}");

    // 2) 查询已有同名令牌:有则复用,没有才创建
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
    let id = id.ok_or("未找到令牌,请到控制台手动复制 Key")?;

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

    let models = fetch_models();
    apply_configs(&key, &models)?;
    let mask = if key.len() > 13 { format!("{}…{}", &key[..10], &key[key.len()-4..]) } else { key.clone() };
    let sp = state_path()?;
    if let Some(dir) = sp.parent() { fs::create_dir_all(dir).ok(); }
    fs::write(&sp, serde_json::to_string_pretty(&serde_json::json!({
        "username": username.trim(),
        "key_masked": mask,
        "key": key,
    })).unwrap_or_default()).map_err(|e| format!("写入登录记录失败:{e}"))?;
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

/// 恢复所有被接管的文件到最初状态(有备份还原,无备份删除)
fn restore_files() -> Result<(), String> {
    for rel in [
        (".claude", "settings.json"),
        (".codex", "auth.json"),
        (".codex", "config.toml"),
        (".grok", "auth.json"),
        (".grok", "config.toml"),
        (".openclaw", "openclaw.config.json"),
    ] {
        if let Ok(pp) = p(&[rel.0, rel.1]) { restore_or_remove(&pp); }
    }
    restore_or_remove(&p(&[".config", "opencode", "opencode.json"])?);
    let _ = fs::remove_file(p(&[".codex", "maomao-model-catalog.json"])?);
    Ok(())
}

/// 退出账号:还原配置 + 忘掉登录记录
#[tauri::command]
fn logout() -> Result<(), String> {
    restore_files()?;
    let _ = fs::remove_file(state_path()?);
    Ok(())
}

// 在 App 内置浏览器窗口打开站点页面(充值/控制台/注册)
#[tauri::command]
fn open_site_page(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri::Manager;
    let parsed: tauri::Url = url.parse().map_err(|e| format!("地址无效:{e}"))?;
    if let Some(w) = app.get_webview_window("site") {
        let js = format!("location.href = '{}'", parsed.as_str().replace('\'', ""));
        let _ = w.eval(&js);
        let _ = w.set_focus();
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(&app, "site", tauri::WebviewUrl::External(parsed))
        .title("猫猫 Token")
        .inner_size(1080.0, 760.0)
        .min_inner_size(640.0, 520.0)
        .build()
        .map_err(|e| format!("打开窗口失败:{e}"))?;
    Ok(())
}

// ---------- 启动与打开 ----------

#[tauri::command]
fn launch(id: String) -> Result<(), String> {
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
            let out = Command::new("cmd").args(["/C", &format!("start \"\" \"{app}\"")]).output()
                .map_err(|e| format!("启动失败:{e}"))?;
            if !out.status.success() {
                return Err(format!("启动 {app} 失败:{}", String::from_utf8_lossy(&out.stderr)));
            }
            Ok(())
        }
        ToolKind::Cli => {
            if !which_in_login_shell(spec.0) {
                return Err(format!("{} 未安装,请先参考官方文档安装", spec.1));
            }
            #[cfg(target_os = "macos")]
            let out = Command::new("osascript").args(["-e", &format!(
                "tell application \"Terminal\" to activate\ntell application \"Terminal\" to do script \"{}\"",
                spec.0
            )]).output().map_err(|e| format!("打开终端失败:{e}"))?;
            #[cfg(target_os = "windows")]
            let cmd_name = spec.0;
            #[cfg(target_os = "windows")]
            let out = Command::new("cmd")
                .args(["/C", &format!("start \"猫猫 {cmd_name}\" cmd /K {cmd_name}")])
                .output()
                .map_err(|e| format!("打开终端失败:{e}"))?;
            if !out.status.success() {
                return Err(format!("启动失败:{}", String::from_utf8_lossy(&out.stderr)));
            }
            Ok(())
        }
    }
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let out = Command::new("open").arg(&url).output().map_err(|e| format!("打开失败:{e}"))?;
    #[cfg(target_os = "windows")]
    let out = Command::new("cmd").args(["/C", &format!("start \"\" \"{url}\"")]).output().map_err(|e| format!("打开失败:{e}"))?;
    let _ = out;
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
        .invoke_handler(tauri::generate_handler![get_state, save_key, login_and_setup, logout, launch, open_url, open_site_page])
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
        .tooltip("猫猫 Token")
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

    // 真正退出(Cmd+Q / 托盘退出)时还原配置;登录记录保留,下次启动自动重新接管
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
