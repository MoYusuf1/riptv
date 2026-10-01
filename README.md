# RIPTV

A lightweight IPTV player for your browser. Watch live TV, movies, and series using
an Xtream account or an M3U/M3U8 playlist.

RIPTV runs on your computer. No hosting account or GitHub login is needed.

## Get started

1. Install **Rust, Git, and FFmpeg** using the [setup guide](docs/setup.md).
2. Run:

   ```sh
   git clone https://github.com/MoYusuf1/riptv.git
   cd riptv
   ```

3. Start setup for your system:

   | System | Command |
   | --- | --- |
   | Windows (PowerShell or Command Prompt) | `.\scripts\setup.cmd` |
   | Linux / macOS | `./scripts/setup.sh` |

4. Open **http://127.0.0.1:3000**. Choose **Public TV** to try free channels,
   or **Add** to connect your own account or playlist.

The script installs the WebAssembly target and Dioxus build tool if needed, then
builds and starts RIPTV. The first build can take several minutes.
Keep the terminal open while watching; press **Ctrl+C** to stop.

Native Windows setup supports Windows 10/11 x64. WSL also works with the Linux instructions.

## Start again

From the `riptv` folder:

```sh
cargo riptv
```

## Update

Stop RIPTV, then run:

```sh
git pull --ff-only
```

Then run the setup command for your system again.

## Troubleshooting

- **A channel won't play?** Confirm FFmpeg is installed; some public channels
  may be offline or unavailable in your region.
- **Port 3000 is busy?** Run `IPTV_PORT=3001 cargo riptv` and open
  `http://127.0.0.1:3001`. In Windows PowerShell, use
  `$env:IPTV_PORT=3001; cargo riptv`.
- **Need logs?** Run `cargo riptv --logs`. The terminal prints the log location.
  See [diagnostics](docs/reference.md#diagnostics-why-a-channel-wont-play-or-keeps-buffering)
  for testing and log details.

Profiles, including passwords, are stored in this browser on your computer.
Use RIPTV on a trusted device. The server is local to this computer; phone access
and public hosting require a separate secure deployment.

[Setup help](docs/setup.md) · [Features and development](docs/reference.md) ·
[Live playback internals](docs/live-playback.md)
