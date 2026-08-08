[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BackupDirectory,
    [Parameter(Mandatory)][ValidateSet("RESTORE-PROOFLINE")][string]$ConfirmRestore
)
$ErrorActionPreference = "Stop"
$source = [IO.Path]::GetFullPath($BackupDirectory)
$sqlPath = Join-Path $source "postgres.sql"
$objectsPath = Join-Path $source "objects"
if (-not (Test-Path -LiteralPath $sqlPath) -or -not (Test-Path -LiteralPath $objectsPath)) {
    throw "Backup is missing postgres.sql or objects/."
}
Write-Warning "This replaces the current ProofLine database and MinIO objects. Stop public ingest first."
docker compose stop gateway worker caddy
Get-Content -Raw -LiteralPath $sqlPath | docker compose exec -T postgres psql -v ON_ERROR_STOP=1 -U proofline -d proofline
if ($LASTEXITCODE -ne 0) { throw "Database restore failed; media was not replaced." }
docker compose exec -T minio sh -c 'test "$(readlink -f /data)" = /data && rm -rf -- /data/* /data/.[!.]* /data/..?*'
if ($LASTEXITCODE -ne 0) { throw "Unable to clear the exact MinIO /data restore target." }
docker compose cp "$objectsPath/." minio:/data
if ($LASTEXITCODE -ne 0) { throw "Object restore failed." }
docker compose up -d --wait
Write-Host "Restore completed. Run the evidence sample verification before reopening ingest."
