param([string]$Tag = '')

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-Location (Split-Path $PSScriptRoot -Parent)

$metadata = cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
$package = $metadata.packages | Where-Object name -eq 'linklane'
$version = $package.version
if ($Tag -and $Tag -ne "v$version") {
    throw "Tag $Tag must match Cargo.toml version v$version"
}
if ($Tag -and -not (Test-Path -LiteralPath "docs/releases/$Tag.md")) {
    throw "Missing release notes: docs/releases/$Tag.md"
}

cargo build --locked --release --target x86_64-pc-windows-msvc
$exe = Join-Path $metadata.target_directory 'x86_64-pc-windows-msvc/release/linklane.exe'
$dist = Join-Path $pwd 'dist'
$staging = Join-Path $dist "linklane-v$version-windows-x86_64"
New-Item -ItemType Directory -Force -Path $staging | Out-Null

# A release binary uses the Windows GUI subsystem: explicitly wait for it.
$smokeOutput = Join-Path $dist 'smoke-output.txt'
$smokeError = Join-Path $dist 'smoke-error.txt'
$smoke = Start-Process -FilePath $exe -ArgumentList '--demo','--dry-run','https://example.com' -WindowStyle Hidden -PassThru -RedirectStandardOutput $smokeOutput -RedirectStandardError $smokeError
if (-not $smoke.WaitForExit(30000)) {
    Stop-Process -Id $smoke.Id
    throw 'Release executable smoke check timed out'
}
if ($smoke.ExitCode -ne 0 -or (Get-Content -LiteralPath $smokeOutput -Raw).Trim() -ne 'ASK') {
    throw "Release executable smoke check failed; inspect $smokeError"
}

$toolVersion = '0.9.3'
$toolName = "cargo-about-$toolVersion-x86_64-pc-windows-msvc"
$toolRoot = Join-Path $pwd 'artifacts/tools'
$about = Join-Path $toolRoot "$toolName/cargo-about.exe"
if (-not (Test-Path -LiteralPath $about)) {
    New-Item -ItemType Directory -Force -Path $toolRoot | Out-Null
    $archive = Join-Path $toolRoot "$toolName.tar.gz"
    Invoke-WebRequest -Uri "https://github.com/EmbarkStudios/cargo-about/releases/download/$toolVersion/$toolName.tar.gz" -OutFile $archive
    tar -xzf $archive -C $toolRoot
}
& $about generate --locked about.hbs -o (Join-Path $staging 'THIRD_PARTY_LICENSES.html')

Copy-Item -LiteralPath $exe -Destination (Join-Path $staging 'linklane.exe')
Copy-Item -LiteralPath 'README.md','LICENSE','scripts/uninstall.ps1' -Destination $staging
New-Item -ItemType Directory -Force -Path (Join-Path $staging 'docs') | Out-Null
Copy-Item -LiteralPath 'docs/RULES.md','docs/images' -Destination (Join-Path $staging 'docs') -Recurse -Force
Copy-Item -LiteralPath $exe -Destination (Join-Path $dist 'linklane.exe')
Copy-Item -LiteralPath (Join-Path $staging 'THIRD_PARTY_LICENSES.html') -Destination $dist
Compress-Archive -Path "$staging/*" -DestinationPath (Join-Path $dist "linklane-v$version-windows-x86_64.zip") -Force
Write-Output "Packaged Linklane v$version for Windows x64"
