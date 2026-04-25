#!/usr/bin/env python3

import argparse
import json
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path


class OutputBuffer:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._chunks: list[str] = []

    def append(self, text: str) -> None:
        with self._lock:
            self._chunks.append(text)

    def snapshot(self) -> str:
        with self._lock:
            return "".join(self._chunks)

    def wait_for(self, needle: str, timeout: float, start_len: int = 0) -> bool:
        deadline = time.time() + timeout
        while time.time() < deadline:
            if needle in self.snapshot()[start_len:]:
                return True
            time.sleep(0.05)
        return needle in self.snapshot()[start_len:]


class QmpClient:
    def __init__(self, sock: socket.socket) -> None:
        self.sock = sock
        self.file = sock.makefile("rwb", buffering=0)

    @classmethod
    def connect(cls, path: Path, timeout: float) -> "QmpClient":
        deadline = time.time() + timeout
        last_error: Exception | None = None
        while time.time() < deadline:
            try:
                sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                sock.connect(str(path))
                client = cls(sock)
                client._read_message(timeout=2.0)  # greeting
                client.execute("qmp_capabilities")
                return client
            except OSError as err:
                last_error = err
                time.sleep(0.05)
        raise RuntimeError(f"failed to connect to QMP socket {path}: {last_error}")

    def close(self) -> None:
        try:
            self.file.close()
        finally:
            self.sock.close()

    def _read_message(self, timeout: float) -> dict:
        self.sock.settimeout(timeout)
        line = self.file.readline()
        if not line:
            raise RuntimeError("QMP closed while waiting for message")
        return json.loads(line.decode("utf-8"))

    def execute(self, command: str, arguments: dict | None = None, timeout: float = 2.0) -> dict:
        payload = {"execute": command}
        if arguments:
            payload["arguments"] = arguments
        self.file.write(json.dumps(payload).encode("utf-8") + b"\r\n")
        self.file.flush()

        deadline = time.time() + timeout
        while time.time() < deadline:
            msg = self._read_message(max(0.1, deadline - time.time()))
            if "return" in msg:
                return msg["return"]
            if "error" in msg:
                raise RuntimeError(f"QMP {command} failed: {msg['error']}")
        raise RuntimeError(f"timed out waiting for QMP reply to {command}")

    def wait_for_event(self, event_name: str, timeout: float = 5.0) -> dict:
        deadline = time.time() + timeout
        while time.time() < deadline:
            msg = self._read_message(max(0.1, deadline - time.time()))
            if msg.get("event") == event_name:
                return msg
        raise RuntimeError(f"timed out waiting for QMP event {event_name}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run a QMP-driven xHCI hotplug smoke test.")
    parser.add_argument("--bios", required=True, help="Path to bios.img")
    parser.add_argument("--fsimg", required=True, help="Path to fs.img")
    parser.add_argument("--boot-timeout", type=float, default=12.0, help="Seconds to wait for the initial boot")
    parser.add_argument("--hotplug-timeout", type=float, default=6.0, help="Seconds to wait for each hotplug step")
    return parser.parse_args()


def start_reader(proc: subprocess.Popen[str], output: OutputBuffer) -> threading.Thread:
    def pump() -> None:
        assert proc.stdout is not None
        for line in proc.stdout:
            sys.stdout.write(line)
            sys.stdout.flush()
            output.append(line)

    thread = threading.Thread(target=pump, daemon=True)
    thread.start()
    return thread


def build_command(args: argparse.Namespace, qmp_path: Path) -> list[str]:
    return [
        "qemu-system-x86_64",
        "-drive",
        f"format=raw,file={args.bios},snapshot=on",
        "-drive",
        f"file={args.fsimg},if=ide,format=raw,index=1,snapshot=on",
        "-m",
        "512M",
        "-vga",
        "std",
        "-display",
        "none",
        "-debugcon",
        "stdio",
        "-qmp",
        f"unix:{qmp_path},server=on,wait=off",
        "-device",
        "qemu-xhci,id=xhci",
    ]


def terminate_qemu(proc: subprocess.Popen[str], reader: threading.Thread) -> int:
    try:
        proc.terminate()
        try:
            return proc.wait(timeout=2.0)
        except subprocess.TimeoutExpired:
            proc.kill()
            return proc.wait(timeout=2.0)
    finally:
        reader.join(timeout=1.0)


def hotplug_device(
    qmp: QmpClient,
    output: OutputBuffer,
    args: argparse.Namespace,
    *,
    driver: str,
    device_id: str,
    add_message: str,
    enum_message: str,
    fallback_message: str,
) -> None:
    before_add = len(output.snapshot())
    qmp.execute(
        "device_add",
        {"driver": driver, "bus": "xhci.0", "id": device_id},
    )

    if not output.wait_for("[xhci] runtime event: port ", args.hotplug_timeout, before_add):
        raise RuntimeError(f"{add_message} did not trigger a runtime port-change event")
    if not output.wait_for(enum_message, args.hotplug_timeout, before_add):
        raise RuntimeError(f"{add_message} did not trigger fresh HID enumeration")
    if not output.wait_for(fallback_message, args.hotplug_timeout, before_add):
        raise RuntimeError(f"{add_message} did not flip the expected PS/2 fallback state")


def unplug_device(
    qmp: QmpClient,
    output: OutputBuffer,
    args: argparse.Namespace,
    *,
    device_id: str,
    remove_message: str,
    fallback_message: str,
) -> None:
    before_del = len(output.snapshot())
    qmp.execute("device_del", {"id": device_id})
    qmp.wait_for_event("DEVICE_DELETED", timeout=args.hotplug_timeout)

    if not output.wait_for("[xhci] runtime event: port ", args.hotplug_timeout, before_del):
        raise RuntimeError(f"{remove_message} did not trigger a runtime port-change event")
    if not output.wait_for(fallback_message, args.hotplug_timeout, before_del):
        raise RuntimeError(f"{remove_message} did not restore the expected PS/2 fallback state")


def main() -> int:
    args = parse_args()

    with tempfile.TemporaryDirectory(prefix="coolos-qmp-") as tmpdir:
        qmp_path = Path(tmpdir) / "qmp.sock"
        cmd = build_command(args, qmp_path)
        print("+ " + " ".join(str(part) for part in cmd), flush=True)

        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
        )
        output = OutputBuffer()
        reader = start_reader(proc, output)

        qmp: QmpClient | None = None
        try:
            qmp = QmpClient.connect(qmp_path, timeout=3.0)

            if not output.wait_for("[xhci] active init ready", args.boot_timeout):
                raise RuntimeError("xHCI controller did not reach active init ready")
            if not output.wait_for(
                "[input] no USB keyboard detected; enabling PS/2 keyboard fallback",
                args.boot_timeout,
            ):
                raise RuntimeError("kernel did not reach the no-USB-keyboard fallback path")
            if not output.wait_for(
                "[input] no USB mouse detected; enabling PS/2 mouse fallback",
                args.boot_timeout,
            ):
                raise RuntimeError("kernel did not reach the no-USB-mouse fallback path")

            hotplug_device(
                qmp,
                output,
                args,
                driver="usb-kbd",
                device_id="hotkbd",
                add_message="keyboard attach",
                enum_message="hid keyboard iface=",
                fallback_message="[input] USB keyboard detected; PS/2 keyboard fallback disabled",
            )

            hotplug_device(
                qmp,
                output,
                args,
                driver="usb-mouse",
                device_id="hotmouse",
                add_message="mouse attach",
                enum_message="hid mouse iface=",
                fallback_message="[input] USB mouse detected; PS/2 mouse fallback disabled",
            )

            unplug_device(
                qmp,
                output,
                args,
                device_id="hotkbd",
                remove_message="keyboard detach",
                fallback_message="[input] no USB keyboard detected; enabling PS/2 keyboard fallback",
            )

            unplug_device(
                qmp,
                output,
                args,
                device_id="hotmouse",
                remove_message="mouse detach",
                fallback_message="[input] no USB mouse detected; enabling PS/2 mouse fallback",
            )

            rc = terminate_qemu(proc, reader)
            if rc not in (0, -15):
                raise RuntimeError(f"qemu exited with unexpected status {rc}")
        except Exception as err:
            if qmp is not None:
                qmp.close()
            rc = terminate_qemu(proc, reader)
            if rc not in (0, -15, -9):
                print(f"qemu exited with status {rc}", file=sys.stderr)
            print(str(err), file=sys.stderr)
            return 1
        finally:
            if qmp is not None:
                qmp.close()

    print("hotplug smoke ok", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
