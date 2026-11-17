# Reproduce NEO-SSH release checks; no user installation or PATH changes.
[CmdletBinding()]
param([switch]$SkipSsh, [switch]$SkipInstaller)
$ErrorActionPreference = 'Stop'
$projectDir = Split-Path -Parent $PSScriptRoot
$qaDir = Join-Path $projectDir ('.qa\validation-' + [Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($qaDir) | Out-Null
$results = [Collections.Generic.List[object]]::new()
$completed = $false
$failure = $null
function Invoke-Check([string]$Name, [string]$Program, [string[]]$Arguments) {
    $log = Join-Path $qaDir ($Name + '.log')
    $started = [DateTime]::UtcNow
    $command = Get-Command -Name $Program -CommandType Application -ErrorAction Stop | Select-Object -First 1
    $savedPreference = $ErrorActionPreference
    try {
        # Windows PowerShell treats ordinary native stderr (Cargo progress) as
        # error records. The process exit code determines check success.
        $ErrorActionPreference = 'Continue'
        & $command.Source @Arguments 2>&1 | ForEach-Object { $_.ToString() } | Tee-Object -FilePath $log -ErrorAction Stop | Out-Host
        $exitCode = $LASTEXITCODE
    } finally { $ErrorActionPreference = $savedPreference }
    $results.Add([pscustomobject]@{ name = $Name; exit_code = $exitCode; started_utc = $started.ToString('o'); seconds = ([DateTime]::UtcNow - $started).TotalSeconds; log = $log })
    if ($exitCode -ne 0) { throw "$Name failed (exit $exitCode). See $log" }
}
Push-Location $projectDir
try {
    Invoke-Check 'format' 'cargo' @('fmt', '--check')
    Invoke-Check 'check' 'cargo' @('check', '--locked')
    Invoke-Check 'clippy' 'cargo' @('clippy', '--locked', '--all-targets', '--', '-D', 'warnings')
    Invoke-Check 'tests' 'cargo' @('test', '--locked', '--all-targets')
    Invoke-Check 'gui-backend-tests' 'cargo' @('test', '--locked', '-p', 'egui-winit', '--lib')
    if (-not $SkipSsh) {
        # ssh_fixture.py invokes the application for real key generation.
        # cargo test builds test harnesses, not target/debug/neo-ssh.exe.
        Invoke-Check 'fixture-app-build' 'cargo' @('build', '--locked', '--bin', 'neo-ssh')
        Invoke-Check 'config-probe-build' 'cargo' @('build', '--locked', '--example', 'config_probe')
        Invoke-Check 'config-parity' 'python' @('tests/ssh_config_fixture.py')
        Invoke-Check 'probe-build' 'cargo' @('build', '--locked', '--example', 'session_probe')
        Invoke-Check 'ssh-fixture' 'python' @('tests/ssh_fixture.py')
    }
    Invoke-Check 'release' 'cargo' @('build', '--release', '--locked')
    if (-not $SkipInstaller) {
        Invoke-Check 'installer' 'powershell.exe' @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $PSScriptRoot 'install_smoke.ps1'))
    }
    $completed = $true
    Write-Host 'All requested automated checks passed. Native/manual release acceptance remains separate.'
} catch {
    $failure = $_.ToString()
    throw
} finally {
    $summary = [pscustomobject]@{ completed = $completed; failure = $failure; skipped_ssh = [bool]$SkipSsh; skipped_installer = [bool]$SkipInstaller; checks = @($results.ToArray()) }
    $summary | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $qaDir 'results.json') -Encoding UTF8
    Pop-Location
    Write-Host "Validation evidence: $qaDir"
}
