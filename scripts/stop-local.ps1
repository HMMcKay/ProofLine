[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$repositoryRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $repositoryRoot

& docker compose down --remove-orphans
if ($LASTEXITCODE -ne 0) { throw "Docker Compose did not stop cleanly." }
Write-Host "ProofLine containers stopped. Persistent PostgreSQL and media volumes were retained."

