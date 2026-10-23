#!/bin/bash
# 猫猫启动器发布脚本(收存于仓库,不怕 /tmp 清空)
# 用法:  VER=1.5.7 bash scripts/release.sh
#
# 流程: Mac 本地构建(lproj 走 resources、ad-hoc 由 tauri 完成、产物含更新签名)
#   → Windows CI → dmg/exe/zip/tar.gz + latest.json 上传服务器 → 页面链接更新 → latest 别名刷新
#
# 依赖:
#   - ~/.tauri/maomao-updater.key          更新签名私钥(丢失则自动更新全线失效,妥善备份)
#   - GitHub Secrets TAURI_SIGNING_PRIVATE_KEY / _PASSWORD(CI 签 Windows 产物)
#   - ssh root@binaryworld.world 免密;gh 已登录
#   - PATH 里要有 rustup 1.95 工具链与 pnpm(脚本内已自动前置)
set -e
set -e
VER="${VER:?set VER=1.5.6}"
export PATH="$HOME/.rustup/toolchains/1.95-x86_64-apple-darwin/bin:/opt/homebrew/bin:$PATH"
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/maomao-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
cd ~/projects/maomao-launcher

echo "=== Mac build ==="
if ! COREPACK_INTEGRITY_KEYS=0 pnpm tauri build --target aarch64-apple-darwin > "/tmp/macbuild-$VER.log" 2>&1; then
  echo "MAC BUILD FAILED"; grep -E "error" -A 5 "/tmp/macbuild-$VER.log" | head -25; exit 1
fi
grep -E "Finished" "/tmp/macbuild-$VER.log" | head -3
BUNDLE="src-tauri/target/aarch64-apple-darwin/release/bundle"
APP=$(find "$BUNDLE/macos" -maxdepth 1 -name "*.app" | head -1)
DMG=$(find "$BUNDLE/dmg" -name "*.dmg" | head -1)
TAR=$(find "$BUNDLE/macos" -maxdepth 1 -name "*.app.tar.gz" | head -1)
TAR_SIG="$TAR.sig"
test -f "$DMG" && test -f "$TAR" && test -f "$TAR_SIG"
cp "$DMG" "/tmp/Maomao-Token-$VER-mac-arm64.dmg"
cp "$TAR" "/tmp/Maomao-Token-$VER-mac-arm64.app.tar.gz"
cp "$TAR_SIG" /tmp/maomao-mac.tar.gz.sig
rm -rf "/Applications/MaoMao Token.app" "/Applications/MaoMao Power.app" && cp -R "$APP" /Applications/
echo "MAC DONE (dmg + tar.gz + updater sig)"

echo "=== windows CI ==="
sleep 15
RID=$(gh run list --workflow=build-windows.yml --limit 1 --json databaseId --jq '.[0].databaseId')
echo "run $RID"
if ! gh run watch "$RID" --exit-status >/dev/null 2>&1; then
  echo "CI FAILED"; gh run view "$RID" --log-failed 2>&1 | grep -iE "error" | head -20; exit 1
fi
rm -rf "/tmp/win-dist-$VER" && mkdir -p "/tmp/win-dist-$VER"
gh run download "$RID" -D "/tmp/win-dist-$VER"
cp "$(find "/tmp/win-dist-$VER/maomao-windows" -name "*_x64-setup.exe" | head -1)" "/tmp/Maomao-Token-$VER-win-x64.exe"
cp "$(find "/tmp/win-dist-$VER/maomao-windows" -name "*_x64-setup.exe.sig" | head -1)" /tmp/maomao-win.exe.sig
ZIP=$(find "/tmp/win-dist-$VER" -name "Maomao-Token-$VER-win-x64.zip" | head -1)
test -n "$ZIP" && test -f /tmp/maomao-win.exe.sig
echo "artifacts ok"

python3 - "$VER" <<'PYEOF'
import json, pathlib, sys, datetime
ver = sys.argv[1]
mac_sig = pathlib.Path("/tmp/maomao-mac.tar.gz.sig").read_text().strip()
win_sig = pathlib.Path("/tmp/maomao-win.exe.sig").read_text().strip()
B = "https://binaryworld.world"
manifest = {
    "version": ver,
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "platforms": {
        "darwin-aarch64": {"signature": mac_sig, "url": f"{B}/Maomao-Token-{ver}-mac-arm64.app.tar.gz"},
        "windows-x86_64": {"signature": win_sig, "url": f"{B}/Maomao-Token-{ver}-win-x64.exe"},
    },
}
pathlib.Path("/tmp/latest.json").write_text(json.dumps(manifest, ensure_ascii=False))
print("latest.json written for", ver)
PYEOF

cat > "/tmp/update-pages-$VER.py" <<'PYEOF'
import re, os, pathlib, sys
ver = sys.argv[1]
B = "/opt/binaryworld-pages"
def mb(n): return round(os.path.getsize(f"{B}/{n}") / 1048576, 1)
dmg_m = mb(f"Maomao-Token-{ver}-mac-arm64.dmg")
exe_m = mb(f"Maomao-Token-{ver}-win-x64.exe")
zip_m = mb(f"Maomao-Token-{ver}-win-x64.zip")
pat = re.compile(r"Maomao-Token-\d+\.\d+\.\d+-(mac-arm64\.dmg|win-x64\.exe|win-x64\.zip)")
for f in ["index.html", "embed.html", "panel.html", "docs.html"]:
    p = pathlib.Path(f"{B}/{f}"); s = p.read_text()
    s2 = pat.sub(lambda m: f"Maomao-Token-{ver}-{m.group(1)}", s)
    s2 = re.sub(r"(> macOS 版<small>)[\d.]+M", rf"\g<1>{dmg_m}M", s2)
    s2 = re.sub(r"(> Windows 版<small>)[\d.]+M", rf"\g<1>{exe_m}M", s2)
    s2 = re.sub(r"(> Windows zip 版<small>)[\d.]+M", rf"\g<1>{zip_m}M", s2)
    s2 = re.sub(r"(macOS 版)", rf"\g<1>", s2)
    if s2 != s: p.write_text(s2); print(f, f"updated dmg={dmg_m} exe={exe_m} zip={zip_m}")
    else: print(f, "no change")
PYEOF

scp -q "/tmp/Maomao-Token-$VER-win-x64.exe" "$ZIP" "/tmp/Maomao-Token-$VER-mac-arm64.dmg" "/tmp/Maomao-Token-$VER-mac-arm64.app.tar.gz" /tmp/latest.json root@binaryworld.world:/opt/binaryworld-pages/
scp -q "/tmp/update-pages-$VER.py" root@binaryworld.world:/tmp/update-pages-$VER.py
/usr/bin/ssh root@binaryworld.world "VER=$VER bash -s" <<'EOS'
grep -q " /latest.json" /etc/caddy/Caddyfile || sed -i "s| /Maomao-Token-1.2.1-win-x64.exe| /latest.json /Maomao-Token-1.2.1-win-x64.exe|" /etc/caddy/Caddyfile
grep -q " /Maomao-Token-$VER-win-x64.exe" /etc/caddy/Caddyfile || sed -i "s| /Maomao-Token-1.2.1-win-x64.exe| /Maomao-Token-1.2.1-win-x64.exe /Maomao-Token-$VER-mac-arm64.dmg /Maomao-Token-$VER-mac-arm64.app.tar.gz /Maomao-Token-$VER-win-x64.exe /Maomao-Token-$VER-win-x64.zip|" /etc/caddy/Caddyfile
caddy validate --config /etc/caddy/Caddyfile 2>&1 | tail -1
systemctl reload caddy
python3 /tmp/update-pages-$VER.py "$VER"
cd /opt/binaryworld-pages
ln -sf Maomao-Token-$VER-mac-arm64.dmg Maomao-Token-latest-mac-arm64.dmg
ln -sf Maomao-Token-$VER-win-x64.exe Maomao-Token-latest-win-x64.exe
ln -sf Maomao-Token-$VER-win-x64.zip Maomao-Token-latest-win-x64.zip
chmod 644 "Maomao-Token-$VER-mac-arm64.app.tar.gz" latest.json 2>/dev/null || true
sleep 1
for f in mac-arm64.dmg mac-arm64.app.tar.gz win-x64.exe win-x64.zip; do
  curl -s -o /dev/null -w "Maomao-Token-$VER-$f: %{http_code} %{content_type}\n" -r 0-0 "https://binaryworld.world/Maomao-Token-$VER-$f"
done
curl -s -o /dev/null -w "latest.json: %{http_code} %{content_type}\n" https://binaryworld.world/latest.json
# 网站仓库规矩:改了被跟踪文件(页面版本链接、latest.json)要在服务器上提交推送;产物已被 .gitignore 挡住
git add -A
git diff --cached --quiet || git commit -qm "release: v$VER 页面链接与更新清单" 
git push -q origin master || true
EOS
echo "RELEASE-$VER-DONE"
