$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$compiler = Get-Command ISCC.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1
if(-not $compiler){$compiler = Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'}
if(-not (Test-Path -LiteralPath $compiler)){throw 'Install Inno Setup 6 to build FeatherRec-Setup.exe'}
& $compiler (Join-Path $repo 'installer/featherrec.iss')
if($LASTEXITCODE -ne 0){throw 'Installer compilation failed'}
