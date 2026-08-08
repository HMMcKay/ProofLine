[CmdletBinding()]
param([Parameter(Mandatory)][string]$OutputDirectory)
$ErrorActionPreference = "Stop"
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMdd-HHmmss")
$root = [IO.Path]::GetFullPath($OutputDirectory)
$destination = Join-Path $root "proofline-$stamp"
New-Item -ItemType Directory -Force -Path $destination | Out-Null

$sql = docker compose exec -T postgres pg_dump -U proofline -d proofline --clean --if-exists --no-owner --no-privileges
if ($LASTEXITCODE -ne 0) { throw "PostgreSQL backup failed." }
$sql | Set-Content -Encoding utf8 (Join-Path $destination "postgres.sql")

New-Item -ItemType Directory -Path (Join-Path $destination "objects") | Out-Null
docker compose cp minio:/data/. (Join-Path $destination "objects")
if ($LASTEXITCODE -ne 0) { throw "Object-store backup failed." }

$manifest = [ordered]@{
    created_at = (Get-Date).ToUniversalTime().ToString("o")
    postgres_sha256 = (Get-FileHash -Algorithm SHA256 (Join-Path $destination "postgres.sql")).Hash.ToLowerInvariant()
    note = "Server, C2PA, TLS, and offline admin keys are not readable from containers by this script. Back them up separately from their encrypted source."
}
$manifest | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $destination "manifest.json")
Write-Host "Backup written to $destination"
