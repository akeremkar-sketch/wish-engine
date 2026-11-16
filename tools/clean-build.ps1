# Frees the disk space old builds of the engine's own crates take.
#
# Cargo keeps every build variant it ever made (each set of features and
# flags gets its own folder) and never removes old ones; the engine's crates
# link wasmtime into every test binary, so in a few days of work this grew
# past 100 GB (16 November 2026). This removes the outputs of the workspace
# crates and the incremental caches only: third-party dependencies stay, so
# the next build takes minutes, not an hour. Run it at the end of every work
# package, or when less than 30 GB is free.
$ErrorActionPreference = 'Stop'
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
$before = (Get-PSDrive C).Free
$target = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).target_directory
$members = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).packages.name
foreach ($member in $members) {
    cargo clean -q -p $member 2>$null
}
foreach ($profile in 'debug', 'release') {
    $cache = Join-Path $target "$profile\incremental"
    if (Test-Path $cache) { Remove-Item -Recurse -Force $cache }
}
$after = (Get-PSDrive C).Free
'Freed {0:N1} GB; {1:N1} GB free.' -f (($after - $before) / 1GB), ($after / 1GB)
