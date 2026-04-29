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
    return parser.parse_args()


def build_command(args: argparse.Namespace, monitor_socket: str | None = None) -> list[str]:
    cmd = [
        "qemu-system-x86_64",
        f"-drive",
        f"format=raw,file={args.bios},snapshot=on",
        f"-drive",
        f"file={args.fsimg},if=ide,format=raw,index=1,snapshot=on",
        "-m",
        "512M",
        "-vga",
        "std",
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


def request_screendump(monitor_socket: str, out_path: str) -> None:
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
                sock.sendall(f"screendump {out_path}\n".encode())
                time.sleep(0.2)
                return
        except OSError as exc:
            last_error = exc
            time.sleep(0.1)
    raise RuntimeError(f"unable to connect to QEMU monitor: {last_error}")


def assert_desktop_framebuffer(path: str) -> None:
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


def main() -> int:
    args = parse_args()
    monitor_socket = None
    if args.screendump:
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
        return proc.returncode or 1

    missing = [pattern for pattern in args.expect if pattern not in output]
    if missing:
        for pattern in missing:
            print(f"missing expected output: {pattern}", file=sys.stderr)
        return 1

    if args.screendump:
        if not os.path.exists(args.screendump):
            print(f"missing screendump: {args.screendump}", file=sys.stderr)
            return 1
        if args.expect_framebuffer_desktop:
            try:
                assert_desktop_framebuffer(args.screendump)
            except AssertionError as exc:
                print(f"framebuffer assertion failed: {exc}", file=sys.stderr)
                return 1

    print(f"smoke ok after {args.seconds:.1f}s", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
