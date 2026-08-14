#Requires -Version 5.1
# 清理 dsh-bundle 里非 win-x64 的 prebuilds 和开发文件，减小安装包体积。
$ErrorActionPreference = "SilentlyContinue"
$bundle = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\src-tauri\resources\dsh-bundle\node_modules"))
if (-not (Test-Path $bundle)) { Write-Host "bundle not found: $bundle"; exit 1 }

# 1. 删除非 win32-x64 的 prebuilds 子目录
$prebuilds = Get-ChildItem $bundle -Recurse -Directory -Filter "prebuilds"
foreach ($pb in $prebuilds) {
    $subdirs = Get-ChildItem $pb.FullName -Directory
    foreach ($sd in $subdirs) {
        if ($sd.Name -ne "win32-x64") {
            Write-Host "  remove prebuild: $($sd.Name)"
            Remove-Item $sd.FullName -Recurse -Force
        }
    }
}

# 2. 删除 native 模块的 build/Release（prebuilds 已含编译产物）
$builds = Get-ChildItem $bundle -Recurse -Directory -Filter "build" | Where-Object { Test-Path (Join-Path $_.FullName "Release") }
foreach ($b in $builds) {
    Write-Host "  remove build/Release"
    Remove-Item $b.FullName -Recurse -Force
}

# 3. 删除开发文件目录
$devDirs = @("src","deps","third_party","typings","scripts","misc","tests","test","__tests__")
foreach ($name in $devDirs) {
    $found = Get-ChildItem $bundle -Recurse -Directory -Filter $name
    foreach ($d in $found) {
        Remove-Item $d.FullName -Recurse -Force
    }
}

# 4. 删除 .d.ts/.ts/.md 等开发文件
$devFiles = Get-ChildItem $bundle -Recurse -File -Include "*.d.ts","*.ts","*.coffee","*.flow"
foreach ($f in $devFiles) {
    Remove-Item $f.FullName -Force
}

# 统计
$size = (Get-ChildItem $bundle -Recurse -File | Measure-Object -Property Length -Sum).Sum
$count = (Get-ChildItem $bundle -Recurse -File | Measure-Object).Count
Write-Host ""
Write-Host ("Trimmed: {0} MB, {1} files" -f [math]::Round($size/1MB,1), $count)
