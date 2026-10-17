<#
.SYNOPSIS
    Build the OpenLogi Windows MSI locally, mirroring the CI `windows` and
    `windows-msi` jobs in .github/workflows/build.yml (minus code signing).

.DESCRIPTION
    1. cargo build --release of the GUI, overlay, and agent for -Target, with
       the static CRT (RUSTFLAGS="-C target-feature=+crt-static") exactly as CI
       builds them, so the exes need no VC++ Redistributable. This build ships
       no CLI, so openlogi.exe is neither built nor packaged.
    2. Stage the exes under the names the MSI installs (OpenLogi.exe,
       openlogi-agent.exe, openlogi-overlay.exe).
    3. Restore the WiX 6.0.2 local dotnet tool pinned in dotnet-tools.json
       beside this script and run `wix build` on OpenLogi.wxs with the UI and
       Util extensions (installed globally for 6.0.2; added here if missing).

    The MSI lands in <repo>\dist\OpenLogi-v<Version>-windows-<arch>.msi. It is
    unsigned: CI signs with Azure Artifact Signing, which has no local
    equivalent.

    Like CI, the updater's compile-time inputs come from the environment: set
    OPENLOGI_UPDATE_BASE_URL (and OPENLOGI_UPDATE_MINISIGN_PUBLIC_KEY) to bake
    them in; left unset, the build keeps the fail-closed dev behaviour.

.PARAMETER Version
    Numeric x.y.z ProductVersion. Defaults to [workspace.package] version in
    the workspace Cargo.toml.

.PARAMETER Target
    Rust target triple: x86_64-pc-windows-msvc (default) or
    aarch64-pc-windows-msvc.

.PARAMETER BinDir
    Package the exes already in this directory (openlogi-desktop.exe,
    openlogi-overlay.exe, openlogi-agent.exe) instead of building them.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File packaging\windows\build-msi.ps1

.EXAMPLE
    .\packaging\windows\build-msi.ps1 -Version 0.8.14 -BinDir target\x86_64-pc-windows-msvc\release
#>
[CmdletBinding()]
param(
    [string]$Version,
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$Target = 'x86_64-pc-windows-msvc',
    [string]$BinDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$WixVersion = '6.0.2'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Wxs = Join-Path $PSScriptRoot 'OpenLogi.wxs'

# Native tools report failure through $LASTEXITCODE, which
# $ErrorActionPreference does not see. Windows PowerShell 5.1 also turns any
# stderr line (cargo's progress output) into a terminating error under 'Stop'
# once output is redirected, so relax it for the call and judge by exit code.
function Invoke-Native {
    param([string]$What, [scriptblock]$Command)
    $ErrorActionPreference = 'Continue'
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$What failed with exit code $LASTEXITCODE"
    }
}

if (-not $Version) {
    $manifest = Get-Content -Raw (Join-Path $RepoRoot 'Cargo.toml')
    if ($manifest -notmatch '(?ms)^\[workspace\.package\][^\[]*?^version\s*=\s*"([^"]+)"') {
        throw 'could not read [workspace.package] version from Cargo.toml; pass -Version'
    }
    $Version = $Matches[1]
}
# Windows Installer's ProductVersion is numeric; a pre-release suffix would
# fail deep inside wix with a far less obvious message.
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "-Version must be a numeric x.y.z ProductVersion, got '$Version'"
}

$arch = @{ 'x86_64-pc-windows-msvc' = 'x86_64'; 'aarch64-pc-windows-msvc' = 'arm64' }[$Target]
$msiPlatform = @{ 'x86_64-pc-windows-msvc' = 'x64'; 'aarch64-pc-windows-msvc' = 'arm64' }[$Target]

Push-Location $RepoRoot
try {
    if ($BinDir) {
        $releaseDir = (Resolve-Path $BinDir).Path
        Write-Host "Packaging prebuilt exes from $releaseDir"
    } else {
        $targetRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $RepoRoot 'target' }
        $releaseDir = Join-Path $targetRoot "$Target\release"

        # The explicit --target keeps the static-CRT flag off host build
        # scripts, which RUSTFLAGS would otherwise also reach (same reasoning
        # as the CI job's env block).
        $savedRustflags = $env:RUSTFLAGS
        $savedManifestUrl = $env:OPENLOGI_UPDATE_MANIFEST_URL
        $env:RUSTFLAGS = '-C target-feature=+crt-static'
        if ($env:OPENLOGI_UPDATE_BASE_URL) {
            $env:OPENLOGI_UPDATE_MANIFEST_URL = $env:OPENLOGI_UPDATE_BASE_URL.TrimEnd('/') + '/channels/stable/latest.json'
        }
        try {
            Invoke-Native 'cargo build' {
                cargo build --release -p openlogi-desktop -p openlogi-overlay -p openlogi-agent --target $Target
            }
        } finally {
            $env:RUSTFLAGS = $savedRustflags
            $env:OPENLOGI_UPDATE_MANIFEST_URL = $savedManifestUrl
        }
    }

    # Staged under the installed names; the wxs takes each path as a define.
    $stage = Join-Path $RepoRoot "target\msi-stage\$arch"
    if (Test-Path $stage) {
        Remove-Item -Recurse -Force $stage
    }
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    $staged = [ordered]@{
        'openlogi-desktop.exe' = 'OpenLogi.exe'
        'openlogi-agent.exe'   = 'openlogi-agent.exe'
        'openlogi-overlay.exe' = 'openlogi-overlay.exe'
    }
    foreach ($built in $staged.Keys) {
        $source = Join-Path $releaseDir $built
        if (-not (Test-Path $source -PathType Leaf)) {
            throw "missing $source - build it first or point -BinDir at it"
        }
        Copy-Item $source (Join-Path $stage $staged[$built])
    }

    # wix writes a .wixpdb beside its output; keep both in the stage and copy
    # only the MSI to dist\, as CI's collect step does.
    $wixOut = Join-Path $stage 'out\OpenLogi.msi'
    $dist = Join-Path $RepoRoot 'dist'
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    $msi = Join-Path $dist "OpenLogi-v$Version-windows-$arch.msi"

    # `dotnet tool` finds the manifest by walking up from the working
    # directory, so run it from beside dotnet-tools.json.
    Push-Location $PSScriptRoot
    try {
        Invoke-Native 'dotnet tool restore' { dotnet tool restore }
        $extensions = (& dotnet tool run wix -- extension list --global) -join "`n"
        foreach ($ext in 'WixToolset.UI.wixext', 'WixToolset.Util.wixext') {
            if ($extensions -notmatch [regex]::Escape("$ext $WixVersion")) {
                Invoke-Native "wix extension add $ext" {
                    dotnet tool run wix -- extension add --global "$ext/$WixVersion"
                }
            }
        }
        Invoke-Native 'wix build' {
            dotnet tool run wix -- build $Wxs `
                -ext WixToolset.UI.wixext `
                -ext WixToolset.Util.wixext `
                -arch $msiPlatform `
                -d "Version=$Version" `
                -d "ExeFile=$(Join-Path $stage 'OpenLogi.exe')" `
                -d "AgentExeFile=$(Join-Path $stage 'openlogi-agent.exe')" `
                -d "OverlayExeFile=$(Join-Path $stage 'openlogi-overlay.exe')" `
                -o $wixOut
        }
    } finally {
        Pop-Location
    }
    Copy-Item $wixOut $msi -Force

    $size = [math]::Round((Get-Item $msi).Length / 1MB, 1)
    Write-Host "Built $msi ($size MB, unsigned)"
} finally {
    Pop-Location
}
