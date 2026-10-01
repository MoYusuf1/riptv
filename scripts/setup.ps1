#requires -Version 5.1
[CmdletBinding()]
param(
    [switch]$BuildOnly,
    [switch]$Logs,
    [switch]$Help
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($Help) {
    Write-Host 'Usage: .\scripts\setup.cmd [-BuildOnly] [-Logs]'
    Write-Host 'Builds RIPTV and starts it locally. Requires Rust, Git, FFmpeg and MSVC C++ tools.'
    Write-Host 'See docs/setup.md for the Windows prerequisites.'
    exit 0
}

# Windows PowerShell does not throw when a native program returns a failure code.
function Invoke-Checked {
    param([string]$Program, [string[]]$Arguments)
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Program failed (exit $LASTEXITCODE). Fix the error above, then run setup again."
    }
}

function Test-DioxusVersion {
    param([string]$Program)
    $version = & $Program --version
    return ($LASTEXITCODE -eq 0 -and "$version" -match '^dioxus\s+0\.7\.10(?:\s|$)')
}

$setupExit = 0
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    if ($env:OS -ne 'Windows_NT') {
        throw 'Use ./scripts/setup.sh on Linux or macOS.'
    }
    foreach ($tool in @('cargo', 'rustc', 'rustup', 'git', 'ffmpeg', 'ffprobe')) {
        if (-not (Get-Command $tool -CommandType Application -ErrorAction SilentlyContinue)) {
            throw "Missing $tool. Follow the Windows instructions in docs/setup.md, reopen your terminal, and retry."
        }
    }

    $rustInfo = Invoke-Checked -Program rustc -Arguments @('-vV')
    if (-not ($rustInfo -match '^host: x86_64-pc-windows-msvc$')) {
        throw 'Windows setup requires x64 MSVC Rust. Install it with rustup default stable-x86_64-pc-windows-msvc.'
    }

    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
        throw 'Install Visual Studio Build Tools with Desktop development with C++ (including the Windows SDK). See docs/setup.md.'
    }
    $visualStudio = Invoke-Checked -Program $vswhere -Arguments @(
        '-latest', '-products', '*', '-requires',
        'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'
    )
    if (-not $visualStudio) {
        throw 'The MSVC C++ tools are missing. Add Desktop development with C++ and the Windows SDK in Visual Studio Installer.'
    }

    $wasmLibDir = Invoke-Checked -Program rustc -Arguments @('--print', 'target-libdir', '--target', 'wasm32-unknown-unknown')
    if (-not (Test-Path -Path (Join-Path $wasmLibDir 'libcore-*.rlib'))) {
        Write-Host 'Installing the Rust WebAssembly target...'
        Invoke-Checked -Program rustup -Arguments @('target', 'add', 'wasm32-unknown-unknown')
    }

    $dxPath = $null
    $systemDx = Get-Command dx -CommandType Application -ErrorAction SilentlyContinue
    if ($systemDx -and (Test-DioxusVersion $systemDx.Source)) {
        $dxPath = $systemDx.Source
    } else {
        # Keep the official build tool in this checkout; don't replace a global dx installation.
        $toolDir = Join-Path (Get-Location).Path '.tools\dioxus-0.7.10-windows-x64'
        if (Test-Path -LiteralPath $toolDir) {
            $cached = @(Get-ChildItem -LiteralPath $toolDir -Filter dx.exe -File -Recurse)
            if ($cached.Count -eq 1 -and (Test-DioxusVersion $cached[0].FullName)) {
                $dxPath = $cached[0].FullName
            }
        }
        if (-not $dxPath) {
            Write-Host 'Downloading Dioxus CLI 0.7.10 for Windows...'
            $null = New-Item -ItemType Directory -Force -Path $toolDir
            $archive = Join-Path $toolDir 'dx.zip'
            [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
            Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/DioxusLabs/dioxus/releases/download/v0.7.10/dx-x86_64-pc-windows-msvc.zip' -OutFile $archive
            $expectedHash = '45eb4f87b7f86fdba8508ee7e0ef1f9e13cf8f2ffcc6a2d5858d0e8ea7dd1531'
            if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expectedHash) {
                throw 'Dioxus download checksum did not match. Run setup again to download a fresh copy.'
            }
            Expand-Archive -LiteralPath $archive -DestinationPath $toolDir -Force
            $installed = @(Get-ChildItem -LiteralPath $toolDir -Filter dx.exe -File -Recurse)
            if ($installed.Count -ne 1 -or -not (Test-DioxusVersion $installed[0].FullName)) {
                throw 'Dioxus could not start. Check the installation error above.'
            }
            $dxPath = $installed[0].FullName
        }
    }

    Write-Host 'Building RIPTV (the first build can take several minutes)...'
    Invoke-Checked -Program $dxPath -Arguments @('build', '--web', '--release', '--locked', '-p', 'app')
    Invoke-Checked -Program cargo -Arguments @('build', '--release', '--locked', '-p', 'riptv')

    if ($BuildOnly) {
        Write-Host "Build complete. Run 'cargo riptv' to start RIPTV."
    } else {
        $port = if ($env:IPTV_PORT) { $env:IPTV_PORT } else { '3000' }
        Write-Host "Open http://127.0.0.1:$port once RIPTV starts. Press Ctrl+C to stop it."
        $serverArgs = @('run', '--release', '--locked', '-p', 'riptv', '--')
        if ($Logs) { $serverArgs += '--logs' }
        Invoke-Checked -Program cargo -Arguments $serverArgs
    }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    $setupExit = 1
} finally {
    Pop-Location
}
exit $setupExit
