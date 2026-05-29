# contextd Windows Install Script
# Usage: powershell -ExecutionPolicy Bypass -File install.ps1

param(
    [string]$Version = "latest",
    [string]$InstallDir = "$env:USERPROFILE\.contextd\bin"
)

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
    if ($arch -eq "ARM64") { return "aarch64" }
    Write-Error "Unsupported architecture: $arch"
    exit 1
}

function Install-Contextd {
    $ver = Get-ContextdVersion
    $arch = Get-Arch
    $filename = "contextd-windows-$arch.exe"
    $url = "https://github.com/$Repo/releases/download/v$ver/$filename"

    Write-Host "Installing contextd v$ver ($arch)..."

    # Create install directory
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

    # Download binary
    $out = "$InstallDir\contextd.exe"
    Write-Host "Downloading $url ..."
    Invoke-WebRequest -Uri $url -OutFile $out

    # Add to PATH
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($currentPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable("Path", "$currentPath;$InstallDir", "User")
        Write-Host "Added $InstallDir to PATH"
    }

    Write-Host "contextd installed successfully!"
    Write-Host "Run 'contextd --help' to get started."
}

Install-Contextd
