#!/usr/bin/env python3
"""Fetch the May 2026 QEMU Sun-3 patch series as development reference."""
from __future__ import annotations

import html
import pathlib
import re
import sys
import urllib.request


def main() -> int:
    out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "build/qemu-patches")
    out.mkdir(parents=True, exist_ok=True)
    for message_id in range(1188133, 1188141):
        url = (
            "https://www.mail-archive.com/"
            f"qemu-devel%40nongnu.org/msg{message_id}.html"
        )
        request = urllib.request.Request(url, headers={"User-Agent": "sun3-rs/0.1"})
        with urllib.request.urlopen(request, timeout=30) as response:
            raw = response.read()
        (out / f"msg{message_id}.html").write_bytes(raw)
        text = raw.decode("utf-8", errors="replace")
        blocks = re.findall(r"<pre(?:\s[^>]*)?>(.*?)</pre>", text, re.I | re.S)
        if not blocks:
            raise RuntimeError(f"no <pre> patch body in {url}")
        body = max(blocks, key=len)
        body = re.sub(r"<[^>]+>", "", body)
        body = html.unescape(body).replace("\r\n", "\n")
        (out / f"msg{message_id}.patch").write_text(body, encoding="utf-8")
        print(f"{message_id}: {len(body)} bytes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
