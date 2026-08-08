[CmdletBinding()]
param([Parameter(Mandatory)][string]$OutputDirectory)
$ErrorActionPreference = "Stop"
$resolved = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $resolved | Out-Null
$privatePath = Join-Path $resolved "server-signing-key.txt"
$adminPath = Join-Path $resolved "admin-private-key.txt"
if ((Test-Path -LiteralPath $privatePath) -or (Test-Path -LiteralPath $adminPath)) {
    throw "Refusing to overwrite an existing signing key in $resolved"
}
Write-Host "Generating the receipt key. Save the printed public SPKI with the deployment record."
docker run --rm -v "${resolved}:/keys" --entrypoint proofline-admin proofline-gateway keygen --private-key-out /keys/server-signing-key.txt
if ($LASTEXITCODE -ne 0) { throw "Unable to generate the server receipt key." }
Write-Host "Generating the offline tombstone key. Configure only the printed public SPKI on the gateway."
docker run --rm -v "${resolved}:/keys" --entrypoint proofline-admin proofline-gateway keygen --private-key-out /keys/admin-private-key.txt
if ($LASTEXITCODE -ne 0) { throw "Unable to generate the offline admin key." }
Write-Warning "Private key files are plaintext. Move this directory into encrypted offline storage now."
