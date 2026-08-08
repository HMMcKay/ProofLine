[CmdletBinding()]
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$runningOnWindows = $env:OS -eq "Windows_NT"
Set-Location -LiteralPath $repositoryRoot

if (-not (Test-Path -LiteralPath "node_modules" -PathType Container) -or -not (Test-Path -LiteralPath ".env" -PathType Leaf)) {
    & "$PSScriptRoot\setup-local.ps1"
}

$gitBash = "C:\Program Files\Git\bin\bash.exe"
if ($runningOnWindows -and (Test-Path -LiteralPath $gitBash -PathType Leaf)) {
    $env:npm_config_script_shell = $gitBash
}

& "$PSScriptRoot\prepare-local-media-plane.ps1"

$composeArguments = @("compose", "up", "-d")
if (-not $SkipBuild) { $composeArguments += "--build" }
$composeArguments += "--wait"
& docker @composeArguments
if ($LASTEXITCODE -ne 0) { throw "The ProofLine media plane did not start successfully." }

Write-Host "Media plane ready at http://localhost:8080"
Write-Host "Starting the public/control plane at http://localhost:3000"
Write-Host "Press Ctrl+C to stop the web server; containers remain available until ./scripts/stop-local.ps1."
& npm run dev
