[CmdletBinding()]
param(
    [string]$ToolVersion = "0.26.60",
    [string]$Ffmpeg = "ffmpeg"
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$example = Join-Path $repo "examples\c2pa"
$inputDir = Join-Path $example "input"
$toolDir = Join-Path $repo ".tools\c2patool"
$tool = Join-Path $toolDir "c2patool\c2patool.exe"

if (-not (Test-Path -LiteralPath $tool)) {
    New-Item -ItemType Directory -Force -Path $toolDir | Out-Null
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/contentauth/c2pa-rs/releases/tags/c2patool-v$ToolVersion" -Headers @{ "User-Agent" = "ProofLine-build" }
    $assetName = "c2patool-v$ToolVersion-x86_64-pc-windows-msvc.zip"
    $asset = $release.assets | Where-Object name -eq $assetName
    if (-not $asset) { throw "Official release asset $assetName was not found." }
    $archive = Join-Path $repo ".tools\c2patool.zip"
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $archive
    Expand-Archive -LiteralPath $archive -DestinationPath $toolDir -Force
}

New-Item -ItemType Directory -Force -Path $inputDir | Out-Null
foreach ($outputName in @("live-video-output", "live-audio-output")) {
    $output = Join-Path $example $outputName
    $resolvedOutput = [IO.Path]::GetFullPath($output)
    $resolvedExample = [IO.Path]::GetFullPath($example) + [IO.Path]::DirectorySeparatorChar
    if (-not $resolvedOutput.StartsWith($resolvedExample, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to clear C2PA output outside the fixture directory: $resolvedOutput"
    }
    if (Test-Path -LiteralPath $output) { Remove-Item -LiteralPath $output -Recurse -Force }
    New-Item -ItemType Directory -Path $output | Out-Null
}

Push-Location $inputDir
try {
    # FFmpeg resolves the init/media templates relative to its working directory,
    # not the MPD path. Running here keeps every generated fixture under input/.
    & $Ffmpeg -hide_banner -loglevel error -y `
        -f lavfi -i "testsrc2=size=640x360:rate=30" `
        -f lavfi -i "sine=frequency=1000:sample_rate=48000" `
        -t 6 -map 0:v:0 -map 1:a:0 `
        -c:v libx264 -preset veryfast -pix_fmt yuv420p -g 60 -keyint_min 60 -sc_threshold 0 `
        -c:a aac -b:a 128k -f dash -seg_duration 2 -use_timeline 0 -use_template 1 `
        -init_seg_name 'init-$RepresentationID$.mp4' `
        -media_seg_name 'fragment-$RepresentationID$-$Number%03d$.m4s' `
        -adaptation_sets 'id=0,streams=v id=1,streams=a' `
        "manifest.mpd"
    $ffmpegExit = $LASTEXITCODE
} finally { Pop-Location }
if ($ffmpegExit -ne 0) { throw "FFmpeg failed to create the CMAF fixture." }

# Older revisions accidentally wrote template outputs at the repository root.
# Remove only byte-identical copies of this freshly generated fixture; anything
# different is preserved and treated as an operator-owned file.
$fixtureNames = @("init-0.mp4", "init-1.mp4", "fragment-0-001.m4s", "fragment-0-002.m4s", "fragment-0-003.m4s", "fragment-1-001.m4s", "fragment-1-002.m4s", "fragment-1-003.m4s", "fragment-1-004.m4s")
foreach ($fixtureName in $fixtureNames) {
    $legacyPath = Join-Path $repo $fixtureName
    $canonicalPath = Join-Path $inputDir $fixtureName
    if ((Test-Path -LiteralPath $legacyPath) -and (Test-Path -LiteralPath $canonicalPath)) {
        $legacyHash = (Get-FileHash -LiteralPath $legacyPath -Algorithm SHA256).Hash
        $canonicalHash = (Get-FileHash -LiteralPath $canonicalPath -Algorithm SHA256).Hash
        if ($legacyHash -eq $canonicalHash) { Remove-Item -LiteralPath $legacyPath -Force }
    }
}

$manifest = Join-Path $example "manifest.json"
& $tool -m $manifest -o (Join-Path $example "live-video-output") (Join-Path $inputDir "init-0.mp4") fragment --fragments_glob "fragment-0-*.m4s"
if ($LASTEXITCODE -ne 0) { throw "Official C2PA live-video authoring failed." }
& $tool -m $manifest -o (Join-Path $example "live-audio-output") (Join-Path $inputDir "init-1.mp4") fragment --fragments_glob "fragment-1-*.m4s"
if ($LASTEXITCODE -ne 0) { throw "Official C2PA live-audio authoring failed." }

$validationSettings = Join-Path $example "development-validation-settings.json"
$videoInit = Join-Path $example "live-video-output\input\init-0.mp4"
$audioInit = Join-Path $example "live-audio-output\input\init-1.mp4"
$videoValidationPath = Join-Path $example "live-video-validation.json"
Push-Location (Split-Path -Parent $videoInit)
try { & $tool --settings $validationSettings (Split-Path -Leaf $videoInit) --detailed fragment --fragments_glob "fragment-0-*.m4s" 2>&1 | Set-Content -Encoding utf8 $videoValidationPath }
finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw "Official C2PA live-video validation failed." }
$audioValidationPath = Join-Path $example "live-audio-validation.json"
Push-Location (Split-Path -Parent $audioInit)
try { & $tool --settings $validationSettings (Split-Path -Leaf $audioInit) --detailed fragment --fragments_glob "fragment-1-*.m4s" 2>&1 | Set-Content -Encoding utf8 $audioValidationPath }
finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw "Official C2PA live-audio validation failed." }

Push-Location (Split-Path -Parent $videoInit)
try { $strictOutput = (& $tool (Split-Path -Leaf $videoInit) fragment --fragments_glob "fragment-0-*.m4s" 2>&1) -join "`n" }
finally { Pop-Location }
$strictOutput | Set-Content -Encoding utf8 (Join-Path $example "strict-trust-validation.txt")
if ($strictOutput -notmatch "signingCredential\.untrusted") {
    throw "Strict validation did not identify the deliberately untrusted development credential."
}

$outcome = [ordered]@{
    generated_at = (Get-Date).ToUniversalTime().ToString("o")
    c2patool_version = (& $tool --version) -join ""
    fixture = "synthetic 640x360 AVC plus AAC CMAF, six seconds"
    live_video_binding = "valid_with_untrusted_development_signing_credential"
    live_audio_binding = "valid_with_untrusted_development_signing_credential"
    production_trust = "not_configured"
    validation_policy = "Asset bindings validated with trust checking disabled; a separate strict run must and does report signingCredential.untrusted."
    note = "The official author and validator are the same pinned executable in this spike. CI should also validate with a separately installed official SDK before production trust is claimed."
}
$outcome | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $example "outcome.json")
Write-Host "C2PA CMAF compatibility spike passed with development credentials."
