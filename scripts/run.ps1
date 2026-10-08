# Builds the kernel and boots it in QEMU (PVH entry, no ISO needed).
# Usage: .\scripts\run.ps1 [-Debug] [-NoGraphic]
param(
    [switch]$Debug,
    [switch]$NoGraphic
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$buildProfile = if ($Debug) { "debug" } else { "release" }
if ($Debug) { cargo build } else { cargo build --release }
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$qemu = (Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue).Source
if (-not $qemu) {
    $candidate = "C:\Program Files\qemu\qemu-system-x86_64.exe"
    if (Test-Path $candidate) { $qemu = $candidate }
}
if (-not $qemu) {
    Write-Error "qemu-system-x86_64 not found. Install it: winget install SoftwareFreedomConservancy.QEMU"
}

$kernel = "target\x86_64-unknown-none\$buildProfile\huldra"
$qemuArgs = @("-m", "256M", "-no-reboot", "-kernel", $kernel)
if ($NoGraphic) { $qemuArgs += "-nographic" } else { $qemuArgs += @("-serial", "stdio") }
& $qemu @qemuArgs
