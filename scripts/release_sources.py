#!/usr/bin/env python3
"""Archive corresponding FFmpeg + codec sources and the original build scripts."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
DEST = ROOT / "target/release-sources"
SOURCES = [
    ("build-scripts", "https://github.com/shaka-project/static-ffmpeg-binaries.git", "n8.1.2-1"),
    ("ffmpeg", "https://github.com/FFmpeg/FFmpeg.git", "n8.1.2"),
    ("libvpx", "https://chromium.googlesource.com/webm/libvpx", "v1.16.0"),
    ("svt-av1", "https://gitlab.com/AOMediaCodec/SVT-AV1.git", "v4.1.0"),
    ("x264", "https://code.videolan.org/videolan/x264.git", "0480cb0"),
    ("x265", "https://bitbucket.org/multicoreware/x265_git.git", "4.2"),
    ("opus", "https://github.com/xiph/opus.git", "v1.6.1"),
    ("mbedtls", "https://github.com/ARMmbed/mbedtls.git", "v3.4.1"),
]
COMMITS = {
    "build-scripts": "88caac417541f3bb678fa6670cb73f2d74c7aaf9",
    "ffmpeg": "38b88335f99e76ed89ff3c93f877fdefce736c13",
    "libvpx": "1024874c5919305883187e2953de8fcb4c3d7fa6",
    "svt-av1": "c04f951541ad600e0d9c10836f2ab7b9bc69816d",
    "x264": "0480cb05fa188d37ae87e8f4fd8f1aea3711f7ee",
    "x265": "e444744c03978c1fb4e037168967020cf2648427",
    "opus": "22244de5a79bd1d6d623c32e72bf1954b56235be",
    "mbedtls": "72718dd87e087215ce9155a826ee5a66cfbe9631",
}


def collect_licenses(folder, name):
    for path in folder.rglob("*"):
        if path.is_file() and path.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE")):
            out = DEST / "licenses" / name / path.relative_to(folder)
            out.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, out)


def main():
    DEST.mkdir(parents=True, exist_ok=True)
    records = []
    with tempfile.TemporaryDirectory(prefix="riptv-sources-") as temporary:
        work = Path(temporary)
        with tarfile.open(DEST / "ffmpeg-sources-n8.1.2-1.tar.gz", "w:gz") as archive:
            for name, url, ref in SOURCES:
                folder = work / name
                command = ["git", "-c", "advice.detachedHead=false", "clone", "--quiet"]
                if name != "x264":
                    command += ["--depth", "1", "--branch", ref]
                subprocess.run(command + [url, str(folder)], check=True)
                if name == "x264":
                    subprocess.run(["git", "-C", str(folder), "checkout", "--quiet", ref], check=True)
                revision = subprocess.check_output(["git", "-C", str(folder), "rev-parse", "HEAD"], text=True).strip()
                if revision != COMMITS[name]:
                    raise RuntimeError(f"Unexpected source revision for {name}: {revision}")
                collect_licenses(folder, name)
                source_tar = work / (name + ".tar")
                with source_tar.open("wb") as output:
                    subprocess.run(["git", "-C", str(folder), "archive", "--format=tar", "HEAD"], stdout=output, check=True)
                archive.add(source_tar, arcname=f"ffmpeg-sources/{name}.tar")
                records.append({"name": name, "url": url, "ref": ref, "commit": revision,
                                "sha256": hashlib.sha256(source_tar.read_bytes()).hexdigest()})
                if name == "ffmpeg":
                    shutil.copy2(folder / "COPYING.GPLv3", DEST / "COPYING.GPLv3")
            lame = work / "lame-3.100.tar.gz"
            with urllib.request.urlopen("https://sourceforge.net/projects/lame/files/lame/3.100/lame-3.100.tar.gz/download", timeout=120) as response:
                with lame.open("wb") as output:
                    shutil.copyfileobj(response, output)
            if hashlib.sha256(lame.read_bytes()).hexdigest() != "ddfe36cab873794038ae2c1210557ad34857a4b6bdc515785d1da9e175b1da1e":
                raise RuntimeError("LAME source checksum mismatch")
            # Validate the response is a source archive, not a CDN error page.
            with tarfile.open(lame) as check:
                if "lame-3.100/COPYING" not in check.getnames():
                    raise RuntimeError("Invalid LAME source archive")
                check.extractall(work / "lame", filter="data")
            collect_licenses(work / "lame", "lame")
            archive.add(lame, arcname="ffmpeg-sources/lame-3.100.tar.gz")
            records.append({"name": "lame", "version": "3.100", "sha256": hashlib.sha256(lame.read_bytes()).hexdigest()})
            manifest = work / "sources.json"
            manifest.write_text(json.dumps(records, indent=2) + "\n")
            archive.add(manifest, arcname="ffmpeg-sources/sources.json")
            archive.add(ROOT / "docs/third-party.md", arcname="ffmpeg-sources/README.md")
    print("FFmpeg corresponding sources archived.")


if __name__ == "__main__":
    main()
