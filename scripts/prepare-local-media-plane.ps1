[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$repositoryRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $repositoryRoot

function Get-DotEnvValue {
    param([Parameter(Mandatory)][string]$Name)

    if (-not (Test-Path -LiteralPath ".env" -PathType Leaf)) { return $null }

    $escapedName = [regex]::Escape($Name)
    foreach ($line in Get-Content -LiteralPath ".env") {
        if ($line -match "^\s*$escapedName\s*=\s*(.*)$") {
            $value = $Matches[1].Trim()
            if ($value.Length -ge 2) {
                $first = $value[0]
                $last = $value[$value.Length - 1]
                if (($first -eq '"' -and $last -eq '"') -or ($first -eq "'" -and $last -eq "'")) {
                    $value = $value.Substring(1, $value.Length - 2)
                }
            }
            return $value
        }
    }
    return $null
}

function New-LocalReceiptKey {
    param([Parameter(Mandatory)][string]$Path)

    $directory = Split-Path -Parent $Path
    New-Item -ItemType Directory -Force -Path $directory | Out-Null

    $key = [Security.Cryptography.ECDsa]::Create()
    try {
        $key.GenerateKey([Security.Cryptography.ECCurve]::CreateFromFriendlyName("nistP256"))
        $encoded = ([Convert]::ToBase64String($key.ExportPkcs8PrivateKey())).TrimEnd('=').Replace('+', '-').Replace('/', '_')
    } finally {
        $key.Dispose()
    }

    [IO.File]::WriteAllText(
        $Path,
        $encoded + [Environment]::NewLine,
        [Text.UTF8Encoding]::new($false)
    )
}

# Compose reads .env itself, but the startup helper also needs to guarantee that
# a stable development receipt identity exists before the gateway is created.
$configuredReceiptKey = [Environment]::GetEnvironmentVariable(
    "PROOFLINE_SERVER_SIGNING_KEY_B64",
    [EnvironmentVariableTarget]::Process
)
if ([string]::IsNullOrWhiteSpace($configuredReceiptKey)) {
    $configuredReceiptKey = Get-DotEnvValue -Name "PROOFLINE_SERVER_SIGNING_KEY_B64"
}

if ([string]::IsNullOrWhiteSpace($configuredReceiptKey)) {
    $localKeyPath = Join-Path $repositoryRoot "private\local-server-signing-key.txt"
    if (-not (Test-Path -LiteralPath $localKeyPath -PathType Leaf)) {
        New-LocalReceiptKey -Path $localKeyPath
        Write-Warning "Generated a plaintext development receipt key at $localKeyPath. Keep private/ protected and never use this key as a production trust identity."
    }
    $configuredReceiptKey = (Get-Content -Raw -LiteralPath $localKeyPath).Trim()
}

$env:PROOFLINE_SERVER_SIGNING_KEY_B64 = $configuredReceiptKey
$env:PROOFLINE_ALLOW_EPHEMERAL_SIGNING_KEY = "false"

# The host-side media integration client signs its internal setup requests with
# the same secret Compose supplies to the gateway. Export it for callers such
# as verify-local.ps1 without printing it or copying it into command arguments.
if ([string]::IsNullOrWhiteSpace($env:PROOFLINE_INTERNAL_SECRET)) {
    $configuredInternalSecret = Get-DotEnvValue -Name "PROOFLINE_INTERNAL_SECRET"
    if (-not [string]::IsNullOrWhiteSpace($configuredInternalSecret)) {
        $env:PROOFLINE_INTERNAL_SECRET = $configuredInternalSecret
    }
}

# POSTGRES_PASSWORD initializes a new volume but PostgreSQL deliberately does
# not change an existing role when .env changes. The official image trusts its
# local Unix socket, so reconcile only that role password without deleting or
# recreating the database volume. psql reads both values from container-local
# environment variables; the password is never placed in host logs or argv.
& docker compose up -d --wait postgres
if ($LASTEXITCODE -ne 0) { throw "PostgreSQL did not become ready." }

$passwordSyncSql = @'
\getenv proofline_role POSTGRES_USER
\getenv proofline_password POSTGRES_PASSWORD
ALTER ROLE :"proofline_role" WITH PASSWORD :'proofline_password';
'@
$passwordSyncSql | & docker compose exec -T postgres sh -c 'exec psql --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" --set ON_ERROR_STOP=1'
if ($LASTEXITCODE -ne 0) { throw "Unable to synchronize the PostgreSQL role password with the current Compose environment." }

Write-Host "Local media-plane credentials are synchronized without replacing persistent data."
