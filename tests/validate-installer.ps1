$ErrorActionPreference = 'Stop'
if($env:GITHUB_ACTIONS -ne 'true'){throw 'Run this installation/uninstallation check only on a disposable GitHub runner'}
$repo = Split-Path $PSScriptRoot -Parent
$installer = Get-ChildItem (Join-Path $repo 'target') -Filter 'FeatherRec-Setup-*.exe' | Select-Object -First 1
if(-not $installer){throw 'Installer missing'}
$destination = Join-Path $env:RUNNER_TEMP 'FeatherRec-installer-check'
$setup = Start-Process -FilePath $installer.FullName -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/SP-',"/DIR=`"$destination`"") -WindowStyle Hidden -PassThru -Wait
if($setup.ExitCode -ne 0){throw "Setup failed: $($setup.ExitCode)"}
try {
    $exe = Join-Path $destination 'featherrec.exe'
    if(-not (Test-Path -LiteralPath $exe)){throw 'Installed GUI executable missing'}
    $sourceHash=(Get-FileHash (Join-Path $repo 'target/release/featherrec.exe') -Algorithm SHA256).Hash
    if((Get-FileHash $exe -Algorithm SHA256).Hash -ne $sourceHash){throw 'Installed executable does not match build'}
    if([Diagnostics.FileVersionInfo]::GetVersionInfo($exe).ProductName -ne 'FeatherRec'){throw 'Application metadata missing'}
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class AppIconCheck {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] static extern IntPtr LoadLibraryEx(string path,IntPtr file,uint flags);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] static extern IntPtr FindResource(IntPtr module,IntPtr name,IntPtr type);
 [DllImport("kernel32.dll")] static extern bool FreeLibrary(IntPtr module);
 public static bool HasIcon(string path){var module=LoadLibraryEx(path,IntPtr.Zero,2);if(module==IntPtr.Zero)return false;try{return FindResource(module,new IntPtr(1),new IntPtr(14))!=IntPtr.Zero;}finally{FreeLibrary(module);}}
}
'@
    if(-not [AppIconCheck]::HasIcon($exe)){throw 'Embedded EXE icon missing'}
    $shortcut=Join-Path $env:APPDATA 'Microsoft/Windows/Start Menu/Programs/FeatherRec.lnk'
    if(-not (Test-Path -LiteralPath $shortcut)){throw 'Start menu shortcut missing'}
    Write-Output 'Installer, installed-file checksum, version metadata, embedded icon and Start menu shortcut passed.'
} finally {
    $uninstaller=Join-Path $destination 'unins000.exe'
    if(Test-Path -LiteralPath $uninstaller){
        $removed=Start-Process -FilePath $uninstaller -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART') -WindowStyle Hidden -PassThru -Wait
        if($removed.ExitCode -ne 0){throw "Uninstall failed: $($removed.ExitCode)"}
    }
}
if(Test-Path -LiteralPath (Join-Path $destination 'featherrec.exe')){throw 'Uninstall left the installed executable behind'}
