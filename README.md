# 猫猫 Token 桌面客户端

桌面启动器。登录、接管本机 AI 客户端配置、检查更新。

源码只包含 App。网页和控制台后端不在这个仓库里。

## 仓库分工

| 内容 | 仓库 | 位置 |
|---|---|---|
| 本 App | `lhrkkk/maomao-launcher` | 本目录 |
| 公开网站、控制台页面、`mm-admin.py` | `lhrkkk/binaryworld-pages` | 服务器 `/opt/binaryworld-pages` |

不要把 `panel.html`、`admin.html`、`mm-admin.py` 再拷回这里。线上用的是网站仓库里的那一份，这里留副本会过期，部署时会覆盖错文件。

## 构建

```bash
export PATH="$HOME/.rustup/toolchains/1.95-x86_64-apple-darwin/bin:$PATH"
COREPACK_INTEGRITY_KEYS=0 pnpm tauri build --target aarch64-apple-darwin
```

产物在 `src-tauri/target/aarch64-apple-darwin/release/bundle/`。`bundle_dmg.sh` 失败是预期情况，用 `codesign --force --deep -s -` 签名后再 `hdiutil` 打包。

Windows 包由 GitHub Actions 构建（`.github/workflows/`）。推送必须走 `git@github.com:lhrkkk/maomao-launcher.git`，`gh` 的 HTTPS 推送没有 workflow 权限。
