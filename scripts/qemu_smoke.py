#!/usr/bin/env python3

import argparse
import subprocess
import sys
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
    return parser.parse_args()


def build_command(args: argparse.Namespace) -> list[str]:
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


def main() -> int:
    args = parse_args()
    cmd = build_command(args)
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

    print(f"smoke ok after {args.seconds:.1f}s", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
