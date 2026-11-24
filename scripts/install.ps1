# contextd Windows Install Script
# Usage: powershell -ExecutionPolicy Bypass -File install.ps1

param(
    [string]$Version = "latest",
    [string]$InstallDir = "$env:USERPROFILE\.contextd\bin"
)

$ErrorActionPreference = "Stop"
$Repo = "sandy-sachin7/contextd"

function Get-ContextdVersion {
    if ($Version -eq "latest") {
        $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
        return $releases.tag_name.TrimStart('v')
    }
    return $Version.TrimStart('v')
}

function Get-Arch {
    $arch = $env:PROCESSOR_ARCHITECTURE
    if ($arch -eq "AMD64") { return "x86_64" }
    Write-Error "Unsupported architecture: $arch (pre-built Windows binaries are currently x86_64 only; build from source with cargo install)"
    exit 1
}

function Install-Contextd {
    $ver = Get-ContextdVersion
    $arch = Get-Arch
    $filename = "contextd-windows-$arch.exe"
    $baseUrl = "https://github.com/$Repo/releases/download/v$ver"
    $url = "$baseUrl/$filename"
    $sumsUrl = "$baseUrl/SHA256SUMS.txt"

    Write-Host "Installing contextd v$ver ($arch)..."

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

    $tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("contextd-install-" + [System.Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null

    try {
        $tmpBin = Join-Path $tmpDir $filename
        $tmpSums = Join-Path $tmpDir "SHA256SUMS.txt"

        Write-Host "Downloading $url ..."
        Invoke-WebRequest -Uri $url -OutFile $tmpBin -UseBasicParsing
        Invoke-WebRequest -Uri $sumsUrl -OutFile $tmpSums -UseBasicParsing

        $expectedLine = Get-Content $tmpSums | Where-Object { $_ -match ("\s+\*?" + [regex]::Escape($filename) + "$") } | Select-Object -First 1
        if (-not $expectedLine) {
            throw "Checksum entry for $filename not found in SHA256SUMS.txt. Aborting."
        }
        $expectedHash = ($expectedLine -split '\s+')[0].ToLowerInvariant()
        $actualHash = (Get-FileHash -Path $tmpBin -Algorithm SHA256).Hash.ToLowerInvariant()

        if ($actualHash -ne $expectedHash) {
            throw "SHA-256 verification failed! Expected $expectedHash, got $actualHash"
        }
        Write-Host "SHA-256 checksum verified."

        $out = Join-Path $InstallDir "contextd.exe"
        Move-Item -Path $tmpBin -Destination $out -Force
    }
    finally {
        Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
    }

    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($currentPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable("Path", "$currentPath;$InstallDir", "User")
        Write-Host "Added $InstallDir to PATH"
    }

    Write-Host "contextd v$ver installed successfully!"
    Write-Host "Run 'contextd --help' to get started."
}

Install-Contextd
