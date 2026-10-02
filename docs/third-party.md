# Bundled media tools

RIPTV release downloads include **FFmpeg and ffprobe 8.1.2**, built by
[Shaka's static-ffmpeg-binaries project](https://github.com/shaka-project/static-ffmpeg-binaries/releases/tag/n8.1.2-1).
These are separate executables, invoked by RIPTV as subprocesses. They include
GPL components and are distributed under **GPL version 3 or later**. The GPL
text is included as `licenses/COPYING.GPLv3` in each download.

The pinned downloads and SHA-256 hashes are in `scripts/ffmpeg.json`. Release
packaging verifies them before executing the tools. macOS binaries subsequently
receive an ad-hoc signature; package checksums describe those final files.

The **ffmpeg-sources-n8.1.2-1.tar.gz** asset, hosted alongside the application
downloads on the same RIPTV release, contains the corresponding FFmpeg, libvpx,
SVT-AV1, x264, x265, LAME, Opus and mbedTLS sources, their licenses, exact git
revisions, and Shaka's original build scripts and patches. Unpack each inner
archive to rebuild; follow the included build-scripts archive's README and
numbered scripts. Those scripts document the platform configuration and the
mbedTLS compiler-warning patch. The Linux static variant uses Alpine/musl;
macOS and Windows use their native build environments. Build tools and standard
operating-system libraries are not included.

FFmpeg is free software, supplied without warranty. Its authors and upstream
library contributors retain their copyrights. See the bundled license and
source files for redistribution terms. Other Rust dependency notices and
available license texts are included in `licenses/`.
