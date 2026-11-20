param([string]$Archive, [string]$Destination)
$ErrorActionPreference = 'Stop'
function Normal-Path([string]$Value) {
    # .NET Framework ZIP extraction does not accept Rust's canonical \\?\ prefix.
    if ($Value.StartsWith('\\?\UNC\')) { return '\\' + $Value.Substring(8) }
    if ($Value.StartsWith('\\?\')) { return $Value.Substring(4) }
    return $Value
}
$Archive = Normal-Path $Archive
$Destination = Normal-Path $Destination
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::OpenRead($Archive)
try {
    $root = [IO.Path]::GetFullPath($Destination).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    foreach ($entry in $zip.Entries) {
        $target = [IO.Path]::GetFullPath([IO.Path]::Combine($root, $entry.FullName))
        if (!$target.StartsWith($root, [StringComparison]::OrdinalIgnoreCase) -or (($entry.ExternalAttributes -shr 16) -band 0xf000) -eq 0xa000) { throw 'Unsafe ZIP entry' }
    }
} finally { $zip.Dispose() }
[IO.Compression.ZipFile]::ExtractToDirectory($Archive, $Destination)
