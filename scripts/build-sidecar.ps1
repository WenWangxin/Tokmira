# Build the token-audit CLI and copy it to src-tauri/binaries/ for Tauri's externalBin.
# Compatible with Windows PowerShell 5.1. The packaged name is token-audit-<target>.exe;
# Tauri strips the target triple and installs token-audit.exe next to the desktop executable.
param(
    [Parameter(Mandatory = $true)]
    [string]$Target
)

$ErrorActionPreference = "Stop"

if ($Target -notmatch '^[A-Za-z0-9._-]+$') {
    Write-Error "invalid target: $Target"
    exit 1
}

$allowed = @("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")
if ($allowed -notcontains $Target) {
    Write-Error "unsupported target: $Target. Expected x86_64-pc-windows-msvc or aarch64-pc-windows-msvc."
    exit 1
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$BinDir = Join-Path $Root "src-tauri\binaries"
if ($env:CARGO_TARGET_DIR) {
    $TargetRoot = $env:CARGO_TARGET_DIR
} else {
    $TargetRoot = Join-Path $Root "target"
}

New-Item -ItemType Directory -Force -Path $BinDir | Out-Null

Push-Location $Root
try {
    & cargo build --release -p token-audit-cli --target $Target
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
} finally {
    Pop-Location
}

$Src = Join-Path $TargetRoot "$Target\release\token-audit.exe"
$Dest = Join-Path $BinDir "token-audit-$Target.exe"
if (-not (Test-Path -LiteralPath $Src -PathType Leaf)) {
    Write-Error "cargo did not produce $Src"
    exit 1
}

Copy-Item -LiteralPath $Src -Destination $Dest -Force
Write-Host "sidecar: $Dest"
