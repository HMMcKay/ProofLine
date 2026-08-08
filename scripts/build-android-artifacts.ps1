[CmdletBinding()]
param(
    [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\artifacts\android")
)

$ErrorActionPreference = "Stop"
$androidDirectory = (Resolve-Path (Join-Path $PSScriptRoot "..\android")).Path
$output = [IO.Path]::GetFullPath($OutputDirectory)
$temporaryDirectory = Join-Path ([IO.Path]::GetTempPath()) ("proofline-release-" + [Guid]::NewGuid().ToString("N"))
$keystore = Join-Path $temporaryDirectory "development-release.jks"
$password = [Convert]::ToBase64String([Security.Cryptography.RandomNumberGenerator]::GetBytes(32)).Replace("=", "").Replace("+", "A").Replace("/", "B")

function Get-ZipPayloadHash {
    param([string]$Path, [string[]]$Ignore = @())
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead((Resolve-Path $Path))
    $hash = [Security.Cryptography.IncrementalHash]::CreateHash([Security.Cryptography.HashAlgorithmName]::SHA256)
    try {
        foreach ($entry in ($archive.Entries | Sort-Object FullName)) {
            if ($Ignore | Where-Object { $entry.FullName -like $_ }) { continue }
            $hash.AppendData([Text.Encoding]::UTF8.GetBytes($entry.FullName + [char]0))
            $stream = $entry.Open()
            try {
                $buffer = [byte[]]::new(65536)
                while (($read = $stream.Read($buffer, 0, $buffer.Length)) -gt 0) {
                    $hash.AppendData($buffer, 0, $read)
                }
            } finally { $stream.Dispose() }
        }
        return [Convert]::ToHexString($hash.GetHashAndReset())
    } finally {
        $hash.Dispose()
        $archive.Dispose()
    }
}

New-Item -ItemType Directory -Force -Path $output, $temporaryDirectory | Out-Null

try {
    # Use the commonplace production-compatible RSA path for the disposable
    # development signature. The stable production identity still belongs in
    # an operator-managed keystore or HSM, never this repository.
    & keytool -genkeypair -keystore $keystore -storepass $password -keypass $password `
        -alias proofline-development -keyalg RSA -keysize 3072 -validity 3650 `
        -dname "CN=ProofLine Development Build, OU=Development Only, O=ProofLine, C=US" 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "keytool failed with exit code $LASTEXITCODE" }

    $env:PROOFLINE_KEYSTORE_PATH = $keystore
    $env:PROOFLINE_KEYSTORE_PASSWORD = $password
    $env:PROOFLINE_KEY_ALIAS = "proofline-development"
    $env:PROOFLINE_KEY_PASSWORD = $password

    Push-Location $androidDirectory
    try {
        & .\gradlew.bat clean assembleRelease bundleRelease --no-build-cache
        if ($LASTEXITCODE -ne 0) { throw "First Android release build failed" }
        $firstApk = (Get-FileHash "app\build\outputs\apk\release\app-release.apk" -Algorithm SHA256).Hash
        $firstAab = (Get-FileHash "app\build\outputs\bundle\release\app-release.aab" -Algorithm SHA256).Hash
        $firstApkPayload = Get-ZipPayloadHash "app\build\outputs\apk\release\app-release.apk" @("META-INF/*")
        $firstAabPayload = Get-ZipPayloadHash "app\build\outputs\bundle\release\app-release.aab" @("META-INF/*", "BUNDLE-METADATA/com.android.tools/r8.json")
        Copy-Item "app\build\outputs\apk\release\app-release.apk" (Join-Path $output "proofline-v2-development-signed-release.apk") -Force
        Copy-Item "app\build\outputs\bundle\release\app-release.aab" (Join-Path $output "proofline-v2-development-signed-release.aab") -Force

        # Android's APK signing block and R8's buildTimeNs diagnostic are not
        # byte-stable. Compare every runtime payload entry while excluding only
        # signature containers and that documented diagnostic record.
        & .\gradlew.bat clean assembleRelease bundleRelease --no-build-cache
        if ($LASTEXITCODE -ne 0) { throw "Second Android release build failed" }
        $secondApk = (Get-FileHash "app\build\outputs\apk\release\app-release.apk" -Algorithm SHA256).Hash
        $secondAab = (Get-FileHash "app\build\outputs\bundle\release\app-release.aab" -Algorithm SHA256).Hash
        $secondApkPayload = Get-ZipPayloadHash "app\build\outputs\apk\release\app-release.apk" @("META-INF/*")
        $secondAabPayload = Get-ZipPayloadHash "app\build\outputs\bundle\release\app-release.aab" @("META-INF/*", "BUNDLE-METADATA/com.android.tools/r8.json")
        if ($firstApkPayload -ne $secondApkPayload -or $firstAabPayload -ne $secondAabPayload) {
            throw "Clean release runtime payloads were not reproducible"
        }

        & .\gradlew.bat assembleDebug
        if ($LASTEXITCODE -ne 0) { throw "Android debug build failed" }
        Copy-Item "app\build\outputs\apk\debug\app-debug.apk" (Join-Path $output "proofline-v2-debug.apk") -Force
    } finally {
        Pop-Location
    }

    $result = [ordered]@{
        PayloadReproducible = $true
        ByteReproducible = ($firstApk -eq $secondApk -and $firstAab -eq $secondAab)
        ApkSha256 = $firstApk
        AabSha256 = $firstAab
        ApkPayloadSha256 = $firstApkPayload
        AabPayloadSha256 = $firstAabPayload
        OutputDirectory = $output
        SigningIdentity = "ephemeral development key (not a production trust identity)"
        ExcludedFromPayloadComparison = @("APK/AAB signature containers", "R8 buildTimeNs diagnostic metadata")
    }
    $result | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $output "reproducibility.json")
    [pscustomobject]$result
    Get-ChildItem -LiteralPath $output -File | Get-FileHash -Algorithm SHA256 | Select-Object Path, Hash
} finally {
    # The temporary development signing key is deliberately not retained. A
    # production operator must provide a stable protected keystore via env vars.
    if (Test-Path -LiteralPath $temporaryDirectory) {
        Remove-Item -LiteralPath $temporaryDirectory -Recurse -Force
    }
    Remove-Item Env:PROOFLINE_KEYSTORE_PATH, Env:PROOFLINE_KEYSTORE_PASSWORD, Env:PROOFLINE_KEY_ALIAS, Env:PROOFLINE_KEY_PASSWORD -ErrorAction SilentlyContinue
}
