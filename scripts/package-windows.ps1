# Builds Pulse for Windows x64 (on an x64 Windows, whose Rust builds x64 by
# default) and packs it, in dist\ (run on Windows, from
# anywhere in the repository):
#   - Pulse-<version>-windows-x64.zip: Pulse\pulse.exe, the Visual C++ runtime
#     DLLs it uses and the licence notices (the portable copy, and what the
#     updater downloads);
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

# The Visual C++ runtime (pulse.exe and the ONNX Runtime inside it use it; it
# isn't on every Windows), next to pulse.exe as Microsoft allows: the DLLs of the
# Microsoft.VC14x.CRT folder of the Visual Studio that built it.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = if (Test-Path $vswhere) { & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath } else { $null }
if (-not $vs) { throw 'No se encuentra Visual Studio (o sus Build Tools) con las herramientas de C++' }
$crt = Get-ChildItem -Path "$vs\VC\Redist\MSVC\*\x64\Microsoft.VC14*.CRT" -Directory | Sort-Object FullName | Select-Object -Last 1
if (-not $crt) { throw "No está el runtime de Visual C++ en $vs\VC\Redist\MSVC" }
Copy-Item (Join-Path $crt.FullName '*.dll') $stage
Write-Host "Runtime de Visual C++: $($crt.FullName)"

# What pulse.exe loads (for the record: DirectML.dll and DXCORE.dll must be delay-loaded).
$dumpbin = Get-ChildItem -Path "$vs\VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe" -ErrorAction SilentlyContinue | Select-Object -Last 1
if ($dumpbin) {
    & $dumpbin.FullName /nologo /dependents (Join-Path $stage 'pulse.exe') | Tee-Object -FilePath (Join-Path $dist 'dependencias.txt')
}

# Pulse's licence and the notices of what it carries (scripts/notices.py writes
# them; pulse.exe also shows both in «Acerca de Pulse»).
Copy-Item LICENSE (Join-Path $stage 'LICENSE.txt')
Copy-Item packaging/THIRD_PARTY_NOTICES.txt (Join-Path $stage 'THIRD_PARTY_NOTICES.txt')

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
