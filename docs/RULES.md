# 网站设置：TOML 与命令行

底部点“网站设置”，再点“复制路径”。`rules.toml` 顶部列出了所有写法、示例和命令。把路径和你想要的结果交给 AI，例如：“把公司 GitHub 项目分给工作身份，校验并告诉我这个链接会怎么打开。”

## 试一条

文件为空时是 `rules = []`。添加第一条 `[[rules]]` 前，删掉这一行。用“示例与身份标识”或 `rules identities` 找到自己的 `identity`。

```toml
[[rules]]
name = "公司的 GitHub 项目"
enabled = true
all = [
  { kind = "host", value = "github.com" },
  { kind = "pathPrefix", value = "/acme" },
]
exclude = [{ kind = "query", key = "personal", value = { kind = "exact", value = "1" } }]
action = { kind = "open", identity = "chrome:Default" }

[[rules]]
name = "本机开发服务每次询问"
enabled = true
all = [{ kind = "host", value = "localhost" }, { kind = "port", min = 3000, max = 3999 }]
action = { kind = "ask" }
```

从上往下采用首条命中的设置。`all` 全部满足；`any` 非空时至少命中一项；`exclude` 命中任一项会跳过。`all` 与 `any` 至少填写一个条件。“每次都让我选”优先。找不到目标身份时，轻舟会请你重新选择。

| 条件 | 用途 |
| --- | --- |
| `origin` | 精确匹配协议、域名和有效端口；省略尾部 `/` 和默认端口。 |
| `host` | 匹配域名或 IP；`subdomains = true` 同时包含根域与子域。 |
| `scheme` | `http` 或 `https`。 |
| `port` | `min` 至 `max`，含端点；默认端口为 80 或 443。 |
| `pathPrefix` | 以完整路径段匹配，例如 `/acme` 匹配 `/acme/project`。 |
| `path` | 以 `pattern` 匹配百分号编码后的路径。 |
| `url` | 以 `pattern` 匹配完整规范化网址，含查询参数和片段。 |
| `query` | 以 `key` 匹配参数名。可选 `value` 表达式匹配任一同名参数解码后的值。 |

`path`、`url`、`query` 支持 `{ kind = "exact", value = "…" }`、`glob` 或 `regex`。路径的 `*` 匹配一段、`**` 可跨段、`?` 匹配一个字符。`url` 和 `query` 中的 `*` 可跨 `/`。Rust 正则语法支持 `^` / `$` 锚定和 `(?i)` 忽略大小写。

## 链接跳转

轻舟会从 `url`、`target` 等查询参数逐层提取目的地，以目的地决定身份，打开时仍交给浏览器原链接。出现多个目标、过深嵌套或目标带登录信息时，显示选择器并暂停自动记忆。可直接在已有文件中修改下面的设置：

```toml
[resolution]
parameters = ["url", "target", "redirect_url"]

[[resolution.sites]]
host = "go.example.com"
subdomains = true
parameters = ["to"]

[[resolution.sites]]
host = "login.example.com"
parameters = []
```

首条域名匹配项覆盖默认参数。网站对应的空列表表示按原链接匹配。默认和站点列表都为空时全局关闭。省略整个 `[resolution]` 会采用内置默认值。

## 校验与生效

在窗口编辑时，先“测试草稿”，再“校验并应用”。在外部编辑器保存时，点“从文件重新加载”。打开“网站设置”会显示当前生效的版本。加载失败会显示原因，草稿留在磁盘，后续链接仍使用上次生效的设置。

自动记住和移除网站前会检查磁盘上有没有待加载的编辑。建议先加载文件，再继续在窗口中修改。

命令行程序与 `linklane.exe` 放在同一目录。下面使用默认的每用户安装路径：

```powershell
$cli = "$env:LOCALAPPDATA\Linklane\bin\linklane-cli.exe"
& $cli rules path
& $cli rules show
& $cli rules identities
& $cli rules check 'C:\path\draft.toml'
& $cli rules replace 'C:\path\draft.toml'
& $cli rules reload
& $cli rules explain 'https://github.com/acme/example'
```

`check` 省略文件时检查磁盘上的 `rules.toml`。`replace` 应用整个文件（包括跳转设置）；传 `-` 可从标准输入读取 UTF-8。成功输出 `OK: 数量` 或 `APPLIED: 数量`；错误写入标准错误，退出码为 `1`。`show` 输出当前生效的 TOML，可作为编辑副本的起点。身份暂不可用会在实际匹配时提示，也可先用 `identities` 核对本机标识。
