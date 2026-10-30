# 发布构建：产出两个产物（版本跟随 git tag，vX.Y.Z -> X.Y.Z）
#   安装版：dist\BossKey_v<版本>.exe      （Inno Setup 安装包）
#   绿色版：dist\BossKey_Portable_v<版本>.exe
# 用法：powershell -File scripts\release.ps1 [-Version 0.1.18]
param([string]$Version)

$root = 'E:\project\bosskey'

if (-not $Version) {
    $tag = (git -C $root describe --tags --abbrev=0 2>$null)
    if ($tag) { $Version = $tag.TrimStart('v') }
    else {
        $Version = (Select-String -Path "$root\Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
    }
}

Write-Host "版本: $Version"
cargo build --release 2>&1 | Select-Object -Last 1
New-Item -ItemType Directory -Force -Path "$root\dist" | Out-Null

# ---- 绿色版 ----
$portable = "$root\dist\BossKey_Portable_v$Version.exe"
Copy-Item "$root\target\release\bosskey.exe" $portable -Force

# ---- 安装版（Inno Setup）----
$iscc = $env:ISCC
if (-not $iscc) {
    foreach ($p in @("D:\develop_tool\InnoSetup\ISCC.exe", "C:\Program Files (x86)\Inno Setup 6\ISCC.exe")) {
        if (Test-Path $p) { $iscc = $p; break }
    }
}
if (-not $iscc) { $iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source) }
if (-not $iscc) {
    throw "未找到 ISCC.exe（Inno Setup 6）。请安装后重试：winget install JRSoftware.InnoSetup，或用环境变量 ISCC 指定路径"
}

$exePath = "$root\target\release\bosskey.exe"
& $iscc "/DAppVersion=$Version" "/DExePath=$exePath" "$root\installer\bosskey.iss" | Select-Object -Last 3
if ($LASTEXITCODE -ne 0) { throw "Inno Setup 打包失败（退出码 $LASTEXITCODE）" }

Get-Item "$root\dist\BossKey_v$Version.exe", $portable | Select-Object Name, Length
