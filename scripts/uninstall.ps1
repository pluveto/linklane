$ErrorActionPreference = 'Stop'
foreach ($scheme in @('http', 'https')) {
    $choicePath = "HKCU:\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\$scheme\UserChoice"
    $choice = Get-ItemProperty -LiteralPath $choicePath -ErrorAction SilentlyContinue
    if ($choice.ProgId -eq 'Linklane.Url') {
        throw '请先在 Windows 默认应用设置中重新选择 Chrome，再移除轻舟。'
    }
}
Remove-ItemProperty -LiteralPath 'HKCU:\Software\RegisteredApplications' -Name '轻舟 Linklane' -ErrorAction SilentlyContinue
foreach ($key in @('HKCU:\Software\Classes\Linklane.Url', 'HKCU:\Software\Clients\StartMenuInternet\Linklane')) {
    if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key -Recurse }
}
$bin = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Linklane\bin'
foreach ($name in 'linklane.exe', 'linklane-cli.exe') {
    $installedExe = Join-Path $bin $name
    if (Test-Path -LiteralPath $installedExe) { Remove-Item -LiteralPath $installedExe }
}
Write-Output '轻舟已移除。已保存的网站选择仍保留在用户文件夹中。'
