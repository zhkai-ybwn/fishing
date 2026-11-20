[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidatePattern('^(0|[1-9][0-9]{0,2})\.(0|[1-9][0-9]{0,2})\.(0|[1-9][0-9]{0,4})$')]
    [string]$Version,

    [string]$BuildDirectory = (Join-Path $PSScriptRoot '../target/release'),

    [string]$OutputDirectory = (Join-Path $PSScriptRoot "../target/installers/$Version"),

    [string]$Wix = 'wix'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$msiVersion = [Version]::Parse($Version)
if ($msiVersion.Major -gt 255 -or $msiVersion.Minor -gt 255 -or $msiVersion.Build -gt 65535) {
    throw "Windows Installer only supports versions up to 255.255.65535."
}

function Find-VcRedistDirectory {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (!(Test-Path -LiteralPath $vswhere -PathType Leaf)) {
        throw 'Visual Studio Installer was not found. Install the Visual C++ x64 build tools.'
    }

    $vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (!$vsPath) { throw 'Visual C++ x64 build tools were not found.' }

    $redistPath = Join-Path $vsPath 'VC/Redist/MSVC'
    $redist = Get-ChildItem -LiteralPath $redistPath -Directory |
        Where-Object { $_.Name -match '^[0-9]+\.[0-9]+\.[0-9]+$' } |
        Sort-Object { [Version]$_.Name } -Descending |
        ForEach-Object { Join-Path $_.FullName 'x64/Microsoft.VC143.CRT' } |
        Where-Object { Test-Path -LiteralPath $_ -PathType Container } |
        Select-Object -First 1
    if (!$redist) { throw 'The redistributable Visual C++ x64 CRT was not found.' }
    $redist
}

function Invoke-WixBuild([string[]]$Arguments) {
    & $Wix @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "WiX build failed with exit code $LASTEXITCODE."
    }
}

$root = Split-Path -Parent $PSScriptRoot
$iconPath = (Resolve-Path -LiteralPath (Join-Path $root 'assets/fishing.ico')).Path
$runtime = & (Join-Path $PSScriptRoot 'Get-OnnxRuntime.ps1')
$redist = Find-VcRedistDirectory
$output = (New-Item -ItemType Directory -Force -Path $OutputDirectory).FullName
$scratchRoot = (New-Item -ItemType Directory -Force -Path (Join-Path $root 'target/packaging-work')).FullName
$scratch = (New-Item -ItemType Directory -Path (Join-Path $scratchRoot ([Guid]::NewGuid().ToString()))).FullName

try {
    $payload = (New-Item -ItemType Directory -Path (Join-Path $scratch 'payload')).FullName
    Copy-Item -LiteralPath (Join-Path $BuildDirectory 'fishing.exe') -Destination $payload
    Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination (Join-Path $payload 'fishing-LICENSE.txt')

    foreach ($name in @('onnxruntime.dll', 'onnxruntime_providers_shared.dll',
                       'ONNX-Runtime-LICENSE.txt', 'ONNX-Runtime-ThirdPartyNotices.txt')) {
        Copy-Item -LiteralPath (Join-Path $runtime $name) -Destination $payload
    }
    # ONNX Runtime's PE import table lists these four redistributable DLLs.
    foreach ($name in @('msvcp140.dll', 'msvcp140_1.dll', 'vcruntime140.dll', 'vcruntime140_1.dll')) {
        Copy-Item -LiteralPath (Join-Path $redist $name) -Destination $payload
    }
    foreach ($model in @('YuNet', 'SFace')) {
        Copy-Item -LiteralPath (Join-Path $root "crates/vision/models/$model-LICENSE") `
            -Destination (Join-Path $payload "$model-LICENSE.txt")
    }
    Copy-Item -LiteralPath (Join-Path $root 'assets/fonts/OFL.txt') `
        -Destination (Join-Path $payload 'Noto-Sans-SC-OFL.txt')
    & (Join-Path $PSScriptRoot 'Write-RustNotices.ps1') `
        -OutputPath (Join-Path $payload 'Rust-Dependency-Licenses.txt')
    $includeNotices = 'no'
    $notices = Join-Path $root 'THIRD_PARTY_NOTICES.md'
    if (Test-Path -LiteralPath $notices -PathType Leaf) {
        Copy-Item -LiteralPath $notices -Destination $payload
        $includeNotices = 'yes'
    }

    $baseName = "fishing-$Version-windows-x64"
    $msi = Join-Path $scratch "$baseName.msi"
    $exe = Join-Path $scratch "$baseName-setup.exe"

    Invoke-WixBuild @('build', '-arch', 'x64', '-pdbtype', 'none',
        '-d', "Version=$Version", '-d', "PayloadDirectory=$payload", '-d', "IncludeNotices=$includeNotices",
        '-d', "IconPath=$iconPath",
        '-out', $msi, (Join-Path $PSScriptRoot 'Package.wxs'))
    # ICE91 compares user directories with a machine-wide install. This package is per-user.
    Invoke-WixBuild @('msi', 'validate', '-sice', 'ICE91', $msi)
    Invoke-WixBuild @('build', '-arch', 'x64', '-pdbtype', 'none',
        '-ext', 'WixToolset.BootstrapperApplications.wixext',
        '-d', "Version=$Version", '-d', "MsiPath=$msi", '-d', "IconPath=$iconPath",
        '-out', $exe, (Join-Path $PSScriptRoot 'Bundle.wxs'))

    $embedded = Join-Path $scratch 'embedded'
    Invoke-WixBuild @('burn', 'extract', $exe, '-o', $embedded)
    $msiHash = (Get-FileHash -LiteralPath $msi -Algorithm SHA256).Hash
    $payloadHashes = Get-ChildItem -LiteralPath $embedded -Recurse -File | Get-FileHash -Algorithm SHA256
    if ($msiHash -notin $payloadHashes.Hash) {
        throw 'The EXE does not contain the standalone MSI.'
    }

    $hashes = foreach ($file in @($msi, $exe)) {
        $hash = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $(Split-Path -Leaf $file)"
    }
    $checksumFile = Join-Path $scratch 'SHA256SUMS.txt'
    [IO.File]::WriteAllText($checksumFile, ($hashes -join "`n") + "`n", [Text.Encoding]::ASCII)
    foreach ($file in @($msi, $exe, $checksumFile)) {
        Move-Item -LiteralPath $file -Destination $output -Force
    }
    Write-Host "Installers written to $output"
} finally {
    # Only delete the directory generated by this invocation.
    if ((Split-Path -Parent $scratch) -eq $scratchRoot) {
        Remove-Item -LiteralPath $scratch -Recurse -Force
    }
}
