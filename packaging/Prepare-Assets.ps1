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
$fontCommit = '523d033d6cb47f4a80c58a35753646f5c3608a78'
$fontBase = "https://raw.githubusercontent.com/notofonts/noto-cjk/$fontCommit/Sans/SubsetOTF/SC"
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
        Path = 'assets/fonts/NotoSansSC-Regular.otf'
        Url = "$fontBase/NotoSansSC-Regular.otf"
        Sha256 = 'faa6c9df652116dde789d351359f3d7e5d2285a2b2a1f04a2d7244df706d5ea9'
    },
    @{
        Path = 'assets/fonts/NotoSansSC-Bold.otf'
        Url = "$fontBase/NotoSansSC-Bold.otf"
        Sha256 = 'c6cb5a93abaa9edc8ee7463b7ebb7f42d618d40e6ed2f7a5371c97b0b64767c0'
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
