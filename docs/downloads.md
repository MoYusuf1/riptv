# Download help

Get an application archive from [GitHub Releases](https://github.com/MoYusuf1/riptv/releases/latest),
not GitHub's automatically generated “Source code” downloads.

| Your computer | Choose | Start after extracting |
| --- | --- | --- |
| Windows 10/11, 64-bit Intel/AMD | `windows-x64.zip` | Double-click `riptv.exe` |
| Linux, 64-bit Intel/AMD | `linux-x64.tar.gz` | Open a terminal in the folder and run `./riptv` |
| Mac with an M-series chip | `macos-arm64.tar.gz` | Double-click `RIPTV.command` |
| Mac with an Intel chip | `macos-x64.tar.gz` | Double-click `RIPTV.command` |

Linux builds target Ubuntu 22.04 or newer (glibc 2.35+). macOS release builds
target macOS 15 or newer. ARM Linux and ARM Windows downloads are not included
yet; [build from source](setup.md) instead.

Extract **all** files together. The executable, `web/`, and `bin/` must stay in
the same folder. Everything needed to run RIPTV is included, apart from a
modern browser and standard operating-system libraries. No installer, Rust,
Git, administrator privileges, or separate FFmpeg installation is needed.

The browser opens at **http://127.0.0.1:3000**. If it does not, open that address
yourself. Keep the terminal window open while watching; press **Ctrl+C** to
stop. Double-clicking again starts another copy, so stop the first one first.

## Security warnings

These community builds are **not Windows publisher-signed or Apple-notarized**.
Verify that you downloaded them from `MoYusuf1/riptv` on GitHub. If your system
blocks the download, review the warning and decide whether you trust this
release; do not disable your system's security protection. You can also build
from source. A `SHA256SUMS.txt` release asset is available to check downloads.

## Common problems

- **Blank page or missing web app:** extract the whole folder, not just the executable.
- **Port 3000 is busy:** close the other RIPTV window. Advanced: set `IPTV_PORT`
  to another port, then launch again.
- **Channel buffers:** try another feed or lower resolution. A player cannot
  repair a provider sending video slower than it plays.
- **Need logs:** start `./riptv --logs` (Windows: `.\riptv.exe --logs`).
  The terminal prints the sanitized diagnostic log location.
- **Profiles disappeared:** use the same browser and local address as before.
  Changing the browser or port uses separate browser storage.

To update, stop the old copy and extract a new release into its own folder.
Your browser's saved profiles are unaffected.

## How releases are checked

Each platform is built and tested on its native GitHub runner. The extracted
archive is tested from a path containing spaces and an unrelated working
directory. Checks cover app/JavaScript/WebAssembly serving, bundled FFmpeg and
ffprobe, audio conversion, video transcoding, and diagnostic logging. Releases
are published only after all platform checks succeed. Checksums and the
corresponding FFmpeg source archive accompany the downloads.

[Bundled software and licenses](third-party.md)
