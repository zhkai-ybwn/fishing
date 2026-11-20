#Requires -Version 7.0
[CmdletBinding()]
param(
    [string]$RepositoryRoot = (Join-Path $PSScriptRoot '..')
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path -LiteralPath $RepositoryRoot).Path
if (-not (Test-Path -LiteralPath $root -PathType Container)) {
    throw "Repository root is not a directory: $root"
}

$modelCommit = '48b10fa505aeb51fbd127fc60a5c660040e823b7'
$modelBase = "https://media.githubusercontent.com/media/opencv/opencv_zoo/$modelCommit/models"
$fontCommit = 'a85815a42757630ce188fdad368c2dfc444d4773'
$fontBase = "https://raw.githubusercontent.com/google/fonts/$fontCommit/ofl/notosanssc"
$assets = @(
    @{
        Path = 'crates/vision/models/face_detection_yunet_2026may.onnx'
        Url = "$modelBase/face_detection_yunet/face_detection_yunet_2026may.onnx"
        Sha256 = 'ebafce4e3c118d6554634be5c27ab333b4c047a9a8c3faf1d7cf93101c22f0f0'
    },
    @{
        Path = 'crates/vision/models/face_recognition_sface_2021dec.onnx'
        Url = "$modelBase/face_recognition_sface/face_recognition_sface_2021dec.onnx"
        Sha256 = '0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79'
    },
    @{
        Path = 'assets/fonts/NotoSansSC.ttf'
        Url = "$fontBase/NotoSansSC%5Bwght%5D.ttf"
        Sha256 = 'a3041811a78c361b1de50f953c805e0244951c21c5bd412f7232ef0d899af0da'
    }
)

foreach ($asset in $assets) {
    $destination = Join-Path $root $asset.Path
    if ((Test-Path -LiteralPath $destination -PathType Leaf) -and
        (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -eq $asset.Sha256) {
        Write-Host "Verified $($asset.Path)"
        continue
    }

    $parent = Split-Path -Parent $destination
    [void][IO.Directory]::CreateDirectory($parent)
    $temporary = "$destination.$([Guid]::NewGuid().ToString('N')).tmp"
    try {
        curl.exe --fail --silent --show-error --location --retry 3 --retry-all-errors --connect-timeout 20 --max-time 180 --output $temporary $asset.Url
        if ($LASTEXITCODE -ne 0) {
            throw "Download failed for $($asset.Path) (curl exit $LASTEXITCODE)"
        }
        $actual = (Get-FileHash -LiteralPath $temporary -Algorithm SHA256).Hash
        if ($actual -ne $asset.Sha256) {
            throw "SHA-256 mismatch for $($asset.Path). Expected $($asset.Sha256), received $actual."
        }
        [IO.File]::Move($temporary, $destination, $true)
        Write-Host "Prepared $($asset.Path)"
    }
    finally {
        if (Test-Path -LiteralPath $temporary -PathType Leaf) {
            Remove-Item -LiteralPath $temporary -Force
        }
    }
}
