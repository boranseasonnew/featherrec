param([int]$Seconds = 30)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type 'using System; using System.Runtime.InteropServices; public static class WorkloadWindow { [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command); }'
$form = New-Object Windows.Forms.Form
$form.Text = 'FeatherRec validation workload — Esc to close'
$form.ClientSize = New-Object Drawing.Size(1000, 650)
$form.StartPosition = 'CenterScreen'
$form.BackColor = [Drawing.Color]::FromArgb(18,20,26)
$form.KeyPreview = $true
$form.Add_KeyDown({ if ($_.KeyCode -eq 'Escape') { $form.Close() } })
$timer = New-Object Windows.Forms.Timer
$timer.Interval = 16
$clock = [Diagnostics.Stopwatch]::StartNew()
$font = New-Object Drawing.Font('Segoe UI', 22)
$brush = New-Object Drawing.SolidBrush([Drawing.Color]::FromArgb(226,74,87))
$form.Add_Paint({
    param($sender, $event)
    $g = $event.Graphics
    $g.DrawString('FeatherRec · motion and text validation', $font, [Drawing.Brushes]::White, 24, 24)
    $g.DrawString(('{0:0.000} seconds' -f $clock.Elapsed.TotalSeconds), $font, [Drawing.Brushes]::White, 24, 78)
    for ($row = 0; $row -lt 8; $row++) {
        $x = 40 + (1 + [Math]::Sin($clock.Elapsed.TotalSeconds * 2 + $row)) * 350
        $g.FillRectangle($brush, [single]$x, [single](160 + $row * 50), 130, 24)
    }
})
$timer.Add_Tick({
    if (-not $form.Tag) {
        [void][WorkloadWindow]::ShowWindow($form.Handle, 5)
        $form.Activate()
        $form.Tag = $true
    }
    if ($clock.Elapsed.TotalSeconds -ge $Seconds) { $form.Close() }
    else { $form.Invalidate() }
})
$timer.Start()
try { [Windows.Forms.Application]::Run($form) }
finally { $timer.Dispose(); $font.Dispose(); $brush.Dispose(); $form.Dispose() }
