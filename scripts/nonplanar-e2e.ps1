# SPDX-License-Identifier: AGPL-3.0-or-later
<#
.SYNOPSIS
  End-to-end matrix for the non-planar pipeline (COORD acceptance C).

.DESCRIPTION
  Builds a release shale-cli through the build-slot wrapper, then slices four
  parts on four Bambu printers in both `off` and `auto` mode into
  %TEMP%\shale-v02\e2e\, and writes summary.csv next to the G-code.

  Outputs (all outside the repository):
    %TEMP%\shale-v02\e2e\inputs\*.stl
    %TEMP%\shale-v02\e2e\<part>-<printer>-<mode>.gcode
    %TEMP%\shale-v02\e2e\summary.csv
    %TEMP%\shale-v02\e2e\logs\<part>-<printer>-<mode>.log

.PARAMETER SkipBuild
  Reuse the existing release binary (the run still records how long a build took).

.PARAMETER Parts
  Subset of the four parts to slice.

.PARAMETER Printers
  Subset of the four printers to slice.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File scripts\nonplanar-e2e.ps1
#>
[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [string[]]$Parts = @('t-wing', 'bridge-overhang', 'dome', 'pillar-beside-dome'),
    [string[]]$Printers = @('bambu_lab_a1', 'bambu_lab_a1_mini', 'bambu_lab_p1s', 'bambu_lab_x1_carbon'),
    [int]$MaxAngleDeg = 10,
    [int]$SliceTimeoutSec = 300,
    [string]$OutDir,
    [string]$Wrapper
)

$ErrorActionPreference = 'Stop'

# summary.csv is parsed by tools, not by people: fixed decimal points and UTF-8,
# whatever the Windows display language happens to be.
[Threading.Thread]::CurrentThread.CurrentCulture = [Globalization.CultureInfo]::InvariantCulture
Set-StrictMode -Version Latest

$Repo = Split-Path -Parent $PSScriptRoot
if (-not $OutDir) { $OutDir = Join-Path $env:TEMP 'shale-v02\e2e' }
if (-not $Wrapper) { $Wrapper = Join-Path $env:TEMP 'shale-v02\cargo-slot.ps1' }
$Inputs = Join-Path $OutDir 'inputs'
$Logs = Join-Path $OutDir 'logs'
$Csv = Join-Path $OutDir 'summary.csv'
New-Item -ItemType Directory -Force $Inputs, $Logs | Out-Null

# Isolated build target, like every other agent shell in this run.
if (-not $env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'shale-v02\target-e2e'
}

# Resource tree: the slice is only meaningful with the vendored Cura/Bambu data.
$env:SHALE_CURA_DIR = Join-Path $Repo 'vendor\Cura'
$env:SHALE_MATERIALS_DIR = Join-Path $Repo 'vendor\fdm_materials'
$env:SHALE_ORCA_DIR = Join-Path $Repo 'vendor\BambuStudio\resources\profiles'
$env:SHALE_CURAENGINE = Join-Path $Repo 'vendor\curaengine\CuraEngine.exe'
# The engine tests skip silently without all five variables (see COORD.md).
$env:SHALE_CURAENGINE_LIB_DIR = Join-Path $Repo 'vendor\curaengine'
$env:SHALE_BENCH_SUMMARY = '1'
foreach ($v in 'SHALE_CURA_DIR', 'SHALE_MATERIALS_DIR', 'SHALE_ORCA_DIR', 'SHALE_CURAENGINE', 'SHALE_CURAENGINE_LIB_DIR') {
    $path = [Environment]::GetEnvironmentVariable($v)
    if (-not (Test-Path $path)) { throw "$v does not exist: $path" }
}

function Invoke-Cargo {
    param([string[]]$CargoArgs)
    # Note: never let cargo's stdout escape this function — a PowerShell function
    # returns everything it writes, and callers rely on a single value back.
    # cargo paints "Compiling ..." on stderr, and under ErrorActionPreference=Stop
    # a native stderr line becomes a terminating NativeCommandError, so relax the
    # preference for the call and send every record straight to the host.
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        if (Test-Path $Wrapper) {
            & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Wrapper @CargoArgs 2>&1 | Out-Host
        } else {
            & cargo @CargoArgs 2>&1 | Out-Host
        }
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prev
    }
    if ($code -ne 0) { throw "cargo $($CargoArgs -join ' ') failed with $code" }
}

function Get-InputStl {
    param([string]$Name)
    $target = Join-Path $Inputs "$Name.stl"
    if (Test-Path $target) { return $target }
    switch ($Name) {
        'pillar-beside-dome' {
            # Written by tests/nonplanar_auto_twing.rs (SHALE_E2E_INPUT_DIR).
            $env:SHALE_E2E_INPUT_DIR = $Inputs
            Invoke-Cargo @('test', '--locked', '-q', '-p', 'shale-core', '--test', 'nonplanar_auto_twing', 'export_e2e_inputs')
            Remove-Item Env:\SHALE_E2E_INPUT_DIR
        }
        'dome' {
            # tests/export_bench_stls.rs writes dome.stl, chevron-slab.stl and
            # overhang-shelf.stl into examples/ — the last one is tracked. Take a
            # byte copy of whatever is there, run the export, copy the dome out,
            # then put examples/ back exactly as it was (generated STLs are not
            # committed; only bridge-overhang.stl is a source fixture).
            $generated = @('dome', 'chevron-slab', 'overhang-shelf')
            $before = @{}
            foreach ($g in $generated) {
                $p = Join-Path $Repo "examples\$g.stl"
                $before[$g] = $(if (Test-Path $p) { , [IO.File]::ReadAllBytes($p) } else { $null })
            }
            $env:SHALE_EXPORT_BENCH_STLS = '1'
            try {
                Invoke-Cargo @('test', '--locked', '-q', '-p', 'shale-core', '--test', 'export_bench_stls')
            } finally {
                Remove-Item Env:\SHALE_EXPORT_BENCH_STLS -ErrorAction SilentlyContinue
            }
            Copy-Item (Join-Path $Repo 'examples\dome.stl') $target -Force
            foreach ($g in $generated) {
                $p = Join-Path $Repo "examples\$g.stl"
                if ($null -ne $before[$g]) {
                    [IO.File]::WriteAllBytes($p, $before[$g])
                } elseif (Test-Path $p) {
                    Remove-Item $p -Force
                }
            }
        }
        default {
            $src = Join-Path $Repo "examples\$Name.stl"
            if (-not (Test-Path $src)) { throw "no such fixture: $src" }
            Copy-Item $src $target -Force
        }
    }
    if (-not (Test-Path $target)) { throw "could not produce $target" }
    return $target
}

$buildSeconds = 0
if (-not $SkipBuild) {
    Write-Host '[e2e] release build of shale-cli'
    $sw = [Diagnostics.Stopwatch]::StartNew()
    Invoke-Cargo @('build', '--release', '--locked', '-p', 'shale-cli')
    $sw.Stop()
    $buildSeconds = [math]::Round($sw.Elapsed.TotalSeconds, 1)
}
$Cli = Join-Path $env:CARGO_TARGET_DIR 'release\shale-cli.exe'
if (-not (Test-Path $Cli)) { throw "no release binary at $Cli (run without -SkipBuild)" }

$rows = New-Object System.Collections.Generic.List[object]
foreach ($part in $Parts) {
    $stl = Get-InputStl $part
    foreach ($printer in $Printers) {
        foreach ($mode in 'off', 'auto') {
            $tag = "$part-$printer-$mode"
            $gcode = Join-Path $OutDir "$tag.gcode"
            $log = Join-Path $Logs "$tag.log"
            $sw = [Diagnostics.Stopwatch]::StartNew()
            # The CLI writes its progress on stderr; PowerShell 5.1 turns native
            # stderr into a terminating error under ErrorActionPreference=Stop, so
            # drive the child through the process API: both pipes are drained
            # asynchronously (no buffer deadlock), WaitForExit(ms) gives a real
            # per-slice budget, and ExitCode is reliable once it returns true.
            $psi = New-Object System.Diagnostics.ProcessStartInfo
            $psi.FileName = $Cli
            $psi.Arguments = (@('slice', "`"$stl`"", '--printer', $printer, '--max-angle', "$MaxAngleDeg",
                    '--nonplanar', $mode, '-o', "`"$gcode`"") -join ' ')
            $psi.RedirectStandardOutput = $true
            $psi.RedirectStandardError = $true
            $psi.StandardOutputEncoding = [Text.Encoding]::UTF8
            $psi.StandardErrorEncoding = [Text.Encoding]::UTF8
            $psi.UseShellExecute = $false
            $proc = [System.Diagnostics.Process]::Start($psi)
            $tOut = $proc.StandardOutput.ReadToEndAsync()
            $tErr = $proc.StandardError.ReadToEndAsync()
            if ($proc.WaitForExit($SliceTimeoutSec * 1000)) {
                $exit = $proc.ExitCode
            } else {
                # Kill only the tree we started (shale-cli -> CuraEngine); never
                # anything else on the machine.
                & taskkill /T /F /PID $proc.Id 2>&1 | Out-Null
                $proc.WaitForExit(30000) | Out-Null
                $exit = 'timeout'
            }
            $sw.Stop()
            $output = [string]::Concat($tOut.Result, $tErr.Result)
            Set-Content -Path $log -Value $output -Encoding UTF8
            Write-Host ("[e2e] {0} exit={1} {2:N1}s" -f $tag, $exit, $sw.Elapsed.TotalSeconds)

            $summary = $null
            $npLine = ''
            $safetyLine = ''
            $fallback = ''
            foreach ($line in $output -split "\r?\n") {
                if ($line -match '^bench_summary (\{.*\})') {
                    $summary = $Matches[1] | ConvertFrom-Json
                } elseif ($line -match '^nonplanar (.*)') {
                    # The report's own line is the authority on what was written: it
                    # is the flag `planar_fallback` sets on the report, while the
                    # `auto surfaces=...` line below carries the planner's separate
                    # view of the plan it handed over.
                    $npLine = $line
                    if ($line -match 'fallback=(\w+)') {
                        $fallback = $Matches[1]
                    }
                } elseif ($line -match '^nonplanar_safety (.*)') {
                    $safetyLine = $line
                }
            }
            $rows.Add([pscustomobject]@{
                part              = $part
                printer           = $printer
                mode              = $mode
                exit              = $exit
                print_time_s      = $(if ($summary) { $summary.print_time_seconds } else { '' })
                layers            = $(if ($summary) { $summary.layers } else { '' })
                filament_m        = $(if ($summary) { '{0:F4}' -f [double]$summary.filament_m } else { '' })
                nonplanar         = $npLine
                nonplanar_safety  = $safetyLine
                fallback          = $fallback
                wall_s            = [math]::Round($sw.Elapsed.TotalSeconds, 1)
            })
            if ($exit -ne 0 -and -not (Test-Path $gcode)) {
                Write-Warning "[e2e] $tag produced no G-code (see $log)"
            }
        }
    }
}

$rows | Export-Csv -Path $Csv -NoTypeInformation -Encoding UTF8
Write-Host "[e2e] build ${buildSeconds}s; $($rows.Count) slices -> $Csv"
