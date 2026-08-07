[CmdletBinding()]
param(
    # Tauri's Windows bundle selector; validation prevents arbitrary flags
    # from being smuggled into the build command through this wrapper.
    [ValidateSet("nsis", "msi", "nsis,msi")]
    [string]$Bundles = "nsis,msi",

    [string]$Target = ""
)

# Stop on the first failed command and reject uninitialized PowerShell state.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# The script is intentionally Windows-only because it invokes Windows bundle
# targets and reports paths using the Windows target layout.
if ($env:OS -ne "Windows_NT") {
    throw "Windows installers must be built on Windows."
}

# Resolve from the script location so callers may run it from any working
# directory; all subsequent paths remain anchored at the repository root.
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Push-Location $projectRoot

try {
    # Check the toolchain before npm or Tauri work starts, producing a direct
    # missing-command error instead of a later opaque build failure.
    foreach ($command in @("node", "npm", "rustc", "cargo")) {
        if (-not (Get-Command $command -ErrorAction SilentlyContinue)) {
            throw "Required command '$command' was not found in PATH."
        }
    }

    # CI and clean checkouts may not have dependencies yet. `npm ci` is only
    # invoked when node_modules is absent, so an existing install is preserved.
    if (-not (Test-Path (Join-Path $projectRoot "node_modules"))) {
        & npm.cmd ci
        if ($LASTEXITCODE -ne 0) {
            throw "npm ci failed with exit code $LASTEXITCODE."
        }
    }

    # Build arguments are assembled as an array to preserve argument boundaries
    # and to make the optional target flag independent from user quoting.
    $arguments = @("run", "tauri", "--", "build", "--bundles", $Bundles)
    if ($Target.Trim().Length -gt 0) {
        $arguments += @("--target", $Target)
    }

    # The exit code is checked explicitly because PowerShell native command
    # failures do not always behave like terminating PowerShell exceptions.
    & npm.cmd @arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Tauri Windows build failed with exit code $LASTEXITCODE."
    }

    # Tauri places targeted and default bundles in different directories; this
    # output is informational and does not alter or copy the generated files.
    $targetDirectory = if ($Target.Trim().Length -gt 0) {
        Join-Path $projectRoot "src-tauri\target\$Target\release\bundle"
    } else {
        Join-Path $projectRoot "src-tauri\target\release\bundle"
    }

    Write-Host "Windows installers are available under: $targetDirectory"
} finally {
    # Restore the caller's original directory even when dependency or bundle
    # creation fails, keeping this wrapper side-effect bounded.
    Pop-Location
}
