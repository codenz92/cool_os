#!/usr/bin/env python3

import argparse
import os
import socket
import subprocess
import sys
import tempfile
import time


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run coolOS under headless QEMU for a short smoke test.")
    parser.add_argument("--bios", required=True, help="Path to bios.img")
    parser.add_argument("--fsimg", required=True, help="Path to fs.img")
    parser.add_argument("--seconds", type=float, default=6.0, help="How long to let QEMU run")
    parser.add_argument("--memory", default="512M", help="QEMU memory size, e.g. 256M")
    parser.add_argument("--smp", default="1", help="QEMU SMP CPU count")
    parser.add_argument("--vga", default="std", help="QEMU VGA adapter")
    parser.add_argument("--usb", action="store_true", help="Attach xHCI with USB keyboard and mouse")
    parser.add_argument(
        "--expect",
        action="append",
        default=[],
        help="Substring expected in the combined QEMU output; may be passed multiple times",
    )
    parser.add_argument("--screendump", help="Write a QEMU framebuffer PPM before stopping")
    parser.add_argument(
        "--expect-framebuffer-desktop",
        action="store_true",
        help="Assert the screendump contains the desktop taskbar instead of the splash screen",
    )
    parser.add_argument(
        "--expect-framebuffer-start-menu",
        action="store_true",
        help="Assert the screendump contains the open Start menu panel",
    )
    parser.add_argument(
        "--hmp",
        action="append",
        default=[],
        help="QEMU HMP monitor command to run before screendump; may be passed multiple times",
    )
    parser.add_argument(
        "--post-hmp-delay",
        type=float,
        default=0.5,
        help="Seconds to wait after HMP commands before screendump",
    )
    parser.add_argument(
        "--type-text",
        action="append",
        default=[],
        help="Text to inject through QEMU HMP sendkey before screendump; may include \\n",
    )
    parser.add_argument(
        "--artifact-dir",
        help="Directory for per-smoke QEMU logs and screenshots copied for CI artifacts",
    )
    parser.add_argument(
        "--artifact-name",
        default="qemu-smoke",
        help="Stable artifact filename stem used with --artifact-dir",
    )
    parser.add_argument(
        "--expect-framebuffer-window",
        action="store_true",
        help="Assert the screendump contains an open desktop window",
    )
    parser.add_argument(
        "--expect-framebuffer-dialog",
        action="store_true",
        help="Assert the screendump contains an open shell dialog",
    )
    return parser.parse_args()


def build_command(args: argparse.Namespace, monitor_socket: str | None = None) -> list[str]:
    cmd = [
        "qemu-system-x86_64",
        f"-drive",
        f"format=raw,file={args.bios},snapshot=on",
        f"-drive",
        f"file={args.fsimg},if=ide,format=raw,index=1,snapshot=on",
        "-m",
        args.memory,
        "-smp",
        args.smp,
        "-vga",
        args.vga,
        "-display",
        "none",
        "-debugcon",
        "stdio",
    ]
    if monitor_socket:
        cmd.extend(["-monitor", f"unix:{monitor_socket},server,nowait"])
    if args.usb:
        cmd.extend(
            [
                "-device",
                "qemu-xhci,id=xhci",
                "-device",
                "usb-kbd,bus=xhci.0",
                "-device",
                "usb-mouse,bus=xhci.0",
            ]
        )
    return cmd


def run_monitor_command(monitor_socket: str, command: str) -> None:
    deadline = time.time() + 3.0
    last_error: OSError | None = None
    while time.time() < deadline:
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
                sock.settimeout(1.0)
                sock.connect(monitor_socket)
                try:
                    sock.recv(4096)
                except TimeoutError:
                    pass
                sock.sendall(f"{command}\n".encode())
                time.sleep(0.2)
                return
        except OSError as exc:
            last_error = exc
            time.sleep(0.1)
    raise RuntimeError(f"unable to connect to QEMU monitor: {last_error}")


def request_screendump(monitor_socket: str, out_path: str) -> None:
    run_monitor_command(monitor_socket, f"screendump {out_path}")


def hmp_key_for_char(ch: str) -> str | None:
    if ch == "\n" or ch == "\r":
        return "ret"
    if ch == " ":
        return "spc"
    if ch == ".":
        return "dot"
    if ch == "/":
        return "slash"
    if ch == "-":
        return "minus"
    if ch == "_":
        return "shift-minus"
    if ch == ":":
        return "shift-semicolon"
    if ch == ">":
        return "shift-dot"
    if ch.isascii() and ch.isalpha():
        return ch.lower()
    if ch.isascii() and ch.isdigit():
        return ch
    return None


def type_text(monitor_socket: str, text: str) -> None:
    for ch in text:
        key = hmp_key_for_char(ch)
        if key is None:
            raise RuntimeError(f"unsupported HMP text character: {ch!r}")
        run_monitor_command(monitor_socket, f"sendkey {key}")


def read_ppm(path: str) -> tuple[int, int, bytes]:
    with open(path, "rb") as fh:
        if fh.readline().strip() != b"P6":
            raise AssertionError("screendump is not P6 PPM")
        line = fh.readline()
        while line.startswith(b"#"):
            line = fh.readline()
        width, height = [int(part) for part in line.split()]
        maxval = int(fh.readline())
        if maxval != 255:
            raise AssertionError("unsupported PPM max value")
        pixels = fh.read()

    if len(pixels) < width * height * 3:
        raise AssertionError("truncated framebuffer screendump")
    return width, height, pixels


def assert_desktop_framebuffer(path: str) -> None:
    width, height, pixels = read_ppm(path)
    cyan_bottom = 0
    y0 = max(0, height - 80)
    for y in range(y0, height):
        row = y * width * 3
        for x in range(width):
            r, g, b = pixels[row + x * 3 : row + x * 3 + 3]
            if r < 80 and g > 90 and b > 130:
                cyan_bottom += 1

    if cyan_bottom < width // 3:
        raise AssertionError("desktop taskbar cyan edge not found in framebuffer")


def assert_start_menu_framebuffer(path: str) -> None:
    width, height, pixels = read_ppm(path)
    scan_w = min(width, 760)
    y_start = max(0, height // 4)
    y_end = max(y_start, height - 90)
    best_run = 0
    best_y = 0

    for y in range(y_start, y_end):
        row = y * width * 3
        run = 0
        for x in range(scan_w):
            r, g, b = pixels[row + x * 3 : row + x * 3 + 3]
            is_cyan = r < 80 and g > 100 and b > 150
            if is_cyan:
                run += 1
                if run > best_run:
                    best_run = run
                    best_y = y
            else:
                run = 0

    min_run = min(240, max(120, width // 5))
    if best_run < min_run:
        raise AssertionError(
            f"start menu cyan top edge not found; best horizontal run {best_run} at y={best_y}"
        )

    dark_panel_pixels = 0
    panel_x1 = min(scan_w, max(120, best_run))
    for y in range(best_y + 8, min(height - 45, best_y + 180)):
        row = y * width * 3
        for x in range(0, panel_x1):
            r, g, b = pixels[row + x * 3 : row + x * 3 + 3]
            if r < 20 and g < 40 and b < 70:
                dark_panel_pixels += 1

    if dark_panel_pixels < panel_x1 * 20:
        raise AssertionError("start menu dark panel body not found below cyan edge")


def assert_window_framebuffer(path: str) -> None:
    width, height, pixels = read_ppm(path)
    scan_y0 = 40
    scan_y1 = max(scan_y0, height - 120)
    best_run = 0
    best_y = 0
    for y in range(scan_y0, scan_y1):
        row = y * width * 3
        run = 0
        for x in range(0, min(width, 900)):
            r, g, b = pixels[row + x * 3 : row + x * 3 + 3]
            is_window_edge = r < 90 and g > 95 and b > 120
            if is_window_edge:
                run += 1
                if run > best_run:
                    best_run = run
                    best_y = y
            else:
                run = 0
    if best_run < 180:
        raise AssertionError(
            f"desktop window chrome not found; best horizontal run {best_run} at y={best_y}"
        )


def assert_dialog_framebuffer(path: str) -> None:
    width, height, pixels = read_ppm(path)
    x0 = width // 4
    x1 = width - x0
    y0 = height // 4
    y1 = height - y0
    best_run = 0
    best_y = 0
    for y in range(y0, y1):
        row = y * width * 3
        run = 0
        for x in range(x0, x1):
            r, g, b = pixels[row + x * 3 : row + x * 3 + 3]
            is_dialog_alert = r > 170 and g > 60 and g < 170 and b > 60 and b < 170
            if is_dialog_alert:
                run += 1
                if run > best_run:
                    best_run = run
                    best_y = y
            else:
                run = 0
    if best_run < 240:
        raise AssertionError(
            f"shell dialog alert edge not found; best horizontal run {best_run} at y={best_y}"
        )


def artifact_path(args: argparse.Namespace, suffix: str) -> str | None:
    if not args.artifact_dir:
        return None
    safe_name = "".join(
        ch if ch.isalnum() or ch in ("-", "_", ".") else "-" for ch in args.artifact_name
    )
    return os.path.join(args.artifact_dir, f"{safe_name}{suffix}")


def write_artifacts(args: argparse.Namespace, cmd: list[str], output: str, result: str) -> None:
    if not args.artifact_dir:
        return
    os.makedirs(args.artifact_dir, exist_ok=True)
    log_path = artifact_path(args, ".log")
    if log_path:
        with open(log_path, "w", encoding="utf-8") as fh:
            fh.write("+ ")
            fh.write(" ".join(cmd))
            fh.write("\n")
            fh.write(f"result={result}\n")
            fh.write(f"seconds={args.seconds:.1f}\n")
            fh.write(output)
            if output and not output.endswith("\n"):
                fh.write("\n")
    if args.screendump and os.path.exists(args.screendump):
        ppm_path = artifact_path(args, ".ppm")
        if ppm_path and os.path.abspath(ppm_path) != os.path.abspath(args.screendump):
            with open(args.screendump, "rb") as src, open(ppm_path, "wb") as dst:
                dst.write(src.read())


def main() -> int:
    args = parse_args()
    monitor_socket = None
    if args.artifact_dir:
        os.makedirs(args.artifact_dir, exist_ok=True)
    if args.screendump or args.hmp or args.type_text:
        monitor_socket = os.path.join(tempfile.gettempdir(), f"cool-os-qemu-{os.getpid()}.sock")
        try:
            os.unlink(monitor_socket)
        except FileNotFoundError:
            pass

    cmd = build_command(args, monitor_socket)
    print("+ " + " ".join(cmd), flush=True)

    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )

    try:
        output, _ = proc.communicate(timeout=args.seconds)
        exited_early = True
    except subprocess.TimeoutExpired:
        if (args.hmp or args.type_text) and monitor_socket:
            for command in args.hmp:
                run_monitor_command(monitor_socket, command)
            for text in args.type_text:
                type_text(monitor_socket, text.replace("\\n", "\n"))
            time.sleep(max(0.0, args.post_hmp_delay))
        if args.screendump and monitor_socket:
            request_screendump(monitor_socket, args.screendump)
        proc.terminate()
        try:
            output, _ = proc.communicate(timeout=2.0)
        except subprocess.TimeoutExpired:
            proc.kill()
            output, _ = proc.communicate()
        exited_early = False

    if output:
        sys.stdout.write(output)
        if not output.endswith("\n"):
            sys.stdout.write("\n")

    if exited_early and proc.returncode not in (0, None):
        print(f"qemu exited early with status {proc.returncode}", file=sys.stderr)
        write_artifacts(args, cmd, output, f"qemu-exit-{proc.returncode}")
        return proc.returncode or 1

    missing = [pattern for pattern in args.expect if pattern not in output]
    if missing:
        for pattern in missing:
            print(f"missing expected output: {pattern}", file=sys.stderr)
        write_artifacts(args, cmd, output, "missing-output")
        return 1

    if args.screendump:
        if not os.path.exists(args.screendump):
            print(f"missing screendump: {args.screendump}", file=sys.stderr)
            write_artifacts(args, cmd, output, "missing-screendump")
            return 1
        if args.expect_framebuffer_desktop:
            try:
                assert_desktop_framebuffer(args.screendump)
            except AssertionError as exc:
                print(f"framebuffer assertion failed: {exc}", file=sys.stderr)
                write_artifacts(args, cmd, output, "desktop-framebuffer-failed")
                return 1
        if args.expect_framebuffer_start_menu:
            try:
                assert_start_menu_framebuffer(args.screendump)
            except AssertionError as exc:
                print(f"start menu framebuffer assertion failed: {exc}", file=sys.stderr)
                write_artifacts(args, cmd, output, "start-menu-framebuffer-failed")
                return 1
        if args.expect_framebuffer_window:
            try:
                assert_window_framebuffer(args.screendump)
            except AssertionError as exc:
                print(f"window framebuffer assertion failed: {exc}", file=sys.stderr)
                write_artifacts(args, cmd, output, "window-framebuffer-failed")
                return 1
        if args.expect_framebuffer_dialog:
            try:
                assert_dialog_framebuffer(args.screendump)
            except AssertionError as exc:
                print(f"dialog framebuffer assertion failed: {exc}", file=sys.stderr)
                write_artifacts(args, cmd, output, "dialog-framebuffer-failed")
                return 1

    print(f"smoke ok after {args.seconds:.1f}s", flush=True)
    write_artifacts(args, cmd, output, "ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
