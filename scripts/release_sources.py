#!/usr/bin/env python3
"""Fetch the bundled FFmpeg's corresponding source (verified) and unpack its licenses. Stdlib only."""
import hashlib
import io
import json
from pathlib import Path
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
DEST = ROOT / "target/release-sources"
MANIFEST = json.loads((ROOT / "scripts/ffmpeg.json").read_text())
LICENSES = ("LICENSE", "LICENCE", "COPYING", "NOTICE")


def main():
    tag = MANIFEST["tag"]
    url = f'https://github.com/{MANIFEST["repository"]}/releases/download/{tag}/ffmpeg-sources.tar.gz'
    request = urllib.request.Request(url, headers={"User-Agent": "RIPTV-release"})
    with urllib.request.urlopen(request, timeout=120) as response:
        data = response.read()
    if hashlib.sha256(data).hexdigest() != MANIFEST["sources"]:
        raise RuntimeError("FFmpeg source archive checksum mismatch")
    DEST.mkdir(parents=True, exist_ok=True)
    (DEST / f'ffmpeg-sources-{tag.removeprefix("ffmpeg-")}.tar.gz').write_bytes(data)
    # Each source is an archive inside the archive; copy the top-level license files of each.
    with tarfile.open(fileobj=io.BytesIO(data)) as outer:
        for member in outer.getmembers():
            if not (member.isfile() and member.name.endswith((".tar", ".tar.bz2"))):
                continue
            with tarfile.open(fileobj=outer.extractfile(member)) as inner:
                for file in inner.getmembers():
                    parts = file.name.split("/")
                    if file.isfile() and len(parts) == 2 and parts[1].upper().startswith(LICENSES):
                        name = parts[0].split("-3.")[0]  # mbedtls-3.6.7 → mbedtls
                        out = DEST / "licenses" / name / parts[1]
                        out.parent.mkdir(parents=True, exist_ok=True)
                        out.write_bytes(inner.extractfile(file).read())
    (DEST / "COPYING.GPLv3").write_bytes((DEST / "licenses/ffmpeg/COPYING.GPLv3").read_bytes())
    print("FFmpeg corresponding source verified and licenses unpacked.")


if __name__ == "__main__":
    main()
