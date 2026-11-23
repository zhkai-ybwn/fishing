[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Logo,
    [Parameter(Mandatory)]
    [string]$License,
    [Parameter(Mandatory)]
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

function Write-InstallerBitmap([string]$Name, [int]$Height, [Drawing.Rectangle]$Panel, [Drawing.Rectangle]$Mark) {
    $bitmap = [Drawing.Bitmap]::new(493, $Height, [Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $surface = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(232, 243, 246))
    $accent = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(33, 109, 125))
    try {
        $graphics.Clear([Drawing.Color]::White)
        $graphics.FillRectangle($surface, $Panel)
        $graphics.FillRectangle($accent, $Panel.X, $Panel.Bottom - 3, $Panel.Width, 3)
        $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $graphics.DrawImage($fish, $Mark)
        $bitmap.Save((Join-Path $OutputDirectory $Name), [Drawing.Imaging.ImageFormat]::Bmp)
    } finally {
        $accent.Dispose()
        $surface.Dispose()
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

$fish = [Drawing.Image]::FromFile($Logo)
try {
    $banner = [Drawing.Rectangle]::new(0, 0, 493, 58)
    $bannerMark = [Drawing.Rectangle]::new(442, 5, 48, 48)
    Write-InstallerBitmap 'banner.bmp' 58 $banner $bannerMark
    $sidebar = [Drawing.Rectangle]::new(0, 0, 164, 312)
    $sidebarMark = [Drawing.Rectangle]::new(21, 95, 122, 122)
    Write-InstallerBitmap 'dialog.bmp' 312 $sidebar $sidebarMark
} finally {
    $fish.Dispose()
}

$lines = Get-Content -LiteralPath $License
$escaped = $lines | ForEach-Object { $_.Replace('\', '\\').Replace('{', '\{').Replace('}', '\}') }
$rtf = '{\rtf1\ansi{\fonttbl{\f0 Segoe UI;}}\f0\fs20 ' + ($escaped -join '\par ') + '}'
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'license.rtf'), $rtf, [Text.Encoding]::ASCII)
