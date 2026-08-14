#Requires -Version 5.1
# 拉取指定版本的 @deepseek-ai/dsh + 便携 Node.js 到 src-tauri/resources/，供 Tauri 打包进安装包。
# 用法: pwsh fetch-dsh.ps1
#       pwsh fetch-dsh.ps1 -DshVersion 0.1.0-rc.6 -NodeVersion v24.16.0
param(
    [string]$DshVersion = "0.1.0-rc.6",
    [string]$NodeVersion = "v24.16.0"
)

$ErrorActionPreference = "Stop"
$resources = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\src-tauri\resources"))

# ── 1. dsh bundle ──
$dshDest = Join-Path $resources "dsh-bundle"
Write-Host "Fetching @deepseek-ai/dsh@$DshVersion -> $dshDest"
if (Test-Path $dshDest) { Remove-Item -Recurse -Force $dshDest }
New-Item -ItemType Directory -Force -Path $dshDest | Out-Null
Push-Location $dshDest
try {
    npm init -y | Out-Null
    npm install "@deepseek-ai/dsh@$DshVersion" --omit=dev
    if ($LASTEXITCODE -ne 0) { throw "npm install dsh failed" }
} finally { Pop-Location }

# ── 2. 便携 Node.js（win-x64 zip）──
$nodeDest = Join-Path $resources "node-portable"
Write-Host "Fetching Node.js $NodeVersion (win-x64) -> $nodeDest"
if (Test-Path $nodeDest) { Remove-Item -Recurse -Force $nodeDest }
New-Item -ItemType Directory -Force -Path $nodeDest | Out-Null

$zip = Join-Path $env:TEMP "node-$NodeVersion-win-x64.zip"
$url = "https://nodejs.org/dist/$NodeVersion/node-$NodeVersion-win-x64.zip"
Write-Host "  Downloading $url"
Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing

$extractTmp = Join-Path $env:TEMP "node-extract-$NodeVersion"
if (Test-Path $extractTmp) { Remove-Item -Recurse -Force $extractTmp }
Expand-Archive -Path $zip -DestinationPath $extractTmp -Force
# 解压后是 node-<ver>-win-x64\node.exe，把 node.exe 拷到 node-portable/
$nodeExe = Join-Path $extractTmp "node-$NodeVersion-win-x64\node.exe"
if (-not (Test-Path $nodeExe)) { throw "node.exe not found after extract: $nodeExe" }
Copy-Item $nodeExe -Destination (Join-Path $nodeDest "node.exe")
Remove-Item $zip -Force
Remove-Item $extractTmp -Recurse -Force

Write-Host ""
Write-Host "Done."
Write-Host "  dsh@$DshVersion -> $dshDest\node_modules\@deepseek-ai\dsh"
Write-Host "  node $NodeVersion  -> $nodeDest\node.exe"
Write-Host ""
Write-Host "Remember to update DSH_PINNED_VERSION / NODE_PINNED_VERSION in src-tauri/src/dsh.rs to match."
