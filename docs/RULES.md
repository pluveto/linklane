# 高级规则

在“高级规则”中编辑 JSON，“测试草稿”仅解释匹配结果；“校验并应用”保存后生效。
规则按数组顺序匹配，第一条命中即决定打开身份或每次询问。停用规则会跳过；目标身份暂不可用时回到选择器，并显示原因。
all 全部满足，any 至少一项满足（空数组表示忽略），exclude 任意一项满足就排除。all 与 any 至少填写一个条件。
“每次都让我选”和命令行 --choose 优先于所有规则。
基础选择器的“记住网站”会更新已有精确来源规则，或把新规则加在末尾；可在编辑器中调整顺序。

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
