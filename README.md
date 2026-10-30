# 轻舟 Linklane

A compact Windows desktop tool to open external links with the right Google Chrome identity.

轻舟（Linklane）是专为 Windows x64 打造的轻巧工具。日常在 Slack、VSCode 或邮件中点击链接时，网页常常会在错误的 Google Chrome 身份中打开。轻舟作为系统默认的 HTTP/HTTPS 处理程序，自动发现当前已有的 Chrome 身份。点击外部链接时，轻舟会弹出贴合 Chrome 风格的紧凑无边框窗口，点击对应身份即可直接打开。

![轻舟选择界面](docs/images/chooser.png)
*界面截图仅包含演示名称。*

## 功能特点

- 简洁轻巧的交互：顶部区域支持拖拽移动，具备最小化与关闭按钮，按 Esc 键直接退出。每个外部链接使用独立窗口。
- 自动记忆网站：默认勾选“下次也用这个身份打开此网站”。轻舟仅记住精确的协议、域名与端口，不匹配子域名或路径。再次打开该网站时会自动直达。
- 随时灵活调整：在“已记住的网站”中点击“忘记”，下次会重新询问；开启“每次都让我选”可临时暂停自动打开。
- 身份排序与重命名：支持在应用内为身份重命名。最近使用的身份优先展示（含自动打开），跨重启保存；初始顺序与头像颜色读取自 Chrome。
- 保护隐私数据：仅读取 Chrome 身份元数据，不读取 Cookie、密码或浏览历史；本地仅记录网站来源与身份，不保存完整网址、查询参数或令牌。仅本地运行，无后台常驻服务。

## 下载与使用

前往 [GitHub Releases](https://github.com/pluveto/linklane/releases/latest) 获取。发布文件包括 `linklane-v0.1.0-windows-x86_64.zip` 与 `linklane.exe`。推荐使用 ZIP 压缩包（包含主程序、README、LICENSE、THIRD_PARTY_LICENSES.html 与 uninstall.ps1）。

> 说明：v0.1.0 暂无代码签名证书。

1. 解压 ZIP 压缩包，运行 `linklane.exe`。
2. 点击“开始使用”，程序将自动安装至 `%LOCALAPPDATA%/Linklane/bin` 并完成系统注册。配置文件保存在 `%LOCALAPPDATA%/Linklane/settings.json`。无需管理员权限。
3. 在 Windows“默认应用”设置中搜索“轻舟 Linklane”，手动将其设为默认浏览器（同时设置 HTTP 与 HTTPS）。

## 支持范围与限制

- 系统与浏览器：仅支持 Windows x64 及常规安装位置的标准 Google Chrome。暂不支持 macOS 或 Linux，无浏览器扩展。
- 外部与内部链接：内置网页视图（Webview）、强制指定其他浏览器的链接及 Chrome 内部页面跳转均不经过轻舟。
- 匹配机制：不支持通配符、路径分级或按来源应用区分。
- 更新机制：目前不含自动更新功能。

## 卸载

1. 在 Windows 默认应用设置中将默认浏览器切换回 Google Chrome。
2. 关闭轻舟。
3. 运行随附的 `uninstall.ps1` 脚本移除注册项与安装文件。本地配置文件仍会保留。

## 开发者命令

本项目基于 Rust 1.96.0 与 Slint 1.18.1 开发。

```powershell
cargo run
cargo run -- --demo
cargo run -- --choose https://example.com
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo build --locked --release
```

- `--dry-run URL`：打印判断结果，不打开 Chrome。
- `--demo`：使用隔离设置，不打开 Chrome。
- `LINKLANE_HOME`：设置独立的测试数据目录。
- 持续集成与发布：GitHub Actions CI 验证代码格式（fmt）、代码检查（clippy）、测试（tests）与构建（build）；推送 `v*` 标签由 GitHub Actions 构建 Windows 发布包。发布流程要求 Cargo 包版本与 `docs/releases/<tag>.md` 一致。

## 许可证

本项目采用 GPL-3.0-only 许可证开源。
