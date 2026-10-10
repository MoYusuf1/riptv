#!/usr/bin/env bash
# Build the minimal, static ffmpeg and ffprobe that RIPTV bundles, plus their corresponding source.
#
#   scripts/build_ffmpeg.sh linux-x64|windows-x64|macos-arm64|macos-x64 OUT_DIR
#
# Linux and macOS build natively; windows-x64 cross-compiles on Linux with mingw-w64.
# Needs: git, curl, make, a C compiler, nasm, pkg-config.
# Output: OUT_DIR/{ffmpeg,ffprobe}[.exe] and OUT_DIR/ffmpeg-sources.tar.gz (every pinned source,
# this script, and its licenses), with OUT_DIR/licenses and OUT_DIR/COPYING.GPLv3 unpacked.
#
# Only what RIPTV uses is enabled: the demuxers, decoders and parsers for IPTV streams and VOD
# files, AAC and H.264 encoding (x264, plus the platform's hardware encoder), the filters the
# commands in proxy/src/compat.rs ask for, and the lavfi sources the NVENC check and tests use.
set -euo pipefail

PLATFORM=${1:?platform}
OUT=$(mkdir -p "${2:?output directory}" && cd "$2" && pwd)
JOBS=${JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu)}
WORK=${WORK:-$OUT/.work}
SRC=$WORK/src PREFIX=$WORK/prefix
SCRIPT=$(cd "$(dirname "$0")" && pwd)/$(basename "$0")
mkdir -p "$SRC" "$PREFIX"

# name, repository, commit (tag in the comment). Changing one means a new ffmpeg release.
GIT_SOURCES="
ffmpeg https://github.com/FFmpeg/FFmpeg.git 1041abdc962f4cc4f394aa8de9dc5236c0c3b9e7
x264 https://code.videolan.org/videolan/x264.git b35605ace3ddf7c1a5d67a2eb553f034aef41d55
nv-codec-headers https://github.com/FFmpeg/nv-codec-headers.git 1889e62e2d35ff7aa9baca2bceb14f053785e6f1
"
# ffmpeg n8.1.3 · x264 stable · nv-codec-headers n12.1.14.0 (NVIDIA driver 530+)
MBEDTLS_URL=https://github.com/Mbed-TLS/mbedtls/releases/download/mbedtls-3.6.7/mbedtls-3.6.7.tar.bz2
MBEDTLS_SHA256=a7e8bcbec0e6f761b4af24f25677626b35f762f68eef79c08677a363212d11f6

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }

fetch() {
    echo "$GIT_SOURCES" | while read -r name url commit; do
        [ -n "$name" ] || continue
        [ -d "$SRC/$name" ] && continue
        git init -q "$SRC/$name"
        git -C "$SRC/$name" fetch -q --depth 1 "$url" "$commit"
        git -C "$SRC/$name" -c advice.detachedHead=false checkout -q FETCH_HEAD
        [ "$(git -C "$SRC/$name" rev-parse HEAD)" = "$commit" ] || { echo "wrong $name revision" >&2; exit 1; }
    done
    if [ ! -d "$SRC/mbedtls" ]; then
        curl -fsSL --retry 3 -o "$SRC/mbedtls.tar.bz2" "$MBEDTLS_URL"
        [ "$(sha256 "$SRC/mbedtls.tar.bz2")" = "$MBEDTLS_SHA256" ] || { echo "mbedtls checksum mismatch" >&2; exit 1; }
        mkdir "$SRC/mbedtls" && tar -xjf "$SRC/mbedtls.tar.bz2" -C "$SRC/mbedtls" --strip-components 1
    fi
}

CC=cc CROSS="" HOST_FLAGS=() EXTRA=()
case $PLATFORM in
linux-x64)
    TLS=mbedtls
    # glibc stays dynamic so NVENC/NVDEC can load the driver; everything else is static.
    EXTRA=(--enable-cuda --enable-ffnvcodec --enable-nvenc --enable-nvdec
        --enable-encoder=h264_nvenc --enable-hwaccel=h264_nvdec,hevc_nvdec,mpeg2_nvdec
        --extra-ldflags=-static-libgcc) ;;
windows-x64)
    TLS=schannel CROSS=x86_64-w64-mingw32- CC=${CROSS}gcc
    HOST_FLAGS=(--host=x86_64-w64-mingw32 --cross-prefix=$CROSS)
    # Media Foundation reaches Intel, AMD and NVIDIA encoders; NVENC directly when present.
    EXTRA=(--target-os=mingw32 --arch=x86_64 --cross-prefix=$CROSS --enable-w32threads
        --enable-schannel --enable-d3d11va --enable-dxva2 --enable-mediafoundation
        --enable-cuda --enable-ffnvcodec --enable-nvenc --enable-nvdec
        --enable-encoder=h264_mf,h264_nvenc
        --enable-hwaccel=h264_d3d11va,hevc_d3d11va,h264_dxva2,hevc_dxva2,h264_nvdec,hevc_nvdec
        --extra-ldflags=-static) ;;
macos-arm64 | macos-x64)
    TLS=mbedtls
    ARCH=$([ "$PLATFORM" = macos-arm64 ] && echo arm64 || echo x86_64)
    CC="clang -arch $ARCH"
    HOST_FLAGS=(--host="$ARCH-apple-darwin")
    EXTRA=(--arch="$ARCH" --cc="$CC" --enable-videotoolbox --enable-encoder=h264_videotoolbox
        --enable-hwaccel=h264_videotoolbox,hevc_videotoolbox) ;;
*) echo "unknown platform $PLATFORM" >&2; exit 1 ;;
esac

fetch

if [ ! -f "$PREFIX/lib/libx264.a" ]; then
    (cd "$SRC/x264" && CC="$CC" ./configure --prefix="$PREFIX" "${HOST_FLAGS[@]}" \
        --enable-static --enable-pic --bit-depth=8 --disable-cli --disable-opencl --disable-lavf \
        --disable-swscale --disable-ffms --disable-gpac --disable-lsmash \
        && make -j"$JOBS" && make install && make distclean)
fi
if [ "$TLS" = mbedtls ] && [ ! -f "$PREFIX/lib/libmbedtls.a" ]; then
    (cd "$SRC/mbedtls" && make -j"$JOBS" -C library CC="$CC" AR="${CROSS}ar" \
        CFLAGS="-O2 -fPIC" libmbedcrypto.a libmbedx509.a libmbedtls.a \
        && mkdir -p "$PREFIX/lib" "$PREFIX/include" \
        && cp library/libmbed*.a "$PREFIX/lib/" && cp -R include/mbedtls include/psa "$PREFIX/include/" \
        && make -C library clean)
    EXTRA+=(--enable-mbedtls)
elif [ "$TLS" = mbedtls ]; then
    EXTRA+=(--enable-mbedtls)
fi
make -C "$SRC/nv-codec-headers" PREFIX="$PREFIX" install >/dev/null

# Components, by what uses them. ffmpeg inserts buffer/format/scale/aresample/trim filters itself.
DEMUXERS=mpegts,hls,mov,matroska,avi,flv,mpegps,mpegvideo,aac,ac3,eac3,dts,truehd,mp3,ogg,wav,h264,hevc,lavfi
DECODERS=h264,hevc,mpeg2video,mpeg1video,mpeg4,aac,aac_latm,ac3,eac3,mp2,mp2float,mp3,mp3float,dca,truehd,mlp,flac,opus,vorbis,alac,pcm_s16le,pcm_s16be,pcm_s24le,pcm_bluray,pcm_dvd,wrapped_avframe
PARSERS=h264,hevc,mpegvideo,mpeg4video,aac,aac_latm,ac3,mpegaudio,dca,mlp,flac,opus,vorbis
BSFS=aac_adtstoasc,h264_mp4toannexb,hevc_mp4toannexb,extract_extradata,null
FILTERS=buffer,buffersink,abuffer,abuffersink,null,anull,format,aformat,scale,aresample,trim,atrim,yadif,transpose,hflip,vflip,setpts,asetpts,nullsrc,testsrc,testsrc2,sine

cd "$SRC/ffmpeg"
echo n8.1.3 > VERSION  # shown by -version; the shallow clone has no tags
PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig" ./configure \
    --prefix="$PREFIX" --pkg-config=pkg-config --pkg-config-flags=--static \
    --extra-cflags="-I$PREFIX/include" --extra-ldflags="-L$PREFIX/lib" \
    --enable-gpl --enable-version3 --enable-static --disable-shared \
    --disable-autodetect --disable-everything --disable-doc --disable-debug --disable-ffplay \
    --disable-outdevs --enable-indev=lavfi --enable-network \
    --enable-libx264 --enable-encoder=libx264,aac,ac3 \
    --enable-protocol=file,pipe,http,https,tcp,tls,crypto,data,udp \
    --enable-demuxer=$DEMUXERS --enable-decoder=$DECODERS --enable-parser=$PARSERS \
    --enable-bsf=$BSFS --enable-filter=$FILTERS --enable-muxer=mp4,mpegts,matroska,null \
    "${EXTRA[@]}"
make -j"$JOBS"
EXE=$([ "$PLATFORM" = windows-x64 ] && echo .exe || true)
for tool in ffmpeg ffprobe; do
    "${CROSS}strip" -o "$OUT/$tool$EXE" "$tool$EXE" 2>/dev/null || strip -o "$OUT/$tool$EXE" "$tool$EXE"
done
make distclean >/dev/null

# Corresponding source: every input above, exactly as built, with this script and the licenses.
cd "$WORK"
rm -rf "$OUT/licenses" sources && mkdir -p "$OUT/licenses" sources/ffmpeg-sources
for name in ffmpeg x264 nv-codec-headers; do
    git -C "$SRC/$name" archive --format=tar --prefix="$name/" HEAD > "sources/ffmpeg-sources/$name.tar"
done
cp "$SRC/mbedtls.tar.bz2" sources/ffmpeg-sources/
cp "$SCRIPT" sources/ffmpeg-sources/build_ffmpeg.sh
for name in ffmpeg x264 nv-codec-headers mbedtls; do
    find "$SRC/$name" -maxdepth 1 -type f \( -iname 'LICENSE*' -o -iname 'COPYING*' -o -iname 'LICENCE*' \) \
        -exec sh -c 'mkdir -p "$1" && cp "$2" "$1/"' _ "$OUT/licenses/$name" {} \;
done
cp "$SRC/ffmpeg/COPYING.GPLv3" "$OUT/COPYING.GPLv3"
tar -czf "$OUT/ffmpeg-sources.tar.gz" -C sources ffmpeg-sources
"$OUT/ffmpeg$EXE" -hide_banner -version 2>/dev/null | head -1 || true
ls -l "$OUT"
