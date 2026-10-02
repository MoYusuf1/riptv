#!/usr/bin/env python3
"""Build a portable release from already-compiled server and web assets. Stdlib only."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = json.loads((ROOT / "scripts/ffmpeg.json").read_text())


def download(url, destination, digest):
    # Fail closed before executing or redistributing any downloaded executable.
    request = urllib.request.Request(url, headers={"User-Agent": "RIPTV-release"})
    with urllib.request.urlopen(request, timeout=120) as response:
        with destination.open("wb") as output:
            shutil.copyfileobj(response, output)
    actual = hashlib.sha256(destination.read_bytes()).hexdigest()
    if actual != digest:
        raise RuntimeError(f"SHA-256 mismatch for {destination.name}: {actual}")


def notices(destination, target):
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", target], cwd=ROOT))
    index = []
    for package in metadata["packages"]:
        if package["source"] is None:
            continue
        folder = Path(package["manifest_path"]).parent
        files = set()
        for pattern in ("LICENSE*", "LICENCE*", "COPYING*", "NOTICE*"):
            files.update(p for p in folder.glob(pattern) if p.is_file())
        if package.get("license_file"):
            files.add(folder / package["license_file"])
        name = f'{package["name"]}-{package["version"]}'
        for path in files:
            if path.is_file():
                out = destination / name / path.name
                out.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(path, out)
        index.append({"name": name, "license": package["license"], "source": package["source"],
                      "repository": package["repository"], "license_files": sorted(p.name for p in files)})
    destination.mkdir(parents=True, exist_ok=True)
    (destination / "rust-dependencies.json").write_text(json.dumps(index, indent=2) + "\n")


def package(platform, version, server, web, output, target):
    suffix = ".exe" if platform.startswith("windows") else ""
    if not server.is_file() or not (web / "index.html").is_file():
        raise RuntimeError("Build the native server and web app before packaging")
    output.mkdir(parents=True, exist_ok=True)
    stem = f"riptv-{version}-{platform}"
    with tempfile.TemporaryDirectory(prefix="riptv-package-") as temporary:
        bundle = Path(temporary) / stem
        bundle.mkdir()
        shutil.copy2(server, bundle / f"riptv{suffix}")
        shutil.copytree(web, bundle / "web")
        (bundle / "bin").mkdir()
        for asset, digest in MANIFEST["assets"][platform].items():
            name = "ffprobe" if asset.startswith("ffprobe") else "ffmpeg"
            path = bundle / "bin" / (name + suffix)
            url = f'https://github.com/{MANIFEST["repository"]}/releases/download/{MANIFEST["tag"]}/{asset}'
            download(url, path, digest)
            path.chmod(0o755)
            subprocess.run([str(path), "-version"], check=True, stdout=subprocess.DEVNULL)
        (bundle / f"riptv{suffix}").chmod(0o755)
        notices(bundle / "licenses", target)
        shutil.copytree(ROOT / "target/release-sources/licenses", bundle / "licenses/media")
        shutil.copy2(ROOT / "docs/third-party.md", bundle / "licenses/FFmpeg-NOTICE.md")
        shutil.copy2(ROOT / "scripts/ffmpeg.json", bundle / "licenses/ffmpeg-downloads.json")
        shutil.copy2(ROOT / "target/release-sources/COPYING.GPLv3", bundle / "licenses/COPYING.GPLv3")
        instruction = "Double-click riptv.exe." if suffix else "Run ./riptv in this folder."
        if platform.startswith("macos"):
            launcher = bundle / "RIPTV.command"
            launcher.write_text('#!/bin/sh\ncd "$(dirname "$0")"\nexec ./riptv "$@"\n')
            launcher.chmod(0o755)
            instruction = "Double-click RIPTV.command."
        (bundle / "START-HERE.txt").write_text(
            f"RIPTV {version}\n\nExtract the entire folder first. {instruction}\n"
            "Your browser opens automatically at http://127.0.0.1:3000.\n"
            "Keep the terminal window open while watching; press Ctrl+C to stop.\n"
            "Choose Public TV to try free channels, or Add your IPTV account/playlist.\n\n"
            "Rust, Git and a separate FFmpeg installation are NOT needed.\n"
            "Keep bin/ and web/ alongside riptv. Do not run inside the archive.\n"
            "Profiles are saved in your browser; use a trusted device.\n"
            "Unsigned builds may trigger Windows/macOS security warnings. Verify the\n"
            "download came from MoYusuf1/riptv Releases before choosing to allow it.\n\n"
            "Help: https://github.com/MoYusuf1/riptv/blob/main/docs/downloads.md\n"
            "FFmpeg licenses: licenses/. Corresponding source: the release's\n"
            "ffmpeg-sources archive. RIPTV source: the release tag on GitHub.\n")
        # Ad-hoc signing lets macOS validate executable integrity; it is NOT notarization.
        if platform.startswith("macos"):
            for binary in [bundle / "riptv", bundle / "bin/ffmpeg", bundle / "bin/ffprobe"]:
                subprocess.run(["codesign", "--force", "--sign", "-", str(binary)], check=True)
        hashes = []
        for path in sorted(bundle.rglob("*")):
            if path.is_file():
                hashes.append(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(bundle).as_posix()}")
        (bundle / "SHA256SUMS.txt").write_text("\n".join(hashes) + "\n")
        if suffix:
            archive = output / (stem + ".zip")
            with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as z:
                for path in sorted(bundle.rglob("*")):
                    if path.is_file():
                        z.write(path, path.relative_to(bundle.parent))
        else:
            archive = output / (stem + ".tar.gz")
            with tarfile.open(archive, "w:gz") as tar:
                tar.add(bundle, arcname=stem)
    print(archive)
    return archive


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=MANIFEST["assets"], required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--server", type=Path, required=True)
    parser.add_argument("--web", type=Path, default=ROOT / "target/dx/app/release/web/public")
    parser.add_argument("--output", type=Path, default=ROOT / "target/packages")
    args = parser.parse_args()
    package(args.platform, args.version, args.server.resolve(), args.web.resolve(), args.output.resolve(), args.target)
