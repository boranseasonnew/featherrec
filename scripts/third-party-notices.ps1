$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$metadata = (& "$PSScriptRoot\dev.ps1" -CargoArgs @('metadata','--locked','--offline','--format-version','1','--filter-platform','x86_64-pc-windows-msvc') | Out-String) | ConvertFrom-Json
$ids = [Collections.Generic.HashSet[string]]::new()
foreach ($node in $metadata.resolve.nodes) { [void]$ids.Add($node.id) }
$output = [Text.StringBuilder]::new()
[void]$output.AppendLine('# Third-party notices')
[void]$output.AppendLine('')
[void]$output.AppendLine('Generated from Cargo.lock for the Windows x64 dependency graph, including build dependencies. Not all listed code is linked into both executables. Font notices are included separately. FeatherRec itself uses the MIT license in LICENSE.')
foreach ($package in ($metadata.packages | Where-Object { $_.source -and $ids.Contains($_.id) } | Sort-Object name,version)) {
    [void]$output.AppendLine("`n## $($package.name) $($package.version)`n")
    [void]$output.AppendLine("Declared license: $($package.license)")
    [void]$output.AppendLine("`nSource: https://crates.io/crates/$($package.name)/$($package.version)")
    $directory = Split-Path $package.manifest_path -Parent
    $files = @(Get-ChildItem -LiteralPath $directory -File | Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|UNLICENSE)' })
    if ($package.name -eq 'epaint_default_fonts') {
        $files += Get-ChildItem -LiteralPath (Join-Path $directory 'fonts') -Filter '*.txt' -File
    }
    foreach ($file in $files) {
        [void]$output.AppendLine("`n### $($file.Name)`n")
        [void]$output.AppendLine('````text')
        [void]$output.AppendLine([IO.File]::ReadAllText($file.FullName))
        [void]$output.AppendLine('````')
    }
}
$text = [Text.RegularExpressions.Regex]::Replace($output.ToString(), '(?m)[\t ]+(?=\r?$)', '')
[IO.File]::WriteAllText((Join-Path $repo 'THIRD_PARTY_NOTICES.md'), $text)
Write-Output 'Generated THIRD_PARTY_NOTICES.md from the resolved Windows dependencies.'
