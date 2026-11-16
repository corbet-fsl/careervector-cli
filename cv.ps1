param(
    [Parameter(Position = 0)]
    [string]$Command = "help",
    [Parameter(Position = 1, ValueFromRemainingArguments = $true)]
    [string[]]$Arguments = @()
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$RepoRoot = $PSScriptRoot
$Binary = Join-Path $RepoRoot ".agent\cache\cv\bin\cv.exe"

switch ($Command.ToLowerInvariant()) {
    "setup" {
        & (Join-Path $RepoRoot ".agent\scripts\bootstrap.ps1") install @Arguments
        exit 0
    }
    "bootstrap" {
        & (Join-Path $RepoRoot ".agent\scripts\bootstrap.ps1") plan @Arguments
        exit 0
    }
    default {
        if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) {
            throw "cv is not set up. Run .\cv.cmd bootstrap, then .\cv.cmd setup."
        }
        . (Join-Path $RepoRoot ".agent\scripts\runtime-id.ps1")
        $RuntimeId = (& $Binary runtime-id 2>$null | Out-String).Trim()
        if ($LASTEXITCODE -ne 0 -or $RuntimeId -ne (Get-SourceFingerprint)) {
            throw "The installed cv runtime is stale. Run .\cv.cmd setup to install the matching binary."
        }
        Push-Location $RepoRoot
        try {
            & $Binary $Command @Arguments
            exit $LASTEXITCODE
        }
        finally {
            Pop-Location
        }
    }
}
