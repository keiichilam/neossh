# Explicit live acceptance: installs into LOCALAPPDATA and updates user PATH.
# Unlike install_smoke.ps1, this leaves the documented user installation in place.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$projectDir = Split-Path -Parent $PSScriptRoot
$qaDir = Join-Path $projectDir ('.qa\windows-install-' + [Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($qaDir) | Out-Null
$completed = $false
$failure = $null
$shellResult = $null
$destination = Join-Path $env:LOCALAPPDATA 'neo-ssh'
$installed = Join-Path $destination 'neo-ssh.exe'
$beforePath = [Environment]::GetEnvironmentVariable('Path', 'User')
try {
    & (Join-Path $projectDir 'install.ps1') -SkipBuild
    $afterPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $matches = @($afterPath -split ';' | Where-Object { $_.Trim().TrimEnd('\').Equals($destination.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase) })
    if ($matches.Count -ne 1) { throw 'Default destination is not present exactly once in user PATH.' }
    & (Join-Path $projectDir 'install.ps1') -SkipBuild
    if ([Environment]::GetEnvironmentVariable('Path', 'User') -ne $afterPath) { throw 'Reinstallation changed user PATH.' }
    $expectedHash = (Get-FileHash -LiteralPath (Join-Path $projectDir 'target\release\neo-ssh.exe')).Hash
    if ((Get-FileHash -LiteralPath $installed).Hash -ne $expectedHash) { throw 'Installed binary differs from release.' }

    # Existing parent processes retain stale environment blocks. Reconstruct the
    # persisted Windows PATH for this fresh shell; do not borrow the caller PATH.
    $childScript = @'
$ErrorActionPreference = 'Stop'
$command = Get-Command neo-ssh -CommandType Application -ErrorAction Stop
if (-not $command.Source.Equals($env:SSH4_INSTALL_EXPECTED, [StringComparison]::OrdinalIgnoreCase)) { throw 'PATH resolves a different executable.' }
$version = (& neo-ssh --version | Out-String).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Fresh-shell version command failed.' }
$helpText = (& neo-ssh --help | Out-String)
if ($LASTEXITCODE -ne 0 -or $helpText -notmatch '--gui' -or $helpText -notmatch '--tmux') { throw 'Fresh-shell help command failed.' }
@{ resolved = $command.Source; version = $version; help = $true; shell_continued = $true } | ConvertTo-Json -Compress
'@
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = Join-Path $PSHOME 'powershell.exe'
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($childScript))
    $start.Arguments = '-NoProfile -NonInteractive -EncodedCommand ' + $encoded
    $start.WorkingDirectory = $qaDir
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
    $start.EnvironmentVariables['Path'] = [Environment]::ExpandEnvironmentVariables("$machinePath;$afterPath")
    $start.EnvironmentVariables['SSH4_INSTALL_EXPECTED'] = $installed
    $process = [Diagnostics.Process]::Start($start)
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    try {
        if (-not $process.WaitForExit(30000)) {
            $process.Kill()
            $process.WaitForExit()
            throw 'Fresh shell did not finish within 30 seconds.'
        }
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        if ($process.ExitCode -ne 0) { throw "Fresh shell failed: $stderr" }
        $shellResult = $stdout | ConvertFrom-Json
    } finally { $process.Dispose() }
    $expectedVersion = (& $installed --version | Out-String).Trim()
    if ($shellResult.version -ne $expectedVersion) { throw 'Fresh-shell version differs from installed version.' }
    $completed = $true
    Write-Host "PASS default install/reinstall/user PATH/fresh-shell command and help: $expectedVersion"
} catch {
    $failure = $_.ToString()
    throw
} finally {
    [pscustomobject]@{
        completed = $completed
        failure = $failure
        installed = $installed
        user_path_changed = ([Environment]::GetEnvironmentVariable('Path', 'User') -ne $beforePath)
        shell = $shellResult
        limitation = 'Fresh PowerShell uses persisted machine/user PATH; existing terminal hosts may retain their old environment until restarted. Interactive console teardown is covered by the CLI fixtures.'
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $qaDir 'results.json') -Encoding UTF8
    Write-Host "Installation evidence: $qaDir"
}
