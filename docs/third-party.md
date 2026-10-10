# Bundled media tools

RIPTV release downloads include **FFmpeg 8.1.3**, built for RIPTV by
[`scripts/build_ffmpeg.sh`](../scripts/build_ffmpeg.sh) and published as a pre-release of this
repository. It contains only what RIPTV uses and links **x264** (GPL) and **mbedTLS** (Apache 2.0;
Windows uses the system's TLS). NVIDIA's codec headers let it use NVENC/NVDEC where the driver is
present. The same pre-release also has an ffprobe that only RIPTV's tests use; the app doesn't
include it.

FFmpeg is a separate executable that RIPTV runs as a subprocess, and is distributed under
**GPL version 3 or later**. RIPTV's own code is under the [MIT License](../LICENSE); FFmpeg keeps its
own license because it runs as a separate program, not as part of RIPTV. The GPL text is embedded
in each app and available while it runs at `http://127.0.0.1:3000/licenses/COPYING.GPLv3`.

The pinned downloads and SHA-256 hashes are in `scripts/ffmpeg.json`. Release packaging verifies
them before executing the tools. macOS binaries subsequently receive an ad-hoc signature; package
checksums describe those final files.

The **ffmpeg-sources-*.tar.gz** asset, hosted alongside the application downloads on the same
RIPTV release, is the complete corresponding source: FFmpeg, x264 and nv-codec-headers at the
exact commits built, the mbedTLS release archive, and the build script that configures and builds
them for each platform (`build_ffmpeg.sh PLATFORM OUT_DIR`). Build tools and standard
operating-system libraries are not included.

FFmpeg is free software, supplied without warranty. Its authors and upstream library contributors
retain their copyrights. See the bundled license and source files for redistribution terms. Other
Rust dependency notices and available license texts are embedded under `licenses/` as well.
