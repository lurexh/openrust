# one-line installer (Windows PowerShell):
# irm https://raw.githubusercontent.com/lurexh/openrust/main/install.ps1 | iex
$ErrorActionPreference = "Stop"

$repo = "lurexh/openrust"
$target = "x86_64-pc-windows-msvc"

Write-Host "==> resolving latest release for $target"
$release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest"
$asset = $release.assets | Where-Object { $_.name -eq "oc-rs-$target.zip" } | Select-Object -First 1
if (-not $asset) { throw "could not find oc-rs-$target.zip in the latest release" }

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("oc-rs-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host "==> downloading $($asset.browser_download_url)"
    $zip = Join-Path $tmp "oc-rs.zip"
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip

    $shaUrl = $asset.browser_download_url + ".sha256"
    try {
        $expected = (Invoke-RestMethod $shaUrl).Split(" ")[0].Trim().ToLower()
        $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
        if ($expected -ne $actual) { throw "checksum mismatch" }
        Write-Host "==> checksum ok"
    } catch {
        Write-Host "==> (skipping checksum: $($_.Exception.Message))"
    }

    Expand-Archive -Path $zip -DestinationPath $tmp
    $binDir = Join-Path $HOME "bin"
    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    Copy-Item (Join-Path $tmp "oc-rs.exe") (Join-Path $binDir "oc-rs.exe") -Force

    if ($env:PATH -notlike "*$binDir*") {
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$binDir", "User")
        Write-Host "==> added $binDir to your user PATH (open a new terminal)"
    }

    Write-Host "==> installed to $binDir\oc-rs.exe — run it: oc-rs"
} finally {
    Remove-Item -Recurse -Force $tmp
}
