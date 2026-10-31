# 轻舟 Linklane

A compact Windows link chooser with browser identities and explainable routing rules.

轻舟把外部链接交给合适的浏览器身份。设为 Windows 默认 HTTP/HTTPS 处理程序后，点击邮件、聊天或编辑器里的链接，就能选择 Chrome、Edge、Brave 或 Firefox 的身份，也能让规则自动决定。

![轻舟选择界面](docs/images/chooser.png)

*截图使用演示身份。日常选择器保持紧凑，高级规则按需展开。*

## 功能

- **多个浏览器，一张身份列表**：自动发现标准安装位置的 Chrome、Edge、Brave、Firefox 稳定版。每张卡片显示身份和浏览器，最近使用优先，支持独立重命名。
- **记住网站**：勾选“下次也用这个身份打开此网站”，保存精确的协议、域名和有效端口。旧版已记住的网站继续生效。
- **高级规则**：底部进入 JSON 编辑器，按域名及子域、协议、端口范围、路径、查询参数或完整网址匹配；支持通配符、正则、全部／任一条件与排除条件。
- **明确优先级**：第一条命中决定打开身份或询问。“每次都让我选”可暂停自动打开；目标身份暂不可用时显示原因并交回选择器。
- **先测试再应用**：草稿测试显示逐条匹配原因，应用时校验。多个窗口修改同一份规则会提示冲突，保留草稿供合并。
- **本地运行**：仅读取浏览器身份元数据。自动记住网站仅保存来源；手动规则保存用户填写的条件；测试网址仅留在当前窗口。退出窗口后结束进程。

详细语法与示例见 [高级规则说明](docs/RULES.md)，应用内“示例与身份标识”同时列出本机可复制的身份标识。

![高级规则与测试](docs/images/advanced-rules.png)

## 安装与使用

公开发布包见 [GitHub Releases](https://github.com/pluveto/linklane/releases/latest)。当前源码版本为 **0.2.0**；本地打包产物为 `linklane-v0.2.0-windows-x86_64.zip`，包含程序、文档、许可证与卸载脚本。

1. 解压并运行 `linklane.exe`。
2. 点击“开始使用”，安装至 `%LOCALAPPDATA%/Linklane/bin` 并完成系统注册。设置位于 `%LOCALAPPDATA%/Linklane/settings.json`。
3. 在 Windows“默认应用”中搜索“轻舟 Linklane”，设为默认浏览器，关联 HTTP 与 HTTPS。

运行需要 Windows x64；安装按当前用户进行。当前构建使用未签名可执行文件。更新采用手动替换安装。

## 支持范围

- Chromium 家族从各自 `User Data/Local State` 发现身份；Firefox 从 `%APPDATA%/Mozilla/Firefox/profiles.ini` 发现相对或绝对路径的身份。
- 当前覆盖标准位置的稳定版安装。便携版、自定义安装路径、多个发行通道和 Firefox Container 标签页留待后续支持。
- 经过 Windows HTTP/HTTPS 关联的外部链接参与路由；浏览器内跳转、应用内 WebView 和强制指定浏览器的链接由对应应用处理。
- 当前条件基于 URL。来源应用、快捷键修饰符及时间段尚未加入条件类型。

## 升级与回滚

首次读取 v0.1 设置时，程序先逐字节备份为 `settings.v1.backup.json`，再原子写入 v2 配置。旧身份转为 `chrome:…`，网站转为精确来源规则，自定义名称与最近使用记录一并保留。校验或写入失败会显示错误并保留原文件。

回滚到旧版时：关闭所有轻舟窗口，另存当前 `settings.json`，恢复旧程序，并把 `settings.v1.backup.json` 复制为 `settings.json`。该步骤恢复升级时的数据；升级后添加的规则保存在另存的 v2 文件中。

## 卸载

在 Windows 默认应用中选择接管链接的浏览器，关闭轻舟，然后运行随包的 `uninstall.ps1`。卸载保留本地配置。

## 开发与验证

Rust 1.96.0，Slint 1.18.1。

```powershell
cargo run -- --demo
cargo run -- --list-identities
cargo run -- --explain https://example.com/project
cargo run -- --dry-run https://example.com
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
./scripts/package.ps1 -Tag v0.2.0
```

`--demo` 使用内存中的演示身份与规则。`--dry-run`、`--explain` 只输出决策；`--choose URL` 强制显示选择器。`LINKLANE_HOME` 可指定独立的测试设置目录。Slint MCP 界面检查用 `--features slint/mcp` 单独构建，正式包省略此功能。

测试覆盖多浏览器身份隔离、启动参数、组合条件、匹配边界、优先级、无效表达式、旧文件迁移与写入失败、并发保存和草稿冲突。CI 执行格式检查、Clippy、测试和构建；推送 `v*` 标签发布匹配版本的安装包。

## 许可证

GPL-3.0-only。
