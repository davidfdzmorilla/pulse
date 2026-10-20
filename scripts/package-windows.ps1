# Builds Pulse for Windows x64 (on an x64 Windows, whose Rust builds x64 by
# default) and packs it, in dist\ (run on Windows, from
# anywhere in the repository):
#   - Pulse-<version>-windows-x64.zip: Pulse\pulse.exe and the licence notices
#     (the portable copy, and what the updater downloads);
#   - Pulse-<version>-windows-x64-setup.exe: the installer, if Inno Setup 6 is
#     installed (winget install JRSoftware.InnoSetup, or choco install innosetup).
# The version: -Version, else PULSE_VERSION, else Cargo's. Nothing is published:
# see docs/BUILD_WINDOWS.md to attach the files to a release.
#   pwsh scripts/package-windows.ps1 [-Version 0.2.0] [-SkipBuild] [-NoInstaller]
param(
    [string]$Version = $env:PULSE_VERSION,
    [switch]$SkipBuild,
    [switch]$NoInstaller
)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location $root
if (-not $Version) {
    $Version = (Select-String -Path 'crates/app/Cargo.toml' -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
}
$env:PULSE_VERSION = $Version
Write-Host "Pulse $Version para Windows x64"

if (-not $SkipBuild) {
    cargo build --release -p app
    if ($LASTEXITCODE -ne 0) { throw "La compilación falló" }
}
$exe = Join-Path $root 'target/release/pulse.exe'
if (-not (Test-Path $exe)) { throw "No está $exe" }

$dist = Join-Path $root 'dist'
$stage = Join-Path $dist 'Pulse'
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item $exe (Join-Path $stage 'pulse.exe')

# The notices of what pulse.exe carries inside (models and ONNX Runtime).
$notices = @(
    'packaging/windows/ONNXRUNTIME_LICENSE',
    'crates/beats/models/LICENSE',
    'crates/stems/models/BASIC_PITCH_NOTICE',
    'crates/stems/models/DRUMS_NOTICE'
)
$text = "Pulse $Version — avisos de terceros`r`n`r`n"
foreach ($file in $notices) {
    $text += "===== $file =====`r`n`r`n" + (Get-Content -Raw -Encoding UTF8 $file) + "`r`n`r`n"
}
Set-Content -Path (Join-Path $stage 'THIRD_PARTY_NOTICES.txt') -Value $text -Encoding UTF8

# The version pulse.exe says it is (its version resource; the updater checks it).
$said = (Get-Item (Join-Path $stage 'pulse.exe')).VersionInfo.ProductVersion
if ($said -ne $Version) { throw "pulse.exe dice ser la versión «$said», no la $Version" }

# Zipped with Windows' tar (bsdtar), which the updater also unzips with; names use "/".
$zip = Join-Path $dist "Pulse-$Version-windows-x64.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
& "$env:SystemRoot\System32\tar.exe" -a -c -f $zip -C $dist Pulse
if ($LASTEXITCODE -ne 0) { throw "No se pudo comprimir" }
$hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
Write-Host "$zip`n  SHA-256 $hash"

if (-not $NoInstaller) {
    $iscc = @(
        (Get-Command iscc.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source),
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    ) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if ($iscc) {
        & $iscc "/DAppVersion=$Version" "/DStage=$stage" "/O$dist" 'packaging/windows/pulse.iss'
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup falló" }
        Write-Host (Join-Path $dist "Pulse-$Version-windows-x64-setup.exe")
    } else {
        Write-Warning 'Sin Inno Setup 6: solo el zip (winget install JRSoftware.InnoSetup)'
    }
}
