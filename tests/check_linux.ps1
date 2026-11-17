param([Parameter(Mandatory = $true)][string]$ZigDirectory, [switch]$Build, [switch]$BuildTests, [switch]$Release)
# Check all Linux targets, or link the app and clipboard QA helper with -Build. Runtime acceptance is separate.
$ErrorActionPreference = 'Stop'
if ($Build -and $BuildTests) { throw 'Choose either Build or BuildTests' }
$projectDir = Split-Path -Parent $PSScriptRoot
$zig = (Resolve-Path -LiteralPath (Join-Path $ZigDirectory 'zig.exe')).Path
$qaDir = Join-Path $projectDir ('.qa\linux-cross-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $qaDir | Out-Null
$compiler = Join-Path $qaDir 'zig-cc.cmd'
$archiver = Join-Path $qaDir 'zig-ar.cmd'
('@"{0}" cc %* -target x86_64-linux-gnu' -f $zig) | Set-Content -LiteralPath $compiler -Encoding ASCII
('@"{0}" ar %*' -f $zig) | Set-Content -LiteralPath $archiver -Encoding ASCII
$settings = @{
    CC_x86_64_unknown_linux_gnu = $compiler
    AR_x86_64_unknown_linux_gnu = $archiver
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER = $compiler
    ZIG_GLOBAL_CACHE_DIR = (Join-Path $projectDir '.qa\zig-global-cache')
    ZIG_LOCAL_CACHE_DIR = (Join-Path $projectDir '.qa\zig-local-cache')
}
$previous = @{}
$exitCode = $null
$failure = $null
$log = Join-Path $qaDir 'check.log'
$cargoArguments = if ($BuildTests) { @('test', '--locked', '--no-run', '--lib', '--bin', 'neo-ssh', '--target', 'x86_64-unknown-linux-gnu') }
                  elseif ($Build) { @('build', '--locked', '--bin', 'neo-ssh', '--example', 'clipboard_fixture', '--target', 'x86_64-unknown-linux-gnu') }
                  else { @('check', '--locked', '--all-targets', '--target', 'x86_64-unknown-linux-gnu') }
if ($Release) { $cargoArguments += '--release' }
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
    if ($exitCode -ne 0) { throw "Linux check failed: $exitCode" }
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
        target = 'x86_64-unknown-linux-gnu'
        scope = ('cargo ' + ($cargoArguments -join ' '))
        log = $log
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $qaDir 'results.json') -Encoding UTF8
    Write-Host "Linux check evidence: $qaDir"
}
