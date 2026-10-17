#!/usr/bin/env python3
"""mm-admin:猫猫管理端的保存通道。

POST /mm-admin/app-config  保存客户端模型配置(需 new-api 管理员 Bearer)
GET  /mm-admin/app-config  读取当前配置

鉴权:请求头 Authorization: Bearer <new-api access_token>,本服务转发到
new-api /api/user/self 验证 role >= 10(管理员),不落地任何凭证。
"""
import json
import os
import tempfile
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

NEWAPI = "http://127.0.0.1:3000"
OUT_PATH = "/opt/binaryworld-pages/app-config.json"
PORT = 5090
MAX_BODY = 64 * 1024


def is_admin(token: str) -> bool:
    req = urllib.request.Request(
        NEWAPI + "/api/user/self",
        headers={"Authorization": "Bearer " + token},
    )
    try:
        with urllib.request.urlopen(req, timeout=8) as r:
            d = json.loads(r.read().decode())
            return bool(d.get("success")) and int(d.get("data", {}).get("role", 0)) >= 10
    except Exception:
        return False


def atomic_write(path: str, text: str) -> None:
    d = os.path.dirname(path)
    fd, tmp = tempfile.mkstemp(dir=d, prefix=".app-config-", suffix=".tmp")
    try:
        with os.fdopen(fd, "w") as f:
            f.write(text)
        os.replace(tmp, path)
        os.chmod(path, 0o644)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


NEWAPI_DB = "/opt/new-api/data/one-api.db"


def db_conn():
    import sqlite3
    return sqlite3.connect("file:" + NEWAPI_DB + "?mode=ro", uri=True, timeout=5)


def user_of_key(raw_key: str):
    """raw_key: 不带 sk- 前缀的令牌 key。返回 (user_id, token_row) 或 None。"""
    import sqlite3
    try:
        with db_conn() as db:
            row = db.execute(
                "SELECT id, user_id, `key`, status FROM tokens WHERE `key`=? AND deleted_at IS NULL",
                (raw_key,),
            ).fetchone()
            return row  # (id, user_id, key, status)
    except Exception:
        return None


def mask_key(k: str) -> str:
    return (k[:12] + "…" + k[-4:]) if len(k) > 20 else k


class H(BaseHTTPRequestHandler):
    def _send(self, code: int, obj) -> None:
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _bearer_key(self):
        auth = self.headers.get("Authorization", "")
        raw = auth[7:].strip() if auth.startswith("Bearer ") else ""
        if not raw:
            import urllib.parse
            raw = urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query).get("key", [""])[0]
        return raw[3:] if raw.startswith("sk-") else raw

    def do_GET(self):
        if self.path.split("?")[0] == "/mm-admin/usage":
            auth = self.headers.get("Authorization", "")
            token = auth[7:].strip() if auth.startswith("Bearer ") else ""
            uid = None
            if token:
                try:
                    req = urllib.request.Request(NEWAPI + "/api/user/self", headers={"Authorization": "Bearer " + token})
                    with urllib.request.urlopen(req, timeout=8) as r:
                        d = json.loads(r.read().decode())
                    if d.get("success") and d.get("data"):
                        uid = int(d["data"].get("id", 0))
                except Exception:
                    uid = None
            if not uid:
                self._send(403, {"success": False, "message": "请先登录"}); return
            import time as _t
            now = int(_t.time())
            since = now - 7 * 86400
            today0 = int(_t.mktime(_t.strptime(_t.strftime("%Y-%m-%d"), "%Y-%m-%d")))
            try:
                with db_conn() as db:
                    raw_daily = dict()
                    for (d, c, q) in db.execute(
                        "SELECT strftime('%Y-%m-%d', created_at, 'unixepoch', 'localtime') d, COUNT(*), SUM(quota) "
                        "FROM logs WHERE type=2 AND user_id=? AND created_at>=? GROUP BY d ORDER BY d", (uid, since)):
                        raw_daily[d] = (c, q or 0)
                    import datetime as _dt
                    daily = []
                    for i in range(7, -1, -1):
                        day = (_dt.date.today() - _dt.timedelta(days=i)).strftime("%Y-%m-%d")
                        c, q = raw_daily.get(day, (0, 0))
                        daily.append((day[5:], c, q))
                    by_model = db.execute(
                        "SELECT model_name, COUNT(*), SUM(quota) FROM logs WHERE type=2 AND user_id=? AND created_at>=? "
                        "GROUP BY model_name ORDER BY SUM(quota) DESC LIMIT 6", (uid, since)).fetchall()
                    t = db.execute("SELECT COUNT(*), COALESCE(SUM(quota),0) FROM logs WHERE type=2 AND user_id=? AND created_at>=?",
                                   (uid, today0)).fetchone()
                    tot = db.execute("SELECT COUNT(*), COALESCE(SUM(quota),0) FROM logs WHERE type=2 AND user_id=? AND created_at>=?",
                                     (uid, since)).fetchone()
                self._send(200, {"success": True, "data": {
                    "today": {"calls": t[0], "usd": t[1] / 500000},
                    "week": {"calls": tot[0], "usd": tot[1] / 500000},
                    "daily": [{"day": d, "calls": c, "usd": (q or 0) / 500000} for (d, c, q) in daily],
                    "by_model": [{"name": n or "—", "calls": c, "usd": (q or 0) / 500000} for (n, c, q) in by_model],
                }})
            except Exception as e:
                self._send(500, {"success": False, "message": str(e)})
            return
        if self.path.split("?")[0] == "/mm-admin/tokens":
            raw = self._bearer_key()
            row = user_of_key(raw) if raw else None
            if not row or row[3] != 1:
                self._send(403, {"success": False, "message": "Key 无效或已停用"}); return
            try:
                with db_conn() as db:
                    items = db.execute(
                        "SELECT id, name, `key`, status, model_limits_enabled, model_limits FROM tokens "
                        "WHERE user_id=? AND deleted_at IS NULL ORDER BY id",
                        (row[1],),
                    ).fetchall()
                self._send(200, {"success": True, "data": [
                    {"id": i, "name": n, "key_masked": "sk-" + mask_key(k), "status": st,
                     "model_limits_enabled": bool(mle), "model_limits": ml or ""}
                    for (i, n, k, st, mle, ml) in items]})
            except Exception as e:
                self._send(500, {"success": False, "message": str(e)})
            return
        if self.path.split("?")[0] == "/mm-admin/balance":
            raw = self._bearer_key()
            row = user_of_key(raw) if raw else None
            if not row or row[3] != 1:
                self._send(403, {"success": False, "message": "Key 无效或已停用"}); return
            try:
                with db_conn() as db:
                    q, u = db.execute("SELECT quota, used_quota FROM users WHERE id=?", (row[1],)).fetchone()
                self._send(200, {"success": True, "data": {"quota_usd": (q or 0) / 500000, "used_usd": (u or 0) / 500000}})
            except Exception as e:
                self._send(500, {"success": False, "message": str(e)})
            return
        if self.path.split("?")[0] == "/mm-admin/stats":
            import urllib.parse as _up
            auth = self.headers.get("Authorization", "")
            token = auth[7:].strip() if auth.startswith("Bearer ") else ""
            if not token or not is_admin(token):
                self._send(403, {"success": False, "message": "需要管理员登录"}); return
            days = 7
            try:
                q = _up.parse_qs(_up.urlparse(self.path).query)
                days = max(1, min(4000, int(q.get("days", ["7"])[0])))
            except Exception:
                pass
            import time as _t
            now = int(_t.time())
            since = now - days * 86400
            today0 = int(_t.mktime(_t.strptime(_t.strftime("%Y-%m-%d"), "%Y-%m-%d")))
            try:
                with db_conn() as db:
                    daily = db.execute(
                        "SELECT strftime('%Y-%m-%d', created_at, 'unixepoch', 'localtime') d, COUNT(*), SUM(quota) "
                        "FROM logs WHERE type=2 AND created_at>=? GROUP BY d ORDER BY d", (since,)).fetchall()
                    by_model = db.execute(
                        "SELECT model_name, COUNT(*), SUM(quota) FROM logs WHERE type=2 AND created_at>=? "
                        "GROUP BY model_name ORDER BY SUM(quota) DESC LIMIT 8", (since,)).fetchall()
                    by_channel = db.execute(
                        "SELECT channel_name, COUNT(*), SUM(quota) FROM logs WHERE type=2 AND created_at>=? "
                        "GROUP BY channel_name ORDER BY SUM(quota) DESC LIMIT 8", (since,)).fetchall()
                    t = db.execute(
                        "SELECT COUNT(*), COALESCE(SUM(quota),0), COUNT(DISTINCT user_id) FROM logs "
                        "WHERE type=2 AND created_at>=?", (today0,)).fetchone()
                    total = db.execute(
                        "SELECT COUNT(*), COALESCE(SUM(quota),0), COUNT(DISTINCT user_id) FROM logs "
                        "WHERE type=2 AND created_at>=?", (since,)).fetchone()
                self._send(200, {"success": True, "data": {
                    "days": days,
                    "today": {"calls": t[0], "usd": (t[1] or 0) / 500000, "users": t[2]},
                    "range": {"calls": total[0], "usd": (total[1] or 0) / 500000, "users": total[2]},
                    "daily": [{"day": d, "calls": c, "usd": (q or 0) / 500000} for (d, c, q) in daily],
                    "by_model": [{"name": n or "—", "calls": c, "usd": (q or 0) / 500000} for (n, c, q) in by_model],
                    "by_channel": [{"name": n or "—", "calls": c, "usd": (q or 0) / 500000} for (n, c, q) in by_channel],
                }})
            except Exception as e:
                self._send(500, {"success": False, "message": str(e)})
            return
        if self.path == "/mm-admin/app-config":
            try:
                with open(OUT_PATH) as f:
                    self._send(200, {"success": True, "data": json.load(f)})
            except FileNotFoundError:
                self._send(200, {"success": True, "data": None})
            except Exception as e:
                self._send(500, {"success": False, "message": str(e)})
            return
        self._send(404, {"success": False, "message": "not found"})

    def do_POST(self):
        if self.path == "/mm-admin/topup":
            auth = self.headers.get("Authorization", "")
            token = auth[7:].strip() if auth.startswith("Bearer ") else ""
            if not token or not is_admin(token):
                self._send(403, {"success": False, "message": "需要管理员登录"}); return
            try:
                import json as _json
                n = int(self.headers.get("Content-Length") or 0)
                body = _json.loads(self.rfile.read(n).decode()) if 0 < n <= 4096 else {}
                uid, usd = int(body.get("user_id", 0)), float(body.get("usd", 0))
                if uid <= 0 or usd == 0:
                    self._send(400, {"success": False, "message": "参数无效"}); return
            except Exception:
                self._send(400, {"success": False, "message": "请求无效"}); return
            import sqlite3
            try:
                db = sqlite3.connect(NEWAPI_DB, timeout=8)
                cur = db.execute(
                    "UPDATE users SET quota = MAX(0, quota + ?) WHERE id = ?",
                    (int(round(usd * 500000)), uid))
                db.commit()
                changed = cur.rowcount
                row = db.execute("SELECT quota FROM users WHERE id=?", (uid,)).fetchone()
                db.close()
                if not changed:
                    self._send(404, {"success": False, "message": "用户不存在"}); return
                self._send(200, {"success": True, "data": {"quota_usd": (row[0] or 0) / 500000}})
            except Exception as e:
                self._send(500, {"success": False, "message": str(e)})
            return
        if self.path == "/mm-admin/token-key":
            raw = self._bearer_key()
            row = user_of_key(raw) if raw else None
            if not row or row[3] != 1:
                self._send(403, {"success": False, "message": "Key 无效或已停用"}); return
            try:
                import json as _json
                n = int(self.headers.get("Content-Length") or 0)
                body = _json.loads(self.rfile.read(n).decode()) if 0 < n <= 4096 else {}
                tid = int(body.get("id", 0))
            except Exception:
                self._send(400, {"success": False, "message": "请求无效"}); return
            try:
                with db_conn() as db:
                    t = db.execute(
                        "SELECT user_id, `key`, status FROM tokens WHERE id=? AND deleted_at IS NULL", (tid,)
                    ).fetchone()
                if not t or t[0] != row[1] or t[2] != 1:
                    self._send(403, {"success": False, "message": "令牌不存在或不可用"}); return
                self._send(200, {"success": True, "data": {"key": "sk-" + t[1]}})
            except Exception as e:
                self._send(500, {"success": False, "message": str(e)})
            return
        if self.path != "/mm-admin/app-config":
            self._send(404, {"success": False, "message": "not found"})
            return
        auth = self.headers.get("Authorization", "")
        token = auth[7:].strip() if auth.startswith("Bearer ") else ""
        if not token or not is_admin(token):
            self._send(403, {"success": False, "message": "需要管理员登录(凭证无效或已过期)"})
            return
        n = int(self.headers.get("Content-Length") or 0)
        if n <= 0 or n > MAX_BODY:
            self._send(400, {"success": False, "message": "body 大小非法"})
            return
        try:
            cfg = json.loads(self.rfile.read(n).decode())
            if not isinstance(cfg, dict):
                raise ValueError("配置必须是对象")
            for k in ("codex", "grok", "full"):
                v = cfg.get(k)
                if not isinstance(v, list) or not all(isinstance(x, str) and x for x in v) or not v:
                    raise ValueError(f"{k} 必须是非空字符串数组")
            for k in ("codex_default", "grok_default", "full_default", "claude_main", "claude_small"):
                v = cfg.get(k)
                if not isinstance(v, str) or not v:
                    raise ValueError(f"{k} 不能为空")
            atomic_write(OUT_PATH, json.dumps(cfg, ensure_ascii=False, indent=2))
            self._send(200, {"success": True, "data": cfg})
        except Exception as e:
            self._send(400, {"success": False, "message": f"配置无效:{e}"})


if __name__ == "__main__":
    ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
