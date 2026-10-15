$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$assetDirectory = Join-Path (Split-Path $PSScriptRoot -Parent) 'assets'
$images = @()
foreach ($size in @(16,24,32,48,64,128,256)) {
    $bitmap = [Drawing.Bitmap]::new($size,$size,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $background = [Drawing.SolidBrush]::new([Drawing.ColorTranslator]::FromHtml('#181a1f'))
    $feather = [Drawing.SolidBrush]::new([Drawing.ColorTranslator]::FromHtml('#e24a57'))
    $pen = [Drawing.Pen]::new([Drawing.ColorTranslator]::FromHtml('#f4eeee'),3)
    $pen.StartCap = $pen.EndCap = [Drawing.Drawing2D.LineCap]::Round
    $round = [Drawing.Drawing2D.GraphicsPath]::new()
    $shape = [Drawing.Drawing2D.GraphicsPath]::new()
    $memory = [IO.MemoryStream]::new()
    try {
        $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $graphics.Clear([Drawing.Color]::Transparent)
        $graphics.ScaleTransform($size/64.0,$size/64.0)
        $round.AddArc(0,0,32,32,180,90); $round.AddArc(32,0,32,32,270,90)
        $round.AddArc(32,32,32,32,0,90); $round.AddArc(0,32,32,32,90,90); $round.CloseFigure()
        $graphics.FillPath($background,$round)
        # Geometry matches assets/feather.svg.
        $shape.AddBezier(18,47,15,30,26,13,49,14)
        $shape.AddBezier(49,14,50,34,39,47,18,47); $shape.CloseFigure()
        $graphics.FillPath($feather,$shape)
        $graphics.DrawLine($pen,14,52,41,25); $graphics.DrawLine($pen,23,42,23,30); $graphics.DrawLine($pen,31,34,42,34)
        $bitmap.Save($memory,[Drawing.Imaging.ImageFormat]::Png)
        $images += @{Size=$size;Bytes=$memory.ToArray()}
        if($size -eq 256){[IO.File]::WriteAllBytes((Join-Path $assetDirectory 'featherrec.png'),$memory.ToArray())}
    } finally { $memory.Dispose(); $shape.Dispose(); $round.Dispose(); $pen.Dispose(); $feather.Dispose(); $background.Dispose(); $graphics.Dispose(); $bitmap.Dispose() }
}
$file = [IO.File]::Create((Join-Path $assetDirectory 'featherrec.ico'))
$writer = [IO.BinaryWriter]::new($file)
try {
    $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$images.Count)
    $offset = 6 + 16*$images.Count
    foreach($image in $images){
        $dimension = if($image.Size -eq 256){0}else{$image.Size}
        $writer.Write([byte]$dimension); $writer.Write([byte]$dimension); $writer.Write([byte]0); $writer.Write([byte]0)
        $writer.Write([uint16]1); $writer.Write([uint16]32); $writer.Write([uint32]$image.Bytes.Length); $writer.Write([uint32]$offset)
        $offset += $image.Bytes.Length
    }
    foreach($image in $images){$writer.Write([byte[]]$image.Bytes)}
} finally {$writer.Dispose()}
Write-Output 'Generated application PNG and seven-size Windows icon from the feather logo.'
