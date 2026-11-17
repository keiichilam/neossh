param([Parameter(Mandatory = $true)][string]$ZigDirectory, [ValidateSet('aarch64-apple-darwin', 'x86_64-apple-darwin')][string]$Target = 'aarch64-apple-darwin', [switch]$Clippy)
# Cross-check all macOS targets. Linking/runtime require a macOS SDK/host and remain separate.
$ErrorActionPreference = 'Stop'
$zigTarget = if ($Target -eq 'aarch64-apple-darwin') { 'aarch64-macos' } else { 'x86_64-macos' }
$targetVariable = $Target.Replace('-', '_')
$projectDir = Split-Path -Parent $PSScriptRoot
$zig = (Resolve-Path -LiteralPath (Join-Path $ZigDirectory 'zig.exe')).Path
$qaDir = Join-Path $projectDir ('.qa\macos-cross-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $qaDir | Out-Null
$compiler = Join-Path $qaDir 'zig-cc.cmd'
$archiver = Join-Path $qaDir 'zig-ar.cmd'
('@"{0}" cc %* -target {1}' -f $zig, $zigTarget) | Set-Content -LiteralPath $compiler -Encoding ASCII
('@"{0}" ar %*' -f $zig) | Set-Content -LiteralPath $archiver -Encoding ASCII
$settings = @{
    ('CC_' + $targetVariable) = $compiler
    ('AR_' + $targetVariable) = $archiver
    ('CARGO_TARGET_' + $targetVariable.ToUpperInvariant() + '_LINKER') = $compiler
    ZIG_GLOBAL_CACHE_DIR = (Join-Path $projectDir '.qa\zig-global-cache')
    ZIG_LOCAL_CACHE_DIR = (Join-Path $projectDir '.qa\zig-local-cache')
}
$previous = @{}
$exitCode = $null
$failure = $null
$log = Join-Path $qaDir 'check.log'
$cargoArguments = @('check', '--locked', '--all-targets', '--target', $Target)
if ($Clippy) { $cargoArguments = @('clippy', '--locked', '--all-targets', '--target', $Target, '--', '-D', 'warnings') }
Push-Location $projectDir
try {
    foreach ($name in $settings.Keys) {
        $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        [Environment]::SetEnvironmentVariable($name, $settings[$name], 'Process')
    }
    $zigVersion = & $zig version
    if ($LASTEXITCODE -ne 0) { throw 'Cannot run Zig' }
    $ErrorActionPreference = 'Continue'
    & cargo @cargoArguments 2>&1 |
        ForEach-Object { $_.ToString() } | Tee-Object -FilePath $log | Out-Host
    $exitCode = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($exitCode -ne 0) { throw "macOS check failed: $exitCode" }
} catch {
    $failure = $_.ToString()
    throw
} finally {
    foreach ($name in $previous.Keys) {
        [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process')
    }
    Pop-Location
    [pscustomobject]@{
        completed = ($exitCode -eq 0 -and $null -eq $failure)
        exit_code = $exitCode
        failure = $failure
        zig_version = $zigVersion
        target = $Target
        scope = ('cargo ' + ($cargoArguments -join ' '))
        log = $log
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $qaDir 'results.json') -Encoding UTF8
    Write-Host "macOS check evidence: $qaDir"
}
