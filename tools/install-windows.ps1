[CmdletBinding()]
param(
    [ValidatePattern("^[A-Za-z0-9_.-]+$")]
    [string]$EnvironmentName = "private-ai",

    [ValidatePattern("^3\.(12|13|14)$")]
    [string]$PythonVersion = "3.12",

    [switch]$SkipChecks,
    [switch]$SkipNativeTools
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

$conda = Get-Command conda -ErrorAction SilentlyContinue
if ($null -eq $conda) {
    $knownLocations = @(
        (Join-Path $env:USERPROFILE "miniconda3\Scripts\conda.exe"),
        (Join-Path $env:USERPROFILE "anaconda3\Scripts\conda.exe"),
        (Join-Path $env:ProgramData "miniconda3\Scripts\conda.exe"),
        (Join-Path $env:ProgramData "anaconda3\Scripts\conda.exe")
    )
    $fallback = $knownLocations | Where-Object { Test-Path $_ } | Select-Object -First 1
    if ($null -eq $fallback) {
        throw "Conda was not found. Open Anaconda Prompt or run 'conda init powershell', then retry."
    }
    $script:CondaCommand = $fallback
} else {
    $script:CondaCommand = $conda.Name
}

Push-Location $RepoRoot
try {
    Write-Step "Checking Conda environment '$EnvironmentName'"
    $environmentJson = & $script:CondaCommand env list --json
    if ($LASTEXITCODE -ne 0) {
        throw "Unable to read the Conda environment list."
    }
    $environmentInfo = ($environmentJson -join [Environment]::NewLine) | ConvertFrom-Json
    $environmentExists = @($environmentInfo.envs) | Where-Object {
        (Split-Path $_ -Leaf) -eq $EnvironmentName
    }

    $condaPackages = @("python=$PythonVersion", "nodejs=22", "pip")
    if (-not $SkipNativeTools) {
        $condaPackages += @("poppler", "tesseract")
    }

    if ($environmentExists) {
        Invoke-Conda `
            -Description "Updating runtime packages in '$EnvironmentName'" `
            -Arguments (@("install", "--yes", "--name", $EnvironmentName, "--channel", "conda-forge") + $condaPackages)
    } else {
        Invoke-Conda `
            -Description "Creating Conda environment '$EnvironmentName'" `
            -Arguments (@("create", "--yes", "--name", $EnvironmentName, "--channel", "conda-forge") + $condaPackages)
    }

    Invoke-Conda `
        -Description "Updating Python packaging tools" `
        -Arguments @(
            "run", "--no-capture-output", "--name", $EnvironmentName,
            "python", "-m", "pip", "install", "--upgrade", "pip", "build"
        )

    Invoke-Conda `
        -Description "Installing API and desktop packages" `
        -Arguments @(
            "run", "--no-capture-output", "--name", $EnvironmentName,
            "python", "-m", "pip", "install",
            "--editable", "services/api[dev]",
            "--editable", "apps/desktop[dev]"
        )

    Invoke-Conda `
        -Description "Installing locked frontend dependencies" `
        -Arguments @(
            "run", "--no-capture-output", "--name", $EnvironmentName,
            "npx", "--yes", "pnpm@10.17.1", "--dir", "apps/web",
            "install", "--frozen-lockfile"
        )

    if (-not $SkipChecks) {
        Invoke-Conda `
            -Description "Running Python tests" `
            -Arguments @(
                "run", "--no-capture-output", "--name", $EnvironmentName,
                "python", "-m", "pytest"
            )

        Invoke-Conda `
            -Description "Running Python lint checks" `
            -Arguments @(
                "run", "--no-capture-output", "--name", $EnvironmentName,
                "python", "-m", "ruff", "check", "."
            )

        Invoke-Conda `
            -Description "Running frontend type checks" `
            -Arguments @(
                "run", "--no-capture-output", "--name", $EnvironmentName,
                "npx", "--yes", "pnpm@10.17.1", "--dir", "apps/web", "typecheck"
            )
    }

    Invoke-Conda `
        -Description "Building frontend production assets" `
        -Arguments @(
            "run", "--no-capture-output", "--name", $EnvironmentName,
            "npx", "--yes", "pnpm@10.17.1", "--dir", "apps/web", "build"
        )

    New-Item -ItemType Directory -Force -Path $PythonDist | Out-Null
    foreach ($package in @("services/api", "apps/desktop")) {
        Invoke-Conda `
            -Description "Building Python wheel for $package" `
            -Arguments @(
                "run", "--no-capture-output", "--name", $EnvironmentName,
                "python", "-m", "build", "--wheel", "--outdir", $PythonDist, $package
            )
    }

    Write-Host ""
    Write-Host "Windows installation and build completed." -ForegroundColor Green
    Write-Host "Frontend: $RepoRoot\apps\web\dist"
    Write-Host "Python wheels: $PythonDist"
    Write-Host "Start development: conda run --no-capture-output -n $EnvironmentName python tools\dev.py"
    Write-Host "Start desktop: conda run --no-capture-output -n $EnvironmentName private-ai-desktop"
} finally {
    Pop-Location
}
