[CmdletBinding()]
param(
    [ValidateSet("nsis", "msi", "nsis,msi")]
    [string]$Bundles = "nsis,msi",

    [string]$Target = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ($env:OS -ne "Windows_NT") {
    throw "Windows installers must be built on Windows."
}

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Push-Location $projectRoot

try {
    foreach ($command in @("node", "npm", "rustc", "cargo")) {
        if (-not (Get-Command $command -ErrorAction SilentlyContinue)) {
            throw "Required command '$command' was not found in PATH."
        }
    }

    if (-not (Test-Path (Join-Path $projectRoot "node_modules"))) {
        & npm.cmd ci
        if ($LASTEXITCODE -ne 0) {
            throw "npm ci failed with exit code $LASTEXITCODE."
        }
    }

    $arguments = @("run", "tauri", "--", "build", "--bundles", $Bundles)
    if ($Target.Trim().Length -gt 0) {
        $arguments += @("--target", $Target)
    }

    & npm.cmd @arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Tauri Windows build failed with exit code $LASTEXITCODE."
    }

    $targetDirectory = if ($Target.Trim().Length -gt 0) {
        Join-Path $projectRoot "src-tauri\target\$Target\release\bundle"
    } else {
        Join-Path $projectRoot "src-tauri\target\release\bundle"
    }

    Write-Host "Windows installers are available under: $targetDirectory"
} finally {
    Pop-Location
}
