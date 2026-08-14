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
# 解压后是 node-<ver>-win-x64\，包含 node.exe + node_modules/（npm 等内置模块）
# 整个目录内容拷到 node-portable/，保证 npm 等工具可用（dsh 插件安装需要 npm）
$nodeSrcDir = Join-Path $extractTmp ("node-" + $NodeVersion + "-win-x64")
if (-not (Test-Path (Join-Path $nodeSrcDir "node.exe"))) { throw "node.exe not found after extract: $nodeSrcDir" }
Copy-Item (Join-Path $nodeSrcDir "*") -Destination $nodeDest -Recurse -Force
Remove-Item $zip -Force
Remove-Item $extractTmp -Recurse -Force

# ── 3. trim bundle（清理非 win32-x64 prebuilds 和开发文件）──
$trimScript = Join-Path $PSScriptRoot "trim-bundle.ps1"
if (Test-Path $trimScript) {
    Write-Host "Trimming dsh-bundle..."
    & $trimScript | Out-Host
}

# ── 4. 打包成 zip，避免 NSIS 直接打包几万小文件（打包/安装都极慢）──
# 安装时只解压 2 个 zip，首启由 dsh.rs 解压到 dsh_home
Add-Type -AssemblyName System.IO.Compression.FileSystem
function New-ZipFromDir($src, $zip) {
    if (Test-Path $zip) { Remove-Item $zip -Force }
    [System.IO.Compression.ZipFile]::CreateFromDirectory($src, $zip, [System.IO.Compression.CompressionLevel]::Optimal, $false)
}

$bundleZip = Join-Path $resources "dsh-bundle.zip"
$nodeZip = Join-Path $resources "node-portable.zip"
Write-Host "Zipping dsh-bundle -> $bundleZip"
New-ZipFromDir $dshDest $bundleZip
Write-Host "Zipping node-portable -> $nodeZip"
New-ZipFromDir $nodeDest $nodeZip

# 删除散文件目录，避免被 Tauri 重复打包进安装包
Remove-Item $dshDest -Recurse -Force
Remove-Item $nodeDest -Recurse -Force

Write-Host ""
Write-Host "Done."
Write-Host "  dsh@$DshVersion -> $bundleZip"
Write-Host "  node $NodeVersion  -> $nodeZip"
Write-Host ""
Write-Host "Remember to update DSH_PINNED_VERSION / NODE_PINNED_VERSION in src-tauri/src/dsh.rs to match."
