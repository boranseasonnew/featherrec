$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$output = Join-Path $repo 'target\portable'
if (Test-Path -LiteralPath $output) {
    $resolvedOutput = [IO.Path]::GetFullPath($output)
    $expectedOutput = [IO.Path]::GetFullPath((Join-Path $repo 'target\portable'))
    if ($resolvedOutput -ne $expectedOutput -or -not $resolvedOutput.StartsWith([IO.Path]::GetFullPath($repo) + [IO.Path]::DirectorySeparatorChar)) { throw 'Package directory escaped the repository' }
    Remove-Item -LiteralPath $resolvedOutput -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $output | Out-Null
foreach ($name in @('featherrec.exe', 'featherrec-cli.exe')) {
    Copy-Item -LiteralPath (Join-Path $repo "target\release\$name") -Destination $output
}
foreach ($name in @('README.md', 'LICENSE', 'SECURITY.md', 'CONTRIBUTING.md', 'THIRD_PARTY_NOTICES.md')) {
    Copy-Item -LiteralPath (Join-Path $repo $name) -Destination $output
}
foreach ($directory in @('assets', 'docs', 'benchmarks', 'tests', 'scripts')) {
    Copy-Item -LiteralPath (Join-Path $repo $directory) -Destination $output -Recurse -Force
}
Get-ChildItem -LiteralPath $output -Filter '*.exe' | Get-FileHash -Algorithm SHA256 | ForEach-Object {
    "$($_.Hash.ToLower())  $([IO.Path]::GetFileName($_.Path))"
} | Set-Content -LiteralPath (Join-Path $output 'SHA256SUMS.txt') -Encoding utf8
Write-Output "Development package: $output"
