[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+$')]
    [string]$Version,

    [string]$InstallerDirectory = (Join-Path $PSScriptRoot "../target/installers/$Version")
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$installPath = Join-Path $env:LOCALAPPDATA 'Programs/BossSensor'
$app = Join-Path $installPath 'fishing.exe'
if (Test-Path -LiteralPath $installPath) {
    throw 'Use a clean test user. The fishing installation directory already exists.'
}

$directory = (Resolve-Path -LiteralPath $InstallerDirectory).Path
$baseName = "fishing-$Version-windows-x64"
$msi = (Resolve-Path -LiteralPath (Join-Path $directory "$baseName.msi")).Path
$exe = (Resolve-Path -LiteralPath (Join-Path $directory "$baseName-setup.exe")).Path
$msiexec = Join-Path $env:WINDIR 'System32/msiexec.exe'

function Invoke-Installer([string]$Program, [string]$Arguments, [string]$LogSwitch, [string]$LogName) {
    $log = Join-Path $env:TEMP "fishing-$Version-$LogName.log"
    $process = Start-Process -FilePath $Program -ArgumentList "$Arguments $LogSwitch `"$log`"" `
        -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 0) {
        if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log -Tail 50 | Write-Host }
        throw "$LogName failed with exit code $($process.ExitCode)."
    }
}

function Test-InstalledApplication {
    foreach ($name in @('fishing.exe', 'onnxruntime.dll', 'onnxruntime_providers_shared.dll',
                       'msvcp140.dll', 'msvcp140_1.dll', 'vcruntime140.dll', 'vcruntime140_1.dll',
                       'Rust-Dependency-Licenses.txt')) {
        if (!(Test-Path -LiteralPath (Join-Path $installPath $name) -PathType Leaf)) {
            throw "Missing installed file: $name"
        }
    }

    # Load the runtime from the installed application directory.
    $previousRuntime = $env:ORT_DYLIB_PATH
    try {
        $env:ORT_DYLIB_PATH = $null
        $process = Start-Process -FilePath $app -WorkingDirectory $installPath `
            -ArgumentList '--self-test' -PassThru -WindowStyle Hidden
        if (!$process.WaitForExit(30000)) {
            $process.Kill()
            throw 'The installed application self-test timed out.'
        }
        if ($process.ExitCode -ne 0) {
            throw "The installed application self-test failed with exit code $($process.ExitCode)."
        }
    } finally {
        $env:ORT_DYLIB_PATH = $previousRuntime
    }
}

function Assert-Uninstalled {
    if (Test-Path -LiteralPath $installPath) { throw 'The installation directory remains after uninstall.' }
}

Invoke-Installer $msiexec "/i `"$msi`" /qn /norestart" '/l*v' 'msi-install'
try { Test-InstalledApplication } finally {
    Invoke-Installer $msiexec "/x `"$msi`" /qn /norestart" '/l*v' 'msi-uninstall'
}
Assert-Uninstalled

Invoke-Installer $exe '-install -quiet -norestart' '-log' 'exe-install'
try { Test-InstalledApplication } finally {
    Invoke-Installer $exe '-uninstall -quiet -norestart' '-log' 'exe-uninstall'
}
Assert-Uninstalled
Write-Host 'Both installers installed, ran the application self-test, and uninstalled.'
