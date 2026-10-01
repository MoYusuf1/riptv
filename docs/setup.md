# Setup help

RIPTV builds from source on Linux and macOS. On Windows, use an Ubuntu terminal
in WSL and follow the Ubuntu instructions below.

## 1. Install prerequisites

Choose the commands for your system. FFmpeg also supplies `ffprobe`.

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

Install the current stable Rust toolchain using the installer at
<https://rustup.rs>, then reopen your terminal. If Rust is already installed
through rustup, `rustup update stable` updates it.

## 2. Build and start

```sh
git clone https://github.com/MoYusuf1/riptv.git
cd riptv
./scripts/setup.sh
```

The script checks prerequisites, installs the Rust WebAssembly target and
Dioxus CLI 0.7.10 when needed, and builds both the app and server using the
checked-in dependency versions. It downloads dependencies from public sources;
GitHub credentials and SSH keys are not required.

Open **http://127.0.0.1:3000**. Keep the terminal running while you watch.
Press **Ctrl+C** to stop. The first build can take several minutes; later builds
reuse what is already compiled.

## Useful commands

Run these from the `riptv` folder:

| Command | What it does |
| --- | --- |
| `cargo riptv` | Start the app after setup |
| `./scripts/setup.sh --build-only` | Build the app and server without starting them |
| `./scripts/setup.sh --logs` | Build and start with diagnostic logs |
| `cargo riptv --logs` | Start with logs without rebuilding the web app |
| `IPTV_PORT=3001 cargo riptv` | Start on port 3001 |

To update, stop the app, run `git pull --ff-only`, then `./scripts/setup.sh`.

## If setup fails

- **Missing tool:** install the prerequisites above, reopen your terminal, and retry.
- **Old Rust:** run `rustup update stable`, then retry.
- **Rust installed through a Linux package manager:** install that distribution's
  `wasm32-unknown-unknown` target package. The script can install the target itself
  only when rustup is available.
- **Build killed or out of memory:** retry with `CARGO_BUILD_JOBS=1 ./scripts/setup.sh`.
- **Address already in use:** stop the other RIPTV process, or choose another port.

The setup script does not run `sudo` or change your system packages.
