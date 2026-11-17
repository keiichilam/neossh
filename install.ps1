# Install NEO-SSH (built from the consolidated ssh4 / ssh4gui / ssh5 sources).
[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'neo-ssh'),
    [switch]$NoPath,
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$projectDir = $PSScriptRoot
$targetDir = Join-Path $projectDir 'target'
if (-not $SkipBuild) {
    & cargo build --release --locked --manifest-path (Join-Path $projectDir 'Cargo.toml') --target-dir $targetDir
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed; installed executable was not changed.' }
}
$sourceExe = Join-Path $targetDir 'release\neo-ssh.exe'
if (-not (Test-Path -LiteralPath $sourceExe -PathType Leaf)) { throw "Release executable is missing: $sourceExe" }
$version = & $sourceExe --version
if ($LASTEXITCODE -ne 0 -or $version -notmatch '^neo-ssh \d+\.\d+\.\d+') { throw 'Release executable failed its version check.' }
$manifest = Get-Content -LiteralPath (Join-Path $projectDir 'Cargo.toml') -Raw
if ($manifest -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'Cannot read the application version from Cargo.toml.' }
if ($version -ne "neo-ssh $($Matches[1])") { throw 'Release executable is stale. Re-run installation without -SkipBuild.' }
$destinationDir = [IO.Path]::GetFullPath($InstallDir)
[IO.Directory]::CreateDirectory($destinationDir) | Out-Null
$destinationExe = Join-Path $destinationDir 'neo-ssh.exe'
if ($sourceExe.Equals($destinationExe, [StringComparison]::OrdinalIgnoreCase)) { throw 'Choose an install directory different from the build output.' }
$stagedExe = Join-Path $destinationDir ('.ssh4-install-' + [Guid]::NewGuid().ToString('N') + '.exe')
try {
    Copy-Item -LiteralPath $sourceExe -Destination $stagedExe
    if ((Get-FileHash -LiteralPath $sourceExe -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $stagedExe -Algorithm SHA256).Hash) { throw 'Staged executable checksum does not match the build.' }
    $stagedVersion = & $stagedExe --version
    if ($LASTEXITCODE -ne 0 -or $stagedVersion -ne $version) { throw 'Staged executable failed its version check.' }
    if ([IO.File]::Exists($destinationExe)) {
        [IO.File]::Replace($stagedExe, $destinationExe, [NullString]::Value)
    } else {
        [IO.File]::Move($stagedExe, $destinationExe)
    }
} finally {
    if (Test-Path -LiteralPath $stagedExe) { Remove-Item -LiteralPath $stagedExe -Force }
}
if (-not $NoPath) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not $userPath) { $userPath = '' }
    $normalized = $destinationDir.TrimEnd('\')
    $found = @($userPath -split ';' | Where-Object { $_.Trim().TrimEnd('\').Equals($normalized, [StringComparison]::OrdinalIgnoreCase) })
    if ($found.Count -eq 0) {
        $separator = if ($userPath.Length -gt 0 -and -not $userPath.EndsWith(';')) { ';' } else { '' }
        [Environment]::SetEnvironmentVariable('Path', "$userPath$separator$destinationDir", 'User')
        Write-Host 'Added install directory to user PATH. Open a new terminal to use it.'
    }
}
Write-Host "Installed $version at $destinationExe"
Write-Host 'Run neo-ssh for the GUI, or neo-ssh user@host for the CLI.'
