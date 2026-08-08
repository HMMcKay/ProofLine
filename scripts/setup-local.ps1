[CmdletBinding()]
param(
    [switch]$SkipNpmInstall
)

$ErrorActionPreference = "Stop"
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$runningOnWindows = $env:OS -eq "Windows_NT"
Set-Location -LiteralPath $repositoryRoot

function Require-Command {
    param([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)][string]$InstallHint)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "$Name is required. $InstallHint"
    }
}

Require-Command -Name git -InstallHint "Install Git for Windows or your platform's Git package."
Require-Command -Name node -InstallHint "Install Node.js 22.13 or newer."
Require-Command -Name npm -InstallHint "npm is installed with Node.js."
Require-Command -Name docker -InstallHint "Install and start Docker Desktop with Compose v2."

$nodeVersionText = (& node --version).Trim().TrimStart("v")
$nodeVersion = [version]$nodeVersionText
if ($nodeVersion -lt [version]"22.13.0") {
    throw "Node.js 22.13 or newer is required; found $nodeVersionText."
}

& docker info *> $null
if ($LASTEXITCODE -ne 0) { throw "Docker is installed but its engine is not ready." }

$gitBash = "C:\Program Files\Git\bin\bash.exe"
if ($runningOnWindows -and -not (Test-Path -LiteralPath $gitBash -PathType Leaf)) {
    throw "Git Bash was not found at $gitBash. Install Git for Windows or set npm_config_script_shell manually."
}
if ($runningOnWindows) { $env:npm_config_script_shell = $gitBash }

if (-not (Test-Path -LiteralPath ".env" -PathType Leaf)) {
    Copy-Item -LiteralPath ".env.example" -Destination ".env"
    Write-Host "Created .env from development defaults. Replace every secret before exposing the service."
} else {
    Write-Host "Preserved existing .env."
}

if (-not $SkipNpmInstall) {
    & npm ci
    if ($LASTEXITCODE -ne 0) { throw "npm ci failed." }
}

& docker compose config --quiet
if ($LASTEXITCODE -ne 0) { throw "Docker Compose configuration is invalid." }

& node scripts/validate-docs.mjs
if ($LASTEXITCODE -ne 0) { throw "Documentation validation failed." }

Write-Host "ProofLine setup is ready. Run ./scripts/start-local.ps1 to start the media and web planes."
