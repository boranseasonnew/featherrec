param([int]$Seconds=24)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type 'using System;using System.Runtime.InteropServices;public static class ResizeWindow {[DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr window,int command);}'
$form=[Windows.Forms.Form]::new()
$form.Text='FeatherRec resize validation'
$form.FormBorderStyle=[Windows.Forms.FormBorderStyle]::None
$form.ClientSize=[Drawing.Size]::new(960,540)
$form.StartPosition='CenterScreen'
$form.BackColor=[Drawing.Color]::FromArgb(24,32,48)
$form.KeyPreview=$true
$form.Add_KeyDown({if($_.KeyCode -eq 'Escape'){$form.Close()}})
$clock=[Diagnostics.Stopwatch]::StartNew()
$font=[Drawing.Font]::new('Segoe UI',24)
$form.Add_Paint({param($sender,$event)
    $g=$event.Graphics; $width=$form.ClientSize.Width; $height=$form.ClientSize.Height
    $g.FillRectangle([Drawing.Brushes]::DarkRed,0,0,[single]($width/2),[single]($height/2))
    $g.FillRectangle([Drawing.Brushes]::DarkGreen,[single]($width/2),0,[single]($width/2),[single]($height/2))
    $g.FillRectangle([Drawing.Brushes]::DarkBlue,0,[single]($height/2),[single]($width/2),[single]($height/2))
    $g.FillRectangle([Drawing.Brushes]::DarkGoldenrod,[single]($width/2),[single]($height/2),[single]($width/2),[single]($height/2))
    $g.DrawString("$width x $height",$font,[Drawing.Brushes]::White,20,20)
    $g.DrawString(('{0:0.00} seconds' -f $clock.Elapsed.TotalSeconds),$font,[Drawing.Brushes]::White,20,70)
    $g.FillEllipse([Drawing.Brushes]::White,[single](($width-60)*(1+[Math]::Sin($clock.Elapsed.TotalSeconds*3))/2),[single]($height/2),50,50)
})
$timer=[Windows.Forms.Timer]::new(); $timer.Interval=16
$script:lastPhase=0
$timer.Add_Tick({
    if(-not $form.Tag){[void][ResizeWindow]::ShowWindow($form.Handle,5);$form.Tag=$true}
    $phase=[Math]::Min(4,[int][Math]::Floor($clock.Elapsed.TotalSeconds/4))
    if($phase -ne $script:lastPhase){
        $sizes=@(@(960,540),@(1280,720),@(800,600),@(1600,900),@(960,540))
        $form.ClientSize=[Drawing.Size]::new($sizes[$phase][0],$sizes[$phase][1]);$script:lastPhase=$phase
    }
    if($clock.Elapsed.TotalSeconds -ge $Seconds){$form.Close()}else{$form.Invalidate()}
})
$timer.Start()
try{[Windows.Forms.Application]::Run($form)}finally{$timer.Dispose();$font.Dispose();$form.Dispose()}
