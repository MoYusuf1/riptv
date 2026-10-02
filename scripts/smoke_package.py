#!/usr/bin/env python3
"""Test the downloaded app without external resources, system FFmpeg, or a repo cwd."""
import argparse
import functools
import http.server
import json
import os
import re
import shutil
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import tomllib
import urllib.parse
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent.parent


def request(url, data=None):
    headers = {"Content-Type": "application/json"} if data is not None else {}
    req = urllib.request.Request(url, data=data, headers=headers)
    return urllib.request.urlopen(req, timeout=30)


def smoke(archive):
    with tempfile.TemporaryDirectory(prefix="RIPTV release test with spaces ") as temporary:
        work = Path(temporary)
        if archive.suffix == ".zip":
            with zipfile.ZipFile(archive) as z:
                z.extractall(work)
            executable = work / "RIPTV.app/Contents/MacOS/riptv"
            executable.chmod(0o755)
        else:
            executable = work / ("riptv.exe" if os.name == "nt" else "riptv")
            shutil.copy2(archive, executable)
            executable.chmod(0o755)
        version = subprocess.check_output([str(executable), "--version"], text=True).strip().removeprefix("RIPTV ")
        assert version == tomllib.loads((ROOT / "proxy/Cargo.toml").read_text())["package"]["version"]
        with socket.socket() as port_socket:
            port_socket.bind(("127.0.0.1", 0))
            port = port_socket.getsockname()[1]
        env = os.environ.copy()
        env["IPTV_PORT"] = str(port)
        env.pop("IPTV_WEB", None)
        server = subprocess.Popen([str(executable), "--logs", "--no-open"], cwd=work,
                                  env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        upstream = http.server.ThreadingHTTPServer(("127.0.0.1", 0),
            functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(ROOT / "proxy/fixtures")))
        threading.Thread(target=upstream.serve_forever, daemon=True).start()
        try:
            base = f"http://127.0.0.1:{port}"
            for _ in range(100):
                if server.poll() is not None:
                    raise RuntimeError(server.stdout.read().decode())
                try:
                    with request(base + "/") as response:
                        assert b"<html" in response.read()
                    break
                except OSError:
                    time.sleep(0.1)
            else:
                raise RuntimeError("Packaged server did not become ready")
            with request(base + "/") as response:
                html = response.read().decode()
            scripts = re.findall(r'src="([^"]+\.js)"', html)
            wasm = []
            for script in scripts:
                with request(base + script) as response:
                    js = response.read().decode()
                wasm.extend(re.findall(r'["\x27](/[^"\x27]+\.wasm)["\x27]', js))
            assert scripts and wasm
            for asset in scripts + wasm + ["/icon.svg"]:
                with request(base + asset) as response:
                    assert response.status == 200 and len(response.read()) > 100, asset
            with request(base + "/diagnostics") as response:
                assert json.load(response) == []
            with request(base + "/licenses/COPYING.GPLv3") as response:
                assert b"GNU GENERAL PUBLIC LICENSE" in response.read()
            with request(base + "/allow?host=127.0.0.1", b"") as response:
                assert response.status == 204
            for fixture, mode in (("h264_ac3.ts", "copy"), ("hevc_ac3.ts", "transcode")):
                source = f"http://127.0.0.1:{upstream.server_port}/{fixture}"
                query = urllib.parse.urlencode({"url": source})
                with request(base + "/compat/check?" + query) as response:
                    assert response.status == 204
                    assert response.headers["x-riptv-video"] in ("copy", "transcode")
                with request(base + "/compat?" + query + "&video=" + mode) as response:
                    output = response.read()
                    assert len(output) > 1000 and b"ftyp" in output[:64], (fixture, len(output))
            print("PASS: single app, version, embedded web assets, logging, bundled audio conversion and HEVC transcode")
        finally:
            server.terminate()
            try:
                log, _ = server.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                log, _ = server.communicate()
            print(log.decode(errors="replace"))
            upstream.shutdown()
            upstream.server_close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    smoke(parser.parse_args().archive.resolve())
