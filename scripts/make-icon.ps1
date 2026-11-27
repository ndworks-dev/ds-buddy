# Draws the app icon (ui/icon.png) and wraps it in icon.ico for the .exe.
# The ICO holds a single PNG-compressed 256x256 image, which Windows Vista+ reads.
# Run from the repository root: .\scripts\make-icon.ps1

Add-Type -AssemblyName System.Drawing
$root = Split-Path $PSScriptRoot -Parent
$size = 256

function New-RoundedRect([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    $p
}

$bmp = New-Object System.Drawing.Bitmap $size, $size
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = 'AntiAlias'
$g.Clear([System.Drawing.Color]::Transparent)

# Tile: graphite, like the app background.
$tile = New-RoundedRect 8 8 240 240 56
$g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.ColorTranslator]::FromHtml('#16191E'))), $tile)
$g.DrawPath((New-Object System.Drawing.Pen ([System.Drawing.ColorTranslator]::FromHtml('#2C313A')), 4), $tile)

# Touchpad with the two light bars beside it, as on a DualSense.
$pad = New-RoundedRect 74 70 108 76 16
$g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.ColorTranslator]::FromHtml('#E8EAED'))), $pad)
$rect = New-Object System.Drawing.Rectangle 40, 60, 176, 150
$light = New-Object System.Drawing.Drawing2D.LinearGradientBrush $rect, ([System.Drawing.ColorTranslator]::FromHtml('#4DA3FF')), ([System.Drawing.ColorTranslator]::FromHtml('#5EE6A8')), 90
$g.FillPath($light, (New-RoundedRect 50 66 14 110 7))
$g.FillPath($light, (New-RoundedRect 192 66 14 110 7))

# Pairing pulse under the touchpad.
$pen = New-Object System.Drawing.Pen ([System.Drawing.ColorTranslator]::FromHtml('#5EE6A8')), 10
$pen.StartCap = 'Round'; $pen.EndCap = 'Round'
$g.DrawArc($pen, 98, 150, 60, 60, 30, 120)
$g.DrawArc($pen, 78, 140, 100, 100, 40, 100)
$g.Dispose()

$png = Join-Path $root 'ui\icon.png'
$bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()

$bytes = [System.IO.File]::ReadAllBytes($png)
$ms = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $ms
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]1)          # ICONDIR: reserved, type icon, 1 image
$w.Write([byte]0); $w.Write([byte]0); $w.Write([byte]0); $w.Write([byte]0) # 256x256 (0 = 256), no palette
$w.Write([uint16]1); $w.Write([uint16]32)                                 # planes, bits per pixel
$w.Write([uint32]$bytes.Length); $w.Write([uint32]22)                     # image size, offset
$w.Write($bytes)
[System.IO.File]::WriteAllBytes((Join-Path $root 'icon.ico'), $ms.ToArray())
Write-Output "Wrote ui\icon.png and icon.ico"
