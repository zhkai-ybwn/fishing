[CmdletBinding()]
param(
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '../target/dependencies/onnxruntime-1.22.1-win-x64')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Extract only the native CPU runtime and notices from Microsoft's published wheel.
# No Python code or runtime is installed or packaged.
$url = 'https://files.pythonhosted.org/packages/5d/54/7139d463bb0a312890c9a5db87d7815d4a8cce9e6f5f28d04f0b55fcb160/onnxruntime-1.22.1-cp312-cp312-win_amd64.whl'
$sha256 = '6A64291D57EA966A245F749EB970F4FA05A64D26672E05A83FDB5DB6B7D62F87'
$output = (New-Item -ItemType Directory -Force -Path $OutputDirectory).FullName
$archive = Join-Path $output 'onnxruntime.whl'

if (!(Test-Path -LiteralPath $archive -PathType Leaf) -or
    (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $sha256) {
    $partial = Join-Path $output ([Guid]::NewGuid().ToString() + '.download')
    try {
        & curl.exe --fail --location --retry 2 --silent --show-error --output $partial $url
        if ($LASTEXITCODE -ne 0) {
            throw "Downloading ONNX Runtime failed with exit code $LASTEXITCODE."
        }
        if ((Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash -ne $sha256) {
            throw 'The ONNX Runtime archive SHA-256 did not match.'
        }
        Move-Item -LiteralPath $partial -Destination $archive -Force
    } finally {
        if (Test-Path -LiteralPath $partial) { Remove-Item -LiteralPath $partial }
    }
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$entries = @{
    'onnxruntime/capi/onnxruntime.dll'                  = 'onnxruntime.dll'
    'onnxruntime/capi/onnxruntime_providers_shared.dll' = 'onnxruntime_providers_shared.dll'
    'onnxruntime/LICENSE'                              = 'ONNX-Runtime-LICENSE.txt'
    'onnxruntime/ThirdPartyNotices.txt'                = 'ONNX-Runtime-ThirdPartyNotices.txt'
}
$zip = [System.IO.Compression.ZipFile]::OpenRead($archive)
try {
    foreach ($entryName in $entries.Keys) {
        $entry = $zip.GetEntry($entryName)
        if (!$entry) { throw "Missing ONNX Runtime archive entry: $entryName" }
        $destination = Join-Path $output $entries[$entryName]
        if ((Test-Path -LiteralPath $destination -PathType Leaf) -and
            (Get-Item -LiteralPath $destination).Length -eq $entry.Length) {
            continue
        }
        [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $destination, $true)
    }
} finally {
    $zip.Dispose()
}

$output
