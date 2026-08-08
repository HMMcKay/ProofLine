[CmdletBinding()]
param(
    [switch]$IncludeRust,
    [switch]$IncludeContainers,
    [switch]$IncludeAndroid,
    [switch]$IncludeC2pa
)

$ErrorActionPreference = "Stop"
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$runningOnWindows = $env:OS -eq "Windows_NT"
Set-Location -LiteralPath $repositoryRoot

$gitBash = "C:\Program Files\Git\bin\bash.exe"
if ($runningOnWindows -and (Test-Path -LiteralPath $gitBash -PathType Leaf)) {
    $env:npm_config_script_shell = $gitBash
}

function Invoke-Checked {
    param([Parameter(Mandatory)][scriptblock]$Command, [Parameter(Mandatory)][string]$Label)
    Write-Host "`n== $Label =="
    & $Command
    if ($LASTEXITCODE -ne 0) { throw "$Label failed with exit code $LASTEXITCODE." }
}

Invoke-Checked -Label "Documentation" -Command { npm run docs:check }
Invoke-Checked -Label "Web lint" -Command { npm run lint }
Invoke-Checked -Label "Web typecheck" -Command { npm run typecheck }
Invoke-Checked -Label "Web and protocol tests" -Command { npm test }
Invoke-Checked -Label "Production web build and rendered routes" -Command { npm run test:built }

if ($IncludeRust) {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw "Rust 1.88 is required for -IncludeRust." }
    Invoke-Checked -Label "Rust formatting" -Command { cargo fmt --all -- --check }
    Invoke-Checked -Label "Rust Clippy" -Command { cargo clippy --workspace --all-targets -- -D warnings }
    Invoke-Checked -Label "Rust tests" -Command { cargo test --workspace }
    Invoke-Checked -Label "Rust release build" -Command { cargo build --workspace --release }
}

if ($IncludeContainers) {
    Invoke-Checked -Label "Container stack" -Command { docker compose -f docker-compose.yml -f docker-compose.test.yml up -d --build --wait }
    Invoke-Checked -Label "Media-plane scenarios" -Command { npm run test:media }
}

if ($IncludeAndroid) {
    Push-Location android
    try {
        Invoke-Checked -Label "Android tests, lint and debug APK" -Command { ./gradlew.bat testDebugUnitTest lintDebug assembleDebug }
    } finally { Pop-Location }
}

if ($IncludeC2pa) {
    Invoke-Checked -Label "C2PA/CMAF compatibility spike" -Command { ./scripts/c2pa-spike.ps1 }
}

Write-Host "`nRequested ProofLine checks passed. Review docs/validation/ before inferring physical-device or production readiness."
