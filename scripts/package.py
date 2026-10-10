#!/usr/bin/env python3
"""Build a single-file app with its web app and media tools embedded. Stdlib only."""
import argparse
import hashlib
import json
import os
import plistlib
import re
from pathlib import Path
import shutil
import subprocess
import tempfile
import urllib.request

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
    filename = {"windows-x64": "RIPTV-Windows.exe", "linux-x64": "RIPTV-Linux",
                "macos-arm64": "RIPTV-Mac-AppleSilicon.zip", "macos-x64": "RIPTV-Mac-Intel.zip"}[platform]
    with tempfile.TemporaryDirectory(prefix="riptv-package-") as temporary:
        bundle = Path(temporary) / "resources"
        bundle.mkdir()
        shutil.copytree(web, bundle / "web")
        # Local builds retain old hashed assets; embed only the current app version.
        html = (bundle / "web/index.html").read_text()
        current_js = re.findall(r'src="([^"]*/assets/app-[^"]+\.js)"', html)
        keep = {Path(p).name for p in current_js}
        for name in list(keep):
            js = (bundle / "web/assets" / name).read_text()
            keep.update(Path(p).name for p in re.findall(r'["\x27](/[^"\x27]+\.wasm)["\x27]', js))
        if not current_js or not any(p.endswith(".wasm") for p in keep):
            raise RuntimeError("Missing current JavaScript or WebAssembly")
        for pattern in ("app-*.js", "app_bg-*.wasm"):
            for path in (bundle / "web/assets").glob(pattern):
                if path.name not in keep:
                    path.unlink()
        (bundle / "bin").mkdir()
        # Keep native test tools outside the published downloads; the app needs only ffmpeg.
        tools = ROOT / "target/test-tools" / platform / "bin"
        tools.mkdir(parents=True, exist_ok=True)
        for asset, digest in MANIFEST["assets"][platform].items():
            name = "ffprobe" if asset.startswith("ffprobe") else "ffmpeg"
            path = (tools if name == "ffprobe" else bundle / "bin") / (name + suffix)
            url = f'https://github.com/{MANIFEST["repository"]}/releases/download/{MANIFEST["tag"]}/{asset}'
            download(url, path, digest)
            path.chmod(0o755)
            subprocess.run([str(path), "-version"], check=True, stdout=subprocess.DEVNULL)
            if platform.startswith("macos"):
                subprocess.run(["codesign", "--force", "--sign", "-", str(path)], check=True)
        notices(bundle / "licenses", target)
        shutil.copytree(ROOT / "target/release-sources/licenses", bundle / "licenses/media")
        shutil.copy2(ROOT / "docs/third-party.md", bundle / "licenses/FFmpeg-NOTICE.md")
        shutil.copy2(ROOT / "scripts/ffmpeg.json", bundle / "licenses/ffmpeg-downloads.json")
        shutil.copy2(ROOT / "target/release-sources/COPYING.GPLv3", bundle / "licenses/COPYING.GPLv3")
        shutil.copy2(ROOT / "LICENSE", bundle / "licenses/RIPTV-LICENSE")
        shutil.copytree(bundle / "licenses", bundle / "web/licenses")
        hashes = []
        for path in sorted(bundle.rglob("*")):
            if path.is_file():
                hashes.append(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(bundle).as_posix()}")
        (bundle / "SHA256SUMS.txt").write_text("\n".join(hashes) + "\n")
        for path in (bundle / "bin").iterdir():
            shutil.copy2(path, tools / path.name)
        env = os.environ.copy()
        env["IPTV_BUNDLE_DIR"] = str(bundle)
        subprocess.run(["cargo", "build", "--release", "--locked", "-p", "riptv", "--target", target],
                       cwd=ROOT, env=env, check=True)
        server = ROOT / "target" / target / "release" / ("riptv" + suffix)
        if subprocess.check_output([str(server), "--version"], text=True).strip() != f"RIPTV {version}":
            raise RuntimeError("Package version does not match the executable")
        if platform.startswith("macos"):
            app = Path(temporary) / "RIPTV.app"
            binary = app / "Contents/MacOS/riptv"
            binary.parent.mkdir(parents=True)
            shutil.copy2(server, binary)
            binary.chmod(0o755)
            with (app / "Contents/Info.plist").open("wb") as info:
                plistlib.dump({"CFBundleExecutable": "riptv", "CFBundleIdentifier": "io.github.moyusuf1.riptv",
                              "CFBundleName": "RIPTV", "CFBundlePackageType": "APPL",
                              "CFBundleShortVersionString": version, "CFBundleVersion": version,
                              "LSMinimumSystemVersion": "15.0"}, info)
            # Integrity signing only; these builds are still not Apple-notarized.
            subprocess.run(["codesign", "--force", "--sign", "-", str(app)], check=True)
            archive = output / filename
            subprocess.run(["ditto", "-c", "-k", "--keepParent", str(app), str(archive)], check=True)
        else:
            archive = output / filename
            shutil.copy2(server, archive)
            archive.chmod(0o755)
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
