[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$OutputPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$manifest = Join-Path $PSScriptRoot '../Cargo.toml'
$target = 'x86_64-pc-windows-msvc'
$metadata = & cargo metadata --manifest-path $manifest --format-version 1 --locked --offline --filter-platform $target
if ($LASTEXITCODE -ne 0) {
    throw 'Cargo could not read locked Windows dependencies. Build the application before packaging.'
}
$metadata = $metadata | ConvertFrom-Json

# Walk the filtered build and runtime graph. Test-only dependencies are not distributed.
$nodes = @{}
foreach ($node in $metadata.resolve.nodes) { $nodes[$node.id] = $node }
$pending = [Collections.Generic.Stack[string]]::new()
$used = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$pending.Push($metadata.resolve.root)
while ($pending.Count -gt 0) {
    $id = $pending.Pop()
    if (!$used.Add($id)) { continue }
    foreach ($dependency in $nodes[$id].deps) {
        if ($dependency.dep_kinds | Where-Object kind -ne 'dev') {
            $pending.Push($dependency.pkg)
        }
    }
}

$packages = $metadata.packages |
    Where-Object { $_.source -and $used.Contains($_.id) } |
    Sort-Object name, version

$writer = [IO.StreamWriter]::new([IO.Path]::GetFullPath($OutputPath), $false)
try {
    $writer.WriteLine('fishing Rust dependency notices')
    $writer.WriteLine("Resolved from Cargo.lock for $target.")
    foreach ($package in $packages) {
        $writer.WriteLine()
        $writer.WriteLine("===== $($package.name) $($package.version) =====")
        if ($package.license) { $writer.WriteLine("License: $($package.license)") }
        if ($package.repository) { $writer.WriteLine("Repository: $($package.repository)") }
        if ($package.authors.Count -gt 0) { $writer.WriteLine("Authors: $($package.authors -join ', ')") }

        $directory = Split-Path -Parent $package.manifest_path
        $files = @(Get-ChildItem -LiteralPath $directory -File |
            Where-Object Name -Match '^(LICENSE|COPYING|NOTICE|COPYRIGHT)([._-]|$)')
        if ($package.license_file) {
            $path = [IO.Path]::GetFullPath($package.license_file, $directory)
            $prefix = $directory + [IO.Path]::DirectorySeparatorChar
            if (!$path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
                throw "License file for $($package.name) is outside its crate directory."
            }
            $files += Get-Item -LiteralPath $path
        }
        if ($package.name -eq 'epaint_default_fonts') {
            $files += Get-ChildItem -LiteralPath (Join-Path $directory 'fonts') -File -Filter '*.txt'
        }
        foreach ($file in ($files | Sort-Object FullName -Unique)) {
            $name = $file.FullName.Substring($directory.Length + 1)
            $writer.WriteLine()
            $writer.WriteLine("--- $name ---")
            $writer.WriteLine([IO.File]::ReadAllText($file.FullName).TrimEnd())
        }
    }
} finally {
    $writer.Dispose()
}
