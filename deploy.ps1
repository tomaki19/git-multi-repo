#
# SPDX-FileCopyrightText: 2026 Thomas Killer
# SPDX-License-Identifier: MIT
#
# Packages a release of git-multi-repo for the host platform (Windows):
#   dist/git-multi-repo-<version>-<target>.zip  (binary, README.md, LICENSE)
#   dist/git-multi-repo-<version>-<target>.zip.sha256
#   dist/git-multi-repo-<version>-<target>.msi (+ .sha256), if WiX (`wix`) is installed
#
# The working tree must be clean so every archive corresponds to a commit,
# and formatting, lints and tests must pass before anything is built.
#
# Usage: powershell -ExecutionPolicy Bypass -File deploy.ps1   (or: pwsh deploy.ps1)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$name = 'git-multi-repo'
$root = $PSScriptRoot
Set-Location $root

function Fail([string]$message) {
    [Console]::Error.WriteLine("error: $message")
    exit 1
}

# Runs a native command and stops when it fails.
function Invoke-Checked([string]$command, [string[]]$arguments) {
    & $command @arguments
    if ($LASTEXITCODE -ne 0) {
        Fail "'$command $($arguments -join ' ')' failed with exit code $LASTEXITCODE"
    }
}

foreach ($tool in 'cargo', 'rustc', 'git') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        Fail "'$tool' is required but not installed"
    }
}

if (git status --porcelain) {
    Fail 'working tree has uncommitted changes; commit or stash them first'
}

Write-Host '==> checking formatting, lints and tests'
Invoke-Checked cargo @('fmt', '--check')
Invoke-Checked cargo @('clippy', '--all-targets', '--locked', '--', '-D', 'warnings')
Invoke-Checked cargo @('test', '--locked')

Write-Host '==> building release binary'
Invoke-Checked cargo @('build', '--release', '--locked')

# `cargo pkgid` prints e.g. `path+file:///…#0.1.0` or `…#name@0.1.0`.
$pkgid = cargo pkgid
$version = ($pkgid -split '#')[-1]
$version = ($version -split '@')[-1]
$target = rustc -vV | Where-Object { $_ -like 'host: *' } | ForEach-Object { $_.Substring(6) }
if (-not $version -or -not $target) {
    Fail 'cannot determine version or host target'
}

$package = "$name-$version-$target"
$archive = "$package.zip"
$dist = Join-Path $root 'dist'
$staging = Join-Path $dist $package
$archivePath = Join-Path $dist $archive

Write-Host "==> packaging $archive"
New-Item -ItemType Directory -Force -Path $dist | Out-Null
if (Test-Path $archivePath) {
    Write-Host "note: replacing existing dist/$archive"
}
foreach ($path in $staging, $archivePath, "$archivePath.sha256") {
    if (Test-Path $path) {
        Remove-Item -Recurse -Force $path
    }
}
New-Item -ItemType Directory -Path $staging | Out-Null
Copy-Item (Join-Path $root "target/release/$name.exe"), README.md, LICENSE -Destination $staging
Compress-Archive -Path $staging -DestinationPath $archivePath
Remove-Item -Recurse -Force $staging

# Same format as `sha256sum`, so it can be checked with `sha256sum -c`.
$hash = (Get-FileHash -Algorithm SHA256 $archivePath).Hash.ToLower()
[IO.File]::WriteAllText("$archivePath.sha256", "$hash  $archive`n")

Write-Host '==> done'
Write-Host "archive:  dist/$archive"
Write-Host "checksum: $hash"

# Optional MSI installer, built with WiX Toolset v5 (`dotnet tool install --global wix --version 5.0.2`).
if (Get-Command wix -ErrorAction SilentlyContinue) {
    $arch = switch -Wildcard ($target) {
        'x86_64-*' { 'x64' }
        'aarch64-*' { 'arm64' }
        default { Fail "no MSI architecture for target $target" }
    }
    $msi = "$package.msi"
    $msiPath = Join-Path $dist $msi
    # MSI versions must be numeric: drop any pre-release or build suffix.
    $msiVersion = $version -replace '[-+].*$', ''

    Write-Host "==> packaging $msi"
    Invoke-Checked wix @(
        'build', (Join-Path $root 'wix/main.wxs'), '-arch', $arch,
        '-d', "Version=$msiVersion",
        '-d', "RootDir=$root",
        '-d', "BinDir=$(Join-Path $root 'target/release')",
        '-o', $msiPath
    )
    $msiHash = (Get-FileHash -Algorithm SHA256 $msiPath).Hash.ToLower()
    [IO.File]::WriteAllText("$msiPath.sha256", "$msiHash  $msi`n")
    Write-Host "msi:      dist/$msi"
    Write-Host "checksum: $msiHash"
} else {
    Write-Host 'note: wix not installed; skipping the MSI installer'
}
