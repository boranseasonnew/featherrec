param([Parameter(ValueFromRemainingArguments = $true)][string[]]$CargoArgs)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Push-Location $repo
try {
    if (Test-Path "$repo\.tools\cargo\bin\cargo.exe") {
        $env:CARGO_HOME = "$repo\.tools\cargo"
        $env:RUSTUP_HOME = "$repo\.tools\rustup"
        $cargo = "$repo\.tools\cargo\bin\cargo.exe"
    } else { $cargo = (Get-Command cargo).Source }
    # A normal Developer PowerShell already provides LIB. For incomplete Visual
    # Studio installations, use the installed SDK libraries with Rust's linker.
    if (-not $env:LIB) {
        $msvc = Get-ChildItem "${env:ProgramFiles(x86)}\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*" -Directory -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | Select-Object -First 1
        $sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\Lib" -Directory -ErrorAction SilentlyContinue | Sort-Object Name -Descending | Select-Object -First 1
        if ($msvc -and $sdk) {
            $env:LIB = "$($msvc.FullName)\lib\x64;$($sdk.FullName)\um\x64;$($sdk.FullName)\ucrt\x64"
            $rustc = Join-Path (Split-Path $cargo) 'rustc.exe'
            $sysroot = & $rustc --print sysroot
            $linker = Join-Path $sysroot 'lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe'
            if (Test-Path $linker) { $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = $linker }
        }
    }
    & $cargo @CargoArgs
    exit $LASTEXITCODE
} finally { Pop-Location }
