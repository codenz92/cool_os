#!/usr/bin/env python3

import sys
from pathlib import Path
from typing import Callable


PixelPredicate = Callable[[int, int, int], bool]


def read_ppm(path: Path) -> tuple[int, int, bytes]:
    with path.open("rb") as fh:
        if fh.readline().strip() != b"P6":
            raise AssertionError(f"{path}: not a P6 PPM")
        line = fh.readline()
        while line.startswith(b"#"):
            line = fh.readline()
        width, height = [int(part) for part in line.split()]
        maxval = int(fh.readline())
        if maxval != 255:
            raise AssertionError(f"{path}: unsupported max value {maxval}")
        pixels = fh.read()
    expected = width * height * 3
    if len(pixels) < expected:
        raise AssertionError(f"{path}: truncated pixel data")
    return width, height, pixels


def count_pixels(
    width: int,
    height: int,
    pixels: bytes,
    predicate: PixelPredicate,
    x0: int = 0,
    y0: int = 0,
    x1: int | None = None,
    y1: int | None = None,
) -> int:
    x1 = width if x1 is None else min(width, x1)
    y1 = height if y1 is None else min(height, y1)
    total = 0
    for y in range(max(0, y0), max(0, y1)):
        row = y * width * 3
        for x in range(max(0, x0), max(0, x1)):
            r, g, b = pixels[row + x * 3 : row + x * 3 + 3]
            if predicate(r, g, b):
                total += 1
    return total


def longest_horizontal_run(
    width: int,
    height: int,
    pixels: bytes,
    predicate: PixelPredicate,
    x0: int = 0,
    y0: int = 0,
    x1: int | None = None,
    y1: int | None = None,
) -> tuple[int, int]:
    x1 = width if x1 is None else min(width, x1)
    y1 = height if y1 is None else min(height, y1)
    best = 0
    best_y = y0
    for y in range(max(0, y0), max(0, y1)):
        row = y * width * 3
        run = 0
        for x in range(max(0, x0), max(0, x1)):
            r, g, b = pixels[row + x * 3 : row + x * 3 + 3]
            if predicate(r, g, b):
                run += 1
                if run > best:
                    best = run
                    best_y = y
            else:
                run = 0
    return best, best_y


def cyan_edge(r: int, g: int, b: int) -> bool:
    return r < 90 and g > 95 and b > 120


def bright_cyan(r: int, g: int, b: int) -> bool:
    return r < 95 and g > 130 and b > 170


def menu_cyan(r: int, g: int, b: int) -> bool:
    return r < 80 and g > 100 and b > 150


def dark_panel(r: int, g: int, b: int) -> bool:
    return r < 22 and g < 42 and b < 76


def readable_text(r: int, g: int, b: int) -> bool:
    return r > 120 and g > 150 and b > 170


def alert_red(r: int, g: int, b: int) -> bool:
    return r > 170 and 45 < g < 175 and 45 < b < 175


def assert_start_menu(path: Path) -> None:
    width, height, pixels = read_ppm(path)
    scan_w = min(width, 760)
    best_run, best_y = longest_horizontal_run(
        width, height, pixels, menu_cyan, 0, height // 4, scan_w, height - 90
    )
    min_run = min(240, max(120, width // 5))
    if best_run < min_run:
        raise AssertionError(f"{path}: start menu top edge missing, best run={best_run}")

    panel_x1 = min(scan_w, max(120, best_run))
    panel_pixels = count_pixels(
        width, height, pixels, dark_panel, 0, best_y + 8, panel_x1, best_y + 180
    )
    if panel_pixels < panel_x1 * 20:
        raise AssertionError(f"{path}: start menu dark body missing")


def assert_window_profile(path: Path, name: str) -> None:
    width, height, pixels = read_ppm(path)
    best_run, best_y = longest_horizontal_run(
        width, height, pixels, cyan_edge, 0, 40, min(width, 900), height - 120
    )
    if best_run < 180:
        raise AssertionError(f"{path}: {name} window chrome missing, best run={best_run}")

    cyan = count_pixels(width, height, pixels, bright_cyan)
    text = count_pixels(width, height, pixels, readable_text)
    if cyan < 300:
        raise AssertionError(f"{path}: {name} accent pixels too low: {cyan}")
    if text < 350:
        raise AssertionError(f"{path}: {name} readable text pixels too low: {text}")
    if best_y < 40 or best_y > height - 140:
        raise AssertionError(f"{path}: {name} chrome appears outside app area at y={best_y}")


def assert_crash_dialog(path: Path) -> None:
    width, height, pixels = read_ppm(path)
    x0 = width // 4
    x1 = width - x0
    y0 = height // 4
    y1 = height - y0
    best_run, _ = longest_horizontal_run(
        width, height, pixels, alert_red, x0, y0, x1, y1
    )
    red = count_pixels(width, height, pixels, alert_red, x0, y0, x1, y1)
    text = count_pixels(width, height, pixels, readable_text, x0, y0, x1, y1)
    if best_run < 200:
        raise AssertionError(f"{path}: crash dialog alert edge missing, best run={best_run}")
    if red < 700:
        raise AssertionError(f"{path}: crash dialog alert fill too low: {red}")
    if text < 180:
        raise AssertionError(f"{path}: crash dialog text too low: {text}")


def parse_arg(raw: str) -> tuple[str, Path]:
    if "=" not in raw:
        raise SystemExit(f"expected profile=path argument, got {raw!r}")
    profile, path = raw.split("=", 1)
    return profile, Path(path)


def main(argv: list[str]) -> int:
    if not argv:
        raise SystemExit("usage: ppm_visual_assert.py profile=path [...]")

    checks = {
        "start-menu": assert_start_menu,
        "settings": lambda path: assert_window_profile(path, "settings"),
        "diagnostics": lambda path: assert_window_profile(path, "diagnostics"),
        "crash-dialog": assert_crash_dialog,
    }

    for raw in argv:
        profile, path = parse_arg(raw)
        if profile not in checks:
            raise SystemExit(f"unknown visual profile {profile!r}")
        if not path.exists():
            raise AssertionError(f"{path}: screenshot does not exist")
        checks[profile](path)
        print(f"visual profile ok: {profile} {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
