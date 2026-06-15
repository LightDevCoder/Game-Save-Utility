$ErrorActionPreference = "Stop"

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "Cargo was not found. Install the Rust stable toolchain first: https://rustup.rs/"
}

$cargoTargetDir = Join-Path $PSScriptRoot "..\target\cargo-release"
$releaseDir = Join-Path $PSScriptRoot "..\target\release"
$sourceExe = Join-Path $cargoTargetDir "release\game-save-utility.exe"
$cargoToml = Join-Path $PSScriptRoot "..\Cargo.toml"
$versionLine = Get-Content -LiteralPath $cargoToml | Where-Object { $_ -match '^\s*version\s*=\s*"([^"]+)"' } | Select-Object -First 1
if ($versionLine -notmatch '^\s*version\s*=\s*"([^"]+)"') {
    Write-Error "Could not read package version from Cargo.toml."
}
$version = $Matches[1]
$releaseExe = Join-Path $releaseDir "Game_Save_Utility_V$version.exe"
$legacyReleaseExe = Join-Path $releaseDir "Game Save Utility.exe"
$legacyCargoExe = Join-Path $releaseDir "game-save-utility.exe"
$oldPackageExe = Join-Path $releaseDir "game-save-backup-tool.exe"

cargo build --release --target-dir $cargoTargetDir

New-Item -ItemType Directory -Path $releaseDir -Force | Out-Null
if (Test-Path $sourceExe) {
    Copy-Item -LiteralPath $sourceExe -Destination $releaseExe -Force
    Remove-Item -LiteralPath $sourceExe -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $legacyReleaseExe -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $legacyCargoExe -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $oldPackageExe -Force -ErrorAction SilentlyContinue
    Write-Host "Build complete: $releaseExe"
} else {
    Write-Error "Build finished, but the output exe was not found."
}
