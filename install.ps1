# Install the synthetic-customers command on Windows.
#
#   powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/edouard38000/synthetic-customers/main/install.ps1 | iex"
#
# Downloads the prebuilt binary from the GitHub release, checks its SHA-256
# checksum, and copies it to %LOCALAPPDATA%\Programs\synthetic-customers\bin
# (no administrator rights needed).
#
# Optional settings (environment variables):
#   SYNTHETIC_CUSTOMERS_VERSION      release tag to install, e.g. v0.1.0 (default: latest)
#   SYNTHETIC_CUSTOMERS_INSTALL_DIR  where to put the binary
#
# Needs no token or account; it only downloads public release files.

function Install-SyntheticCustomers {
    # Set inside the function so running this through `iex` does not change
    # the caller's session.
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'  # Invoke-WebRequest is much faster without the progress bar
    $Repo = 'edouard38000/synthetic-customers'
    $Bin = 'synthetic-customers'

    # Windows PowerShell 5.1 may default to old TLS versions that GitHub refuses.
    if ($PSVersionTable.PSVersion.Major -lt 6) {
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    }

    # Only x64 builds exist. Windows on ARM runs them through its x64 emulation.
    $arch = $env:PROCESSOR_ARCHITECTURE
    if ($env:PROCESSOR_ARCHITEW6432) { $arch = $env:PROCESSOR_ARCHITEW6432 }
    switch ($arch) {
        'AMD64' { }
        'ARM64' { Write-Host 'Windows on ARM: installing the x64 build, which runs under emulation on Windows 11.' }
        default { throw "No prebuilt binary for Windows on '$arch'. Build from source with: cargo install --git https://github.com/$Repo" }
    }
    $target = 'x86_64-pc-windows-msvc'
    $archive = "$Bin-$target.zip"

    $version = $env:SYNTHETIC_CUSTOMERS_VERSION
    if (-not $version) { $version = 'latest' }
    if ($version -eq 'latest') {
        $base = "https://github.com/$Repo/releases/latest/download"
    } else {
        if (-not $version.StartsWith('v')) { $version = "v$version" }
        $base = "https://github.com/$Repo/releases/download/$version"
    }

    $installDir = $env:SYNTHETIC_CUSTOMERS_INSTALL_DIR
    if (-not $installDir) { $installDir = Join-Path $env:LOCALAPPDATA "Programs\$Bin\bin" }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("$Bin-install-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp -Force | Out-Null
    try {
        Write-Host "Downloading $Bin ($version) for $target..."
        $archivePath = Join-Path $tmp $archive
        $sumsPath = Join-Path $tmp 'SHA256SUMS.txt'
        try {
            Invoke-WebRequest -UseBasicParsing -Uri "$base/$archive" -OutFile $archivePath
            Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS.txt" -OutFile $sumsPath
        } catch {
            throw "Could not download from $base (is there a published release $version?): $($_.Exception.Message)"
        }

        $expected = $null
        foreach ($line in Get-Content -Path $sumsPath) {
            $parts = $line.Trim() -split '\s+', 2
            if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $archive) { $expected = $parts[0].ToLowerInvariant() }
        }
        if (-not $expected) { throw "No checksum listed for $archive." }
        $actual = (Get-FileHash -Algorithm SHA256 -Path $archivePath).Hash.ToLowerInvariant()
        if ($expected -ne $actual) {
            throw "Checksum mismatch for $archive (expected $expected, got $actual). Nothing was installed."
        }
        Write-Host 'Checksum OK.'

        $extractDir = Join-Path $tmp 'x'
        Expand-Archive -Path $archivePath -DestinationPath $extractDir -Force
        $src = Join-Path $extractDir "$Bin-$target\$Bin.exe"
        if (-not (Test-Path -LiteralPath $src)) { throw "The download did not contain $Bin.exe." }

        New-Item -ItemType Directory -Path $installDir -Force | Out-Null
        $dest = Join-Path $installDir "$Bin.exe"
        try {
            Copy-Item -LiteralPath $src -Destination $dest -Force
        } catch {
            throw "Could not write $dest. If $Bin is running, close it and try again. ($($_.Exception.Message))"
        }
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host ''
    Write-Host "Installed $Bin to $dest"
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $onPath = @($userPath -split ';') + @($env:Path -split ';') |
        Where-Object { $_ -and ($_.TrimEnd('\') -ieq $installDir.TrimEnd('\')) }
    if ($onPath) {
        Write-Host "Run '$Bin --help' to get started."
    } else {
        Write-Host ''
        Write-Host "$installDir is not on your PATH yet. To add it for your user, run:"
        Write-Host ''
        $q = $installDir -replace "'", "''"
        $qDest = $dest -replace "'", "''"
        Write-Host "  [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ';$q', 'User')"
        Write-Host ''
        Write-Host 'then open a new terminal window. Until then you can run it as:'
        Write-Host "  & '$qDest' --help"
    }
}

Install-SyntheticCustomers
