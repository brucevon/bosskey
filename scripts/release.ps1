# 发布构建：产出 dist\BossKey-<版本>.exe，版本跟随 git tag（vX.Y.Z -> X.Y.Z）
# 用法：powershell -File scripts\release.ps1 [-Version 0.0.1]
param([string]$Version)

if (-not $Version) {
    $tag = (git -C 'E:\project\bosskey' describe --tags --abbrev=0 2>$null)
    if ($tag) { $Version = $tag.TrimStart('v') }
    else {
        $Version = (Select-String -Path 'E:\project\bosskey\Cargo.toml' -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
    }
}

Write-Host "版本: $Version"
cargo build --release 2>&1 | Select-Object -Last 1
New-Item -ItemType Directory -Force -Path 'E:\project\bosskey\dist' | Out-Null
$dest = "E:\project\bosskey\dist\BossKey-$Version.exe"
Copy-Item 'E:\project\bosskey\target\release\bosskey.exe' $dest -Force
Get-Item $dest | Select-Object Name, Length
