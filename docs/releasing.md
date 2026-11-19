# 发布流程

## GitHub Release

1. 在 GitHub 创建公开仓库 `borber/binger`，并将本地 `master` 推送到该仓库。
2. 在确认工作区和检查结果后创建 `v0.1.0` tag 并推送。
3. `.github/workflows/release.yml` 会构建 Linux x86_64 二进制，生成压缩包和 `SHA256SUMS`，再创建 GitHub Release。

Release 压缩包包含以下文件：

- `binger`
- `README.md`
- `CHANGELOG.md`
- `LICENSE`
- `config.example.toml`

## AUR

Release 创建后，从对应 Release 的 `SHA256SUMS` 复制压缩包校验和，替换 `packaging/aur/binger-bin/PKGBUILD` 中的 `SKIP`，然后在该目录重新生成 `.SRCINFO`：

```bash
cd packaging/aur/binger-bin
makepkg --printsrcinfo > .SRCINFO
makepkg --verifysource
```

确认 `makepkg -si` 可以安装后，将 `PKGBUILD` 和 `.SRCINFO` 提交到 AUR 的 `binger-bin` 仓库。后续版本只需更新 `pkgver`、Release URL 和校验和，再重新生成 `.SRCINFO`。
