<#
.SYNOPSIS
    Create or update the Conda environment for Private AI on Windows, then build it.

.DESCRIPTION
    One Python package, no Node.js. The SolidJS front end and the FastAPI service are
    gone: the application is a single PySide6 process plus an ingestion worker, so the
    whole install is `pip install --editable .[dev]`.

    On the Python version: the old script pinned 3.12/3.13 because RAG-Anything pulled in
    MinerU, which had no 3.14 wheels. RAG-Anything is no longer a dependency of this
    project — the current set is LangChain/LangGraph, PySide6, qasync, mcp, lightrag-hku,
    markitdown, pypdf and numpy — so that reason is gone and 3.14 is allowed. The whole
    set was verified installed and running under CPython 3.14 on macOS/arm64; the
    equivalent cp314 wheels have NOT been verified on Windows in this pass, so 3.13
    remains selectable with -PythonVersion if a wheel turns out to be missing.
#>
[CmdletBinding()]
param(
    [ValidatePattern("^[A-Za-z0-9_.-]+$")]
    [string]$EnvironmentName = "private-ai",

    [ValidatePattern("^3\.(12|13|14)$")]
    [string]$PythonVersion = "3.14",

    [switch]$SkipChecks
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$PythonDist = Join-Path $RepoRoot "dist\python"

function Write-Step {
    param([Parameter(Mandatory = $true)][string]$Message)

    Write-Host ""
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Invoke-Conda {
    param(
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$Description
    )

    Write-Step $Description
    & $script:CondaCommand @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code $LASTEXITCODE`: conda $($Arguments -join ' ')"
    }
}

function Invoke-InEnvironment {
    param(
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$Description
    )

    Invoke-Conda -Description $Description -Arguments (
        @("run", "--no-capture-output", "--name", $EnvironmentName) + $Arguments
    )
}

$conda = Get-Command conda.exe -CommandType Application -ErrorAction SilentlyContinue

if ($null -eq $conda) {
    $knownLocations = @(
        (Join-Path $env:USERPROFILE "miniconda3\Scripts\conda.exe"),
        (Join-Path $env:USERPROFILE "anaconda3\Scripts\conda.exe"),
        (Join-Path $env:ProgramData "miniconda3\Scripts\conda.exe"),
        (Join-Path $env:ProgramData "anaconda3\Scripts\conda.exe")
    )

    $fallback = $knownLocations |
        Where-Object { Test-Path $_ } |
        Select-Object -First 1

    if ($null -eq $fallback) {
        throw "Conda was not found. Open Anaconda Prompt or run 'conda init powershell', then retry."
    }

    $script:CondaCommand = $fallback
}
else {
    $script:CondaCommand = $conda.Source
}

Write-Host "Using Conda executable: $script:CondaCommand"


Push-Location $RepoRoot
try {
    Write-Step "Checking Conda environment '$EnvironmentName'"
    $environmentJson = & $script:CondaCommand @("env", "list", "--json")
    if ($LASTEXITCODE -ne 0) {
        throw "Unable to read the Conda environment list."
    }

    $environmentInfo = ($environmentJson -join [Environment]::NewLine) | ConvertFrom-Json
    $environmentExists = @($environmentInfo.envs) | Where-Object {
        (Split-Path $_ -Leaf) -eq $EnvironmentName
    }

    # Python and pip only. Nothing in this project needs Node.js any more.
    $condaPackages = @("python=$PythonVersion", "pip")

    if ($environmentExists) {
        Invoke-Conda `
            -Description "Updating runtime packages in '$EnvironmentName'" `
            -Arguments (@("install", "--yes", "--name", $EnvironmentName, "--channel", "conda-forge") + $condaPackages)
    } else {
        Invoke-Conda `
            -Description "Creating Conda environment '$EnvironmentName'" `
            -Arguments (@("create", "--yes", "--name", $EnvironmentName, "--channel", "conda-forge") + $condaPackages)
    }

    Invoke-InEnvironment `
        -Description "Updating Python packaging tools" `
        -Arguments @("python", "-m", "pip", "install", "--upgrade", "pip", "build")

    Invoke-InEnvironment `
        -Description "Installing Private AI" `
        -Arguments @("python", "-m", "pip", "install", "--editable", ".[dev]")

    if (-not $SkipChecks) {
        Invoke-InEnvironment `
            -Description "Running tests" `
            -Arguments @("python", "-m", "pytest")

        Invoke-InEnvironment `
            -Description "Running lint checks" `
            -Arguments @("python", "-m", "ruff", "check", "src", "tests", "tools")
    }

    New-Item -ItemType Directory -Force -Path $PythonDist | Out-Null
    Invoke-InEnvironment `
        -Description "Building the Python wheel" `
        -Arguments @("python", "-m", "build", "--wheel", "--outdir", $PythonDist, ".")

    Write-Host ""
    Write-Host "Windows installation and build completed." -ForegroundColor Green
    Write-Host "Python wheel: $PythonDist"
    Write-Host "Start the app:   conda run --no-capture-output -n $EnvironmentName private-ai"
    Write-Host "Start dev mode:  conda run --no-capture-output -n $EnvironmentName python tools\dev.py"
    Write-Host "One MCP server:  conda run --no-capture-output -n $EnvironmentName python tools\dev.py --mcp vector"
} finally {
    Pop-Location
}
