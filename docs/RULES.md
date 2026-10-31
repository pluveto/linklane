# 高级规则

在“高级规则”中编辑 JSON，“测试草稿”仅解释匹配结果；“校验并应用”保存后生效。
规则按数组顺序匹配，第一条命中即决定打开身份或每次询问。停用规则会跳过；目标身份暂不可用时回到选择器，并显示原因。
all 全部满足，any 至少一项满足（空数组表示忽略），exclude 任意一项满足就排除。all 与 any 至少填写一个条件。
“每次都让我选”和命令行 --choose 优先于所有规则。
基础选择器的“记住网站”会更新已有精确来源规则，或把新规则加在末尾；可在编辑器中调整顺序。

## Query 跳转目标

规则先解析 query 中的目标网址，再匹配该目标。实际打开保留完整原始链接，让网站继续完成中转、登录回跳和签名校验。解析全程在本地完成。

例如 https://link.example/out?url=https%3A%2F%2Fgithub.com%2Facme，会按 https://github.com/acme 匹配浏览器身份，浏览器收到的仍是 link.example 的原始链接。勾选记住网站时保存 github.com 的来源。

默认参数（区分大小写）：url、target、redirect、redirect_url、redirect_uri、redirectUrl、destination、dest、return_url、returnUrl、continue。参数值可为 HTTP/HTTPS URL、/ 开头的相对路径或 // 开头的跨域地址。普通搜索词保持原网址。

支持最多 8 层嵌套。每层 query 按 URL 表单规则解码；额外百分号编码最多尝试 3 层，识别出网址即停止，保留目标中的 %2F、%2B 与签名值。不同候选目标、过深嵌套或含账号密码的目标会交回选择器，并暂停自动记忆。重复参数指向同一目标时可继续解析。

高级编辑框在 rules 同级接受 resolution，例如：

```json
{
  "rules": [],
  "resolution": {
    "parameters": ["url", "target", "redirect_url"],
    "sites": [
      { "host": "go.example.com", "parameters": ["to"] },
      { "host": "login.example.com", "parameters": [] },
      { "host": "example.org", "subdomains": true, "parameters": ["destination"] }
    ]
  }
}
```

保留自己的 rules，仅修改 resolution。sites 按顺序取第一条域名匹配项，该项参数列表替换默认列表；空列表表示保留原网址。parameters 与 sites 均为空时全局关闭解析。省略 resolution 会使用默认参数列表。特定网站用 q、u 等参数时，可在 sites 中指定。

“测试草稿”和 --explain 会显示原始链接、逐层参数、匹配目标和命中的浏览器身份。解析结果只用于本次匹配，配置只保存手工设置和记住的目标来源。服务器短链接和 JavaScript 中转页留待后续解析方式支持。

## 示例

identity 请替换为上方“当前可用身份”中的真实标识。

```json
{
  "rules": [
    {
      "name": "公司的 GitHub 项目",
      "enabled": true,
      "all": [
        { "kind": "host", "value": "github.com" },
        { "kind": "pathPrefix", "value": "/acme" }
      ],
      "exclude": [
        { "kind": "query", "key": "personal", "value": { "kind": "exact", "value": "1" } }
      ],
      "action": { "kind": "open", "identity": "chrome:Default" }
    },
    {
      "name": "本机开发服务",
      "enabled": true,
      "all": [
        { "kind": "host", "value": "localhost" },
        { "kind": "port", "min": 3000, "max": 3999 }
      ],
      "action": { "kind": "open", "identity": "edge:Default" }
    },
    {
      "name": "其他公司网站先询问",
      "enabled": true,
      "all": [{ "kind": "host", "value": "example.com", "subdomains": true }],
      "action": { "kind": "ask" }
    }
  ]
}
```

## 条件

- origin：精确协议、域名和有效端口。例如 {"kind":"origin","value":"https://example.com"}。默认端口省略，末尾省略 /。
- host：域名或 IP。域名大小写及国际化域名按 URL 标准归一化。subdomains:true 同时包含根域和子域；example.com.evil.test 永远属于另一个域。
- scheme：value 为 http 或 https。
- port：min / max 为闭区间；省略端口的 https 使用 443，http 使用 80。
- pathPrefix：按路径段匹配。/acme 匹配 /acme 与 /acme/project，排除 /acme-other。
- path：pattern 为 exact、glob 或 regex。路径区分大小写，使用 URL 百分号编码形式。
- url：pattern 为 exact、glob 或 regex，匹配规范化后的完整网址（包含查询参数和片段）。
- query：key 区分大小写；省略 value 表示参数存在。value 为 exact、glob 或 regex，匹配 URL 解码后的参数值；重复参数中任意一个值满足即可。

pattern 示例：{"kind":"glob","value":"/acme/**"}、{"kind":"regex","value":"^/project/[0-9]+$"}。
path glob 中 * 匹配一个路径段，** 跨路径段；? 匹配一个字符。url / query glob 中 * 可跨 /。
正则使用 Rust regex 语法，默认区分大小写且匹配任意子串；用 ^ 和 $ 锚定，(?i) 忽略大小写。
正则表达式采用有界编译大小及线性时间匹配，支持字符类与分组；反向引用和环视会显示校验错误。

## 数据与调试

编辑框中的草稿在点击“校验并应用”后进入 settings.json。语法、字段或规则无效时保留原配置与当前草稿。
其他窗口修改了规则时，会提示先保留草稿，再“重新载入”合并。自动打开产生的最近使用记录可与规则编辑并行保存。
仅手动编写的条件值进入配置；输入测试的网址保留在当前窗口内。自动记住网站仅保存 origin。
--list-identities 列出本机标识；--dry-run URL 输出 OPEN / ASK；--explain URL 输出逐条匹配原因。
