[CmdletBinding()]
param([string]$Output = "artifacts/proofline-sbom.cdx.json")
$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$target = [IO.Path]::GetFullPath((Join-Path $repo $Output))
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
docker run --rm -v "${repo}:/src:ro" anchore/syft:v1.50.0@sha256:1288ea4c8b38767b4e620c1e312c8cb26b6e887a99b4f07ab6cd19fc6f225026 dir:/src `
    --exclude './node_modules/**' --exclude './target/**' --exclude './android/.gradle/**' `
    --exclude './android/app/build/**' --exclude './artifacts/**' --exclude './.tools/**' `
    --exclude './.vinext/**' --exclude './dist/**' --exclude './tmp/**' `
    -o cyclonedx-json | Set-Content -Encoding utf8 $target
if ($LASTEXITCODE -ne 0) { throw "SBOM generation failed." }
Write-Host "CycloneDX SBOM written to $target"
