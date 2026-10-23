#!/bin/bash
# 猫猫启动器发布脚本(收存于仓库,不怕 /tmp 清空)
# 用法:  VER=1.5.7 bash scripts/release.sh
#
# 流程: Mac 本地构建(lproj 走 resources、ad-hoc 由 tauri 完成、产物含更新签名)
#   → Windows CI → dmg/exe/tar.gz + latest.json 上传服务器 → 页面链接更新 → latest 别名刷新
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
if [ -n "$SKIP_MAC" ]; then
  # 复用 /tmp 里已验证的同版本产物(Windows-only 修复重发时省 10-20 分钟)
  test -s "/tmp/Maomao-Token-$VER-mac-arm64.dmg" && test -s "/tmp/Maomao-Token-$VER-mac-arm64.app.tar.gz" && test -s /tmp/maomao-mac.tar.gz.sig
  echo "SKIP_MAC: 复用 /tmp 现有 $VER Mac 产物"
else
# 先清空 bundle 目录:改名前残留的旧产物曾被 find 误拾,导致 1.5.17-1.5.20 的
# 更新包(dmg 除外)内部全是 1.5.15 内容、更新器报签名版本不匹配(2026-10-24 事故)
rm -rf src-tauri/target/aarch64-apple-darwin/release/bundle
if ! COREPACK_INTEGRITY_KEYS=0 pnpm tauri build --target aarch64-apple-darwin > "/tmp/macbuild-$VER.log" 2>&1; then
  echo "MAC BUILD FAILED"; grep -E "error" -A 5 "/tmp/macbuild-$VER.log" | head -25; exit 1
fi
grep -E "Finished" "/tmp/macbuild-$VER.log" | head -3
BUNDLE="src-tauri/target/aarch64-apple-darwin/release/bundle"
# ls -d 只列名字本身(.app 是目录,不加 -d 会列出其内容);-t 按 mtime 取最新
APP=$(ls -dt "$BUNDLE/macos/"*.app | head -1)
TAR=$(ls -dt "$BUNDLE/macos/"*.app.tar.gz | head -1)
DMG=$(ls -dt "$BUNDLE/dmg/"*.dmg | head -1)
TAR_SIG="$TAR.sig"
test -f "$DMG" && test -f "$TAR" && test -f "$TAR_SIG"
# 产物版本断言:内部版本必须等于 $VER,不符立即中止(原子发布,杜绝旧包再混上服务器)
PLIST=$(tar tzf "$TAR" | grep 'Contents/Info.plist$' | head -1)
TARVER=$( [ -n "$PLIST" ] && tar xzOf "$TAR" "$PLIST" 2>/dev/null | plutil -extract CFBundleShortVersionString raw - 2>/dev/null )
if [ "$TARVER" != "$VER" ]; then
  echo "ABORT: 更新包内部版本 '$TARVER' != $VER(产物拿错,拒绝上传)"; exit 1
fi
MNT="/tmp/dmgchk-$VER"; rm -rf "$MNT"
hdiutil attach -nobrowse -quiet "$DMG" -mountpoint "$MNT"
DMGVER=$(plutil -extract CFBundleShortVersionString raw "$MNT/"*.app/Contents/Info.plist 2>/dev/null || echo "?")
hdiutil detach -quiet "$MNT"
if [ "$DMGVER" != "$VER" ]; then
  echo "ABORT: dmg 内部版本 $DMGVER != $VER(产物拿错,拒绝上传)"; exit 1
fi
cp "$DMG" "/tmp/Maomao-Token-$VER-mac-arm64.dmg"
cp "$TAR" "/tmp/Maomao-Token-$VER-mac-arm64.app.tar.gz"
cp "$TAR_SIG" /tmp/maomao-mac.tar.gz.sig
rm -rf "/Applications/MaoMao Token.app" "/Applications/MaoMao Power.app" && cp -R "$APP" /Applications/
echo "MAC DONE (dmg + tar.gz + updater sig, versions verified: $TARVER/$DMGVER)"
fi

echo "=== windows CI ==="
sleep 15
RID=$(gh run list --workflow=build-windows.yml --limit 1 --json databaseId --jq '.[0].databaseId')
echo "run $RID"
if ! gh run watch "$RID" --exit-status >/dev/null 2>&1; then
  echo "CI FAILED"; gh run view "$RID" --log-failed 2>&1 | grep -iE "error" | head -20; exit 1
fi
rm -rf "/tmp/win-dist-$VER" && mkdir -p "/tmp/win-dist-$VER"
gh run download "$RID" -D "/tmp/win-dist-$VER"
# 产物文件名自带版本号,按 $VER 精确匹配,防误拾
cp "$(find "/tmp/win-dist-$VER" -name "*${VER}*_x64-setup.exe" | head -1)" "/tmp/Maomao-Token-$VER-win-x64.exe"
cp "$(find "/tmp/win-dist-$VER" -name "*${VER}*_x64-setup.exe.sig" | head -1)" /tmp/maomao-win.exe.sig
test -f /tmp/maomao-win.exe.sig && test -s "/tmp/Maomao-Token-$VER-win-x64.exe"
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
pat = re.compile(r"Maomao-Token-\d+\.\d+\.\d+-(mac-arm64\.dmg|win-x64\.exe)")
for f in ["index.html", "embed.html", "panel.html", "docs.html"]:
    p = pathlib.Path(f"{B}/{f}"); s = p.read_text()
    s2 = pat.sub(lambda m: f"Maomao-Token-{ver}-{m.group(1)}", s)
    s2 = re.sub(r"(> macOS 版<small>)[\d.]+M", rf"\g<1>{dmg_m}M", s2)
    s2 = re.sub(r"(> Windows 版<small>)[\d.]+M", rf"\g<1>{exe_m}M", s2)
    s2 = re.sub(r"(macOS 版)", rf"\g<1>", s2)
    if s2 != s: p.write_text(s2); print(f, f"updated dmg={dmg_m} exe={exe_m}")
    else: print(f, "no change")
PYEOF

scp -q "/tmp/Maomao-Token-$VER-win-x64.exe" "/tmp/Maomao-Token-$VER-mac-arm64.dmg" "/tmp/Maomao-Token-$VER-mac-arm64.app.tar.gz" /tmp/latest.json root@binaryworld.world:/opt/binaryworld-pages/
scp -q "/tmp/update-pages-$VER.py" root@binaryworld.world:/tmp/update-pages-$VER.py
/usr/bin/ssh root@binaryworld.world "VER=$VER bash -s" <<'EOS'
grep -q " /latest.json" /etc/caddy/Caddyfile || sed -i "s| /Maomao-Token-1.2.1-win-x64.exe| /latest.json /Maomao-Token-1.2.1-win-x64.exe|" /etc/caddy/Caddyfile
grep -q " /Maomao-Token-$VER-win-x64.exe" /etc/caddy/Caddyfile || sed -i "s| /Maomao-Token-1.2.1-win-x64.exe| /Maomao-Token-1.2.1-win-x64.exe /Maomao-Token-$VER-mac-arm64.dmg /Maomao-Token-$VER-mac-arm64.app.tar.gz /Maomao-Token-$VER-win-x64.exe|" /etc/caddy/Caddyfile
caddy validate --config /etc/caddy/Caddyfile 2>&1 | tail -1
systemctl reload caddy
python3 /tmp/update-pages-$VER.py "$VER"
cd /opt/binaryworld-pages
ln -sf Maomao-Token-$VER-mac-arm64.dmg Maomao-Token-latest-mac-arm64.dmg
ln -sf Maomao-Token-$VER-win-x64.exe Maomao-Token-latest-win-x64.exe
chmod 644 "Maomao-Token-$VER-mac-arm64.app.tar.gz" latest.json 2>/dev/null || true
sleep 1
for f in mac-arm64.dmg mac-arm64.app.tar.gz win-x64.exe; do
  curl -s -o /dev/null -w "Maomao-Token-$VER-$f: %{http_code} %{content_type}\n" -r 0-0 "https://binaryworld.world/Maomao-Token-$VER-$f"
done
curl -s -o /dev/null -w "latest.json: %{http_code} %{content_type}\n" https://binaryworld.world/latest.json
# 网站仓库规矩:改了被跟踪文件(页面版本链接、latest.json)要在服务器上提交推送;产物已被 .gitignore 挡住
git add -A
git diff --cached --quiet || git commit -qm "release: v$VER 页面链接与更新清单" 
git push -q origin master || true
EOS
echo "RELEASE-$VER-DONE"
