# Exercise installation and failed replacement without modifying user PATH.
$ErrorActionPreference = 'Stop'
$projectDir = Split-Path -Parent $PSScriptRoot
$qaDir = Join-Path $projectDir ('.qa\install [literal] ' + [Guid]::NewGuid().ToString('N'))
$beforePath = [Environment]::GetEnvironmentVariable('Path', 'User')
& (Join-Path $projectDir 'install.ps1') -SkipBuild -NoPath -InstallDir $qaDir
$installed = Join-Path $qaDir 'neo-ssh.exe'
$hash = (Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash
$version = & $installed --version
if ($LASTEXITCODE -ne 0) { throw 'Installed version check failed.' }
& (Join-Path $projectDir 'install.ps1') -SkipBuild -NoPath -InstallDir $qaDir
if ((Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash -ne $hash) { throw 'Reinstallation changed the binary unexpectedly.' }
$lock = [IO.File]::Open($installed, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
$failed = $false
try {
    try { & (Join-Path $projectDir 'install.ps1') -SkipBuild -NoPath -InstallDir $qaDir }
    catch { $failed = $true }
} finally { $lock.Dispose() }
if (-not $failed) { throw 'Replacing a locked executable unexpectedly succeeded.' }
if ((Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash -ne $hash) { throw 'Failed installation damaged the installed executable.' }
if (@(Get-ChildItem -LiteralPath $qaDir -Force).Count -ne 1) { throw 'Installation left temporary files behind.' }
if ([Environment]::GetEnvironmentVariable('Path', 'User') -ne $beforePath) { throw 'NoPath installation changed user PATH.' }
& $installed --help | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Installed help check failed.' }
Write-Host "PASS install/reinstall/locked-target preservation/staging cleanup/NoPath: $version"
Write-Host "QA executable retained at $installed"
