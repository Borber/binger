# 发布流程

## GitHub Release

1. 更新 `Cargo.toml` 的版本号和 `Cargo.lock`，在 `CHANGELOG.md` 记录本次变更。
2. 提交并推送 `master`，确认 CI 通过后，从该提交创建并推送与版本号对应的 tag，例如 `v0.1.3`。
3. `.github/workflows/release.yml` 会构建 Linux x86_64 二进制并创建 GitHub Release，附上 `binger-<版本>-x86_64-unknown-linux-gnu.tar.gz` 和 `SHA256SUMS`。

Release 压缩包包含以下文件：

- `binger`
- `README.md`
- `CHANGELOG.md`
- `LICENSE`
- `config.example.toml`

## AUR

Release 创建后，将 `packaging/aur/binger-bin/PKGBUILD` 的 `pkgver` 更新为该版本，并将 `pkgrel` 设为 1。把工作流产出的压缩包和 `SHA256SUMS` 下载到同一个目录，运行 `sha256sum -c SHA256SUMS`，再将对应的 SHA256 写入 `sha256sums`。Release URL 随 `pkgver` 更新。

在 Arch Linux 上重新生成元数据并验证打包。

```bash
cd packaging/aur/binger-bin
makepkg --printsrcinfo > .SRCINFO
makepkg --verifysource
makepkg
```

将更新后的 `PKGBUILD` 和 `.SRCINFO` 提交到 AUR 的 `binger-bin` 仓库，推送后检查 AUR 页面上的版本号。GitHub 仓库的 `packaging/aur/binger-bin` 也保留同一份打包元数据。
