#!/usr/bin/env python3

import argparse
import binascii
import os
import struct
import sys
import zlib
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Convert P6 PPM smoke artifacts to PNG.")
    parser.add_argument("paths", nargs="+", help="PPM file or directory containing PPM files")
    return parser.parse_args()


def read_token(fh) -> bytes:
    token = bytearray()
    while True:
        ch = fh.read(1)
        if not ch:
            raise ValueError("unexpected EOF while reading PPM header")
        if ch == b"#":
            fh.readline()
            continue
        if ch.isspace():
            if token:
                return bytes(token)
            continue
        token.extend(ch)


def read_ppm(path: Path) -> tuple[int, int, bytes]:
    with path.open("rb") as fh:
        if read_token(fh) != b"P6":
            raise ValueError("not a P6 PPM")
        width = int(read_token(fh))
        height = int(read_token(fh))
        maxval = int(read_token(fh))
        if maxval != 255:
            raise ValueError("only maxval=255 PPM files are supported")
        pixels = fh.read()

    expected = width * height * 3
    if len(pixels) < expected:
        raise ValueError("truncated PPM pixel data")
    return width, height, pixels[:expected]


def png_chunk(kind: bytes, data: bytes) -> bytes:
    payload = kind + data
    return (
        struct.pack(">I", len(data))
        + payload
        + struct.pack(">I", binascii.crc32(payload) & 0xFFFFFFFF)
    )


def write_png(path: Path, width: int, height: int, pixels: bytes) -> None:
    rows = []
    stride = width * 3
    for y in range(height):
        rows.append(b"\x00" + pixels[y * stride : (y + 1) * stride])
    raw = b"".join(rows)
    data = b"".join(
        [
            b"\x89PNG\r\n\x1a\n",
            png_chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)),
            png_chunk(b"IDAT", zlib.compress(raw, level=6)),
            png_chunk(b"IEND", b""),
        ]
    )
    path.write_bytes(data)


def iter_ppms(paths: list[str]) -> list[Path]:
    out: list[Path] = []
    for raw in paths:
        path = Path(raw)
        if path.is_dir():
            out.extend(sorted(path.glob("*.ppm")))
        elif path.suffix.lower() == ".ppm":
            out.append(path)
    return out


def main() -> int:
    args = parse_args()
    converted = 0
    for ppm in iter_ppms(args.paths):
        try:
            width, height, pixels = read_ppm(ppm)
            png = ppm.with_suffix(".png")
            write_png(png, width, height, pixels)
            print(f"converted {ppm} -> {png}")
            converted += 1
        except Exception as exc:
            print(f"failed to convert {ppm}: {exc}", file=sys.stderr)
            return 1
    if converted == 0:
        print("no PPM files found")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
