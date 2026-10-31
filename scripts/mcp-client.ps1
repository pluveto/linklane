param([int]$Port = 9315)
$script:SlintMcpPort = $Port

function Invoke-SlintTool([string]$Name, [hashtable]$Arguments = @{}) {
    $body = @{jsonrpc='2.0'; id=1; method='tools/call'; params=@{name=$Name; arguments=$Arguments}} | ConvertTo-Json -Depth 30 -Compress
    $response = Invoke-RestMethod -Uri "http://127.0.0.1:$script:SlintMcpPort/mcp" -Method Post -ContentType 'application/json; charset=utf-8' -Headers @{Accept='application/json, text/event-stream'} -Body ([Text.Encoding]::UTF8.GetBytes($body))
    if ($response.error) { throw ($response.error | ConvertTo-Json) }
    if ($response.result.isError) { throw ($response.result | ConvertTo-Json -Depth 10) }
    return $response.result
}
function Get-SlintText([string]$Name, [hashtable]$Arguments = @{}) {
    $result = Invoke-SlintTool $Name $Arguments
    return ($result.content | Where-Object type -eq 'text' | Select-Object -First 1).text | ConvertFrom-Json
}
function Save-SlintScreenshot([hashtable]$Window, [string]$Path) {
    $result = Invoke-SlintTool 'take_screenshot' @{windowHandle=$Window}
    $image = $result.content | Where-Object type -eq 'image' | Select-Object -First 1
    [IO.File]::WriteAllBytes($Path, [Convert]::FromBase64String($image.data))
}
