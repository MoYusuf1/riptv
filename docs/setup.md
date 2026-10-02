# Setup help

RIPTV builds from source on Linux, macOS, and Windows 10/11 x64.
Windows users can use native PowerShell or Command Prompt, or follow the Linux
instructions in an Ubuntu WSL terminal.

## 1. Install prerequisites

Choose the commands for your system. FFmpeg also supplies `ffprobe`.

### Windows 10 / 11 (x64)

Open PowerShell and install the prerequisites with WinGet:

```powershell
winget install --exact --id Microsoft.VisualStudio.2022.BuildTools --source winget --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --exact --id Git.Git --source winget
winget install --exact --id Gyan.FFmpeg --source winget
winget install --exact --id Rustlang.Rustup --source winget
```

Let each installer finish and accept any Windows permission prompts. Then close
and reopen your terminal so it can find the installed tools. If Visual Studio is
already installed, use its installer to add **Desktop development with C++**,
including the **MSVC x64/x86 tools** and a **Windows SDK**.

If WinGet is unavailable, install those tools with their regular installers.
[Microsoft's Rust setup guide](https://learn.microsoft.com/en-us/windows/dev-environment/rust/setup)
explains the C++ and Rust requirements. Accept Rust's default MSVC toolchain.

### Ubuntu / Debian / Ubuntu in WSL

```sh
sudo apt update
sudo apt install git curl build-essential pkg-config libssl-dev ffmpeg
```

### Arch Linux

```sh
sudo pacman -S --needed git curl base-devel pkgconf openssl ffmpeg
```

### macOS

Install Apple's command-line developer tools:

```sh
xcode-select --install
```

With Homebrew installed, run:

```sh
brew install git ffmpeg pkg-config
```

### Rust

If you have not already installed Rust above, install its current stable toolchain at
<https://rustup.rs>, then reopen your terminal. If Rust is already installed
through rustup, `rustup update stable` updates it.

## 2. Build and start

```sh
git clone https://github.com/MoYusuf1/riptv.git
cd riptv
```

**Windows (PowerShell or Command Prompt):**

```powershell
.\scripts\setup.cmd
```

**Linux / macOS / WSL:**

```sh
./scripts/setup.sh
```

The script checks prerequisites, installs the Rust WebAssembly target and
Dioxus CLI 0.7.10 when needed, and builds both the app and server using the
checked-in dependency versions. It downloads dependencies from public sources;
GitHub credentials and SSH keys are not required.

On Windows, Dioxus is downloaded from its official release, checked against its
SHA-256 checksum, and kept in the ignored `.tools` folder. The `.cmd` launcher
runs the PowerShell script with a process-only execution-policy setting; it does
not change your system's policy. Paths containing spaces are supported.

Open **http://127.0.0.1:3000**. Keep the terminal running while you watch.
Choose **Quit** in the account menu, or press **Ctrl+C**, to stop. The first build can take several minutes; later builds
reuse what is already compiled.

## Useful commands

Run these from the `riptv` folder:

| Command | What it does |
| --- | --- |
| `cargo riptv` | Start the app after setup |
| `cargo riptv --check-updates` | Check for a newer release |
| `./scripts/setup.sh --build-only` | Build the app and server without starting them |
| `./scripts/setup.sh --logs` | Build and start with diagnostic logs |
| `cargo riptv --logs` | Start with logs without rebuilding the web app |
| `IPTV_PORT=3001 cargo riptv` | Start on port 3001 |

Windows equivalents:

```powershell
.\scripts\setup.cmd -BuildOnly
.\scripts\setup.cmd -Logs
$env:IPTV_PORT = '3001'
cargo riptv
```

`cargo riptv` and `cargo riptv --logs` work on all platforms after setup. Windows
logs are in `%TEMP%\riptv-diagnostics.log`; the server prints the exact path.

The app checks for new releases while running, including when started from a terminal.
To update your source copy, stop the app, run `git pull --ff-only`, then rerun your setup command.

## If setup fails

- **Missing tool:** install the prerequisites above, reopen your terminal, and retry.
- **Old Rust:** run `rustup update stable`, then retry.
- **Rust installed through a Linux package manager:** install that distribution's
  `wasm32-unknown-unknown` target package. The script can install the target itself
  only when rustup is available.
- **Build killed or out of memory:** retry with `CARGO_BUILD_JOBS=1 ./scripts/setup.sh`.
- **Address already in use:** stop the other RIPTV process, or choose another port.
- **Windows linker or SDK error:** open Visual Studio Installer and confirm that
  Desktop development with C++, MSVC x64/x86 tools, and a Windows SDK are installed.
- **Windows build runs out of memory:** set `$env:CARGO_BUILD_JOBS = '1'` in
  PowerShell, then rerun `.\scripts\setup.cmd`.

The setup scripts do not run `sudo` or install system packages. The Windows
setup is checked by the [Windows workflow](../.github/workflows/windows.yml),
which builds in a path containing spaces, runs native tests, and starts the
server with diagnostic logging.
