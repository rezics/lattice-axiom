#!/usr/bin/env python3
"""Run the development client beside a pinned local Logdy Web viewer."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import platform as host_platform
import re
import signal
import socket
import stat
import subprocess
import sys
import time
from typing import Any, NoReturn, Sequence
from urllib.request import Request, urlopen
import webbrowser


MANIFEST_NAME = "logdy-v0.17.1.json"
RELEASE_BASE_URL = "https://github.com/logdyhq/logdy-core/releases/download"
DEFAULT_UI_PORT = 8080
DEFAULT_INPUT_PORT = 18081
STARTUP_TIMEOUT_SECONDS = 10.0
SHUTDOWN_TIMEOUT_SECONDS = 5.0


class ObservabilityError(RuntimeError):
    """A bounded local observability setup or lifecycle failure."""


@dataclass(frozen=True)
class Artifact:
    """One exact upstream binary selected by host platform and architecture."""

    platform: str
    architecture: str
    file_name: str
    size: int
    sha256: str


@dataclass(frozen=True)
class Manifest:
    """Validated Logdy release evidence consumed by the installer."""

    version: str
    source: str
    source_revision: str
    license: str
    artifacts: tuple[Artifact, ...]


def workspace_root() -> Path:
    """Return the repository root derived from this checked-in script."""

    return Path(__file__).resolve().parents[2]


def _required_string(value: dict[str, Any], key: str) -> str:
    candidate = value.get(key)
    if not isinstance(candidate, str) or not candidate:
        raise ObservabilityError(f"manifest field {key!r} must be a non-empty string")
    return candidate


def load_manifest(path: Path | None = None) -> Manifest:
    """Load and validate the pinned external-tool manifest."""

    manifest_path = path or Path(__file__).with_name(MANIFEST_NAME)
    try:
        document = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ObservabilityError(f"cannot read Logdy manifest: {error}") from error
    if (
        not isinstance(document, dict)
        or type(document.get("schema_version")) is not int
        or document.get("schema_version") != 1
    ):
        raise ObservabilityError("Logdy manifest must use schema_version 1")

    version = _required_string(document, "version")
    source = _required_string(document, "source")
    source_revision = _required_string(document, "source_revision")
    license_name = _required_string(document, "license")
    if not re.fullmatch(r"[0-9a-f]{40}", source_revision):
        raise ObservabilityError("Logdy source_revision must be a full Git commit hash")
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ObservabilityError("Logdy version must be a stable semantic version")
    if source != "https://github.com/logdyhq/logdy-core":
        raise ObservabilityError("Logdy source must remain the reviewed upstream repository")
    if license_name != "Apache-2.0":
        raise ObservabilityError("Logdy license must remain the reviewed Apache-2.0 license")

    raw_artifacts = document.get("artifacts")
    if not isinstance(raw_artifacts, list) or not raw_artifacts:
        raise ObservabilityError("Logdy manifest must declare at least one artifact")
    artifacts: list[Artifact] = []
    identities: set[tuple[str, str]] = set()
    for raw in raw_artifacts:
        if not isinstance(raw, dict):
            raise ObservabilityError("each Logdy artifact must be an object")
        artifact = Artifact(
            platform=_required_string(raw, "platform"),
            architecture=_required_string(raw, "architecture"),
            file_name=_required_string(raw, "file_name"),
            size=raw.get("size"),
            sha256=_required_string(raw, "sha256"),
        )
        if type(artifact.size) is not int or artifact.size <= 0:
            raise ObservabilityError("Logdy artifact size must be a positive integer")
        if not re.fullmatch(r"[0-9a-f]{64}", artifact.sha256):
            raise ObservabilityError("Logdy artifact sha256 must be lowercase hexadecimal")
        if Path(artifact.file_name).name != artifact.file_name:
            raise ObservabilityError("Logdy artifact file_name must not contain a path")
        identity = (artifact.platform, artifact.architecture)
        if identity in identities:
            raise ObservabilityError(f"duplicate Logdy artifact identity: {identity!r}")
        identities.add(identity)
        artifacts.append(artifact)

    return Manifest(
        version=version,
        source=source,
        source_revision=source_revision,
        license=license_name,
        artifacts=tuple(artifacts),
    )


def normalized_host(
    system: str | None = None, machine: str | None = None
) -> tuple[str, str]:
    """Normalize supported Python platform labels to release identities."""

    raw_system = (system or sys.platform).lower()
    if raw_system.startswith("win"):
        platform_name = "windows"
    elif raw_system.startswith("linux"):
        platform_name = "linux"
    elif raw_system.startswith("darwin"):
        platform_name = "darwin"
    else:
        raise ObservabilityError(f"Logdy is not pinned for platform {raw_system!r}")

    raw_machine = (machine or host_platform.machine()).lower()
    if raw_machine in {"amd64", "x86_64"}:
        architecture = "x86_64"
    elif raw_machine in {"aarch64", "arm64"}:
        architecture = "aarch64"
    else:
        raise ObservabilityError(f"Logdy is not pinned for architecture {raw_machine!r}")
    return platform_name, architecture


def select_artifact(manifest: Manifest, identity: tuple[str, str] | None = None) -> Artifact:
    """Select exactly one artifact for a proved host identity."""

    selected_identity = identity or normalized_host()
    candidates = [
        artifact
        for artifact in manifest.artifacts
        if (artifact.platform, artifact.architecture) == selected_identity
    ]
    if len(candidates) != 1:
        raise ObservabilityError(
            f"Logdy manifest has {len(candidates)} artifacts for {selected_identity!r}"
        )
    return candidates[0]


def sha256_file(path: Path) -> str:
    """Hash a file without loading the external binary into memory."""

    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def artifact_matches(path: Path, artifact: Artifact) -> bool:
    """Return whether a cached artifact has the reviewed size and digest."""

    try:
        return path.stat().st_size == artifact.size and sha256_file(path) == artifact.sha256
    except OSError:
        return False


def ensure_logdy(root: Path, manifest: Manifest | None = None) -> Path:
    """Download, verify, and cache the exact Logdy binary for this host."""

    selected_manifest = manifest or load_manifest()
    artifact = select_artifact(selected_manifest)
    tool_directory = root / "run" / "tools" / "logdy" / selected_manifest.version
    executable_name = "logdy.exe" if artifact.platform == "windows" else "logdy"
    executable = tool_directory / executable_name
    if artifact_matches(executable, artifact):
        return executable

    tool_directory.mkdir(parents=True, exist_ok=True)
    temporary = executable.with_name(f"{executable.name}.download")
    temporary.unlink(missing_ok=True)
    url = f"{RELEASE_BASE_URL}/v{selected_manifest.version}/{artifact.file_name}"
    request = Request(url, headers={"User-Agent": "latticeaxiom-dev-observability/1"})
    print(
        "event=logdy_download_started component=dev-observability "
        f"version={selected_manifest.version}",
        file=sys.stderr,
        flush=True,
    )
    try:
        with urlopen(request, timeout=120) as response, temporary.open("wb") as output:
            while chunk := response.read(1024 * 1024):
                output.write(chunk)
        if not artifact_matches(temporary, artifact):
            raise ObservabilityError(
                "downloaded Logdy artifact does not match its pinned size and SHA-256"
            )
        os.replace(temporary, executable)
    except Exception:
        temporary.unlink(missing_ok=True)
        raise

    if artifact.platform != "windows":
        executable.chmod(
            executable.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
        )
    print(
        "event=logdy_download_completed component=dev-observability "
        f"version={selected_manifest.version}",
        file=sys.stderr,
        flush=True,
    )
    return executable


def bounded_port(value: str) -> int:
    """Parse one non-privileged TCP port from CLI or environment input."""

    try:
        port = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("port must be an integer") from error
    if not 1024 <= port <= 65535:
        raise argparse.ArgumentTypeError("port must be in 1024..=65535")
    return port


def assert_port_available(port: int, label: str) -> None:
    """Fail before launch when a requested localhost port is already occupied."""

    probe = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        probe.bind(("127.0.0.1", port))
    except OSError as error:
        raise ObservabilityError(
            f"{label} port {port} on 127.0.0.1 is unavailable: {error}"
        ) from error
    finally:
        probe.close()


def _process_group_options() -> dict[str, Any]:
    if os.name == "nt":
        return {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}
    return {"start_new_session": True}


def wait_for_socket(process: subprocess.Popen[bytes], port: int) -> socket.socket:
    """Wait for Logdy's localhost input socket or a bounded startup failure."""

    deadline = time.monotonic() + STARTUP_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        return_code = process.poll()
        if return_code is not None:
            raise ObservabilityError(f"Logdy exited during startup with code {return_code}")
        try:
            return socket.create_connection(("127.0.0.1", port), timeout=0.25)
        except OSError:
            time.sleep(0.05)
    raise ObservabilityError(
        f"Logdy did not open its input socket on 127.0.0.1:{port} within "
        f"{STARTUP_TIMEOUT_SECONDS:.0f} seconds"
    )


def _wait_then_kill(process: subprocess.Popen[Any]) -> None:
    try:
        process.wait(timeout=SHUTDOWN_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def interrupt_client(process: subprocess.Popen[bytes]) -> None:
    """Forward an interactive interrupt to the complete client process group."""

    if process.poll() is not None:
        return
    try:
        if os.name == "nt":
            process.send_signal(signal.CTRL_BREAK_EVENT)
        else:
            os.killpg(process.pid, signal.SIGINT)
    except OSError:
        process.terminate()
    _wait_then_kill(process)


def stop_logdy(process: subprocess.Popen[bytes]) -> None:
    """Stop the exact sidecar process started by this invocation."""

    if process.poll() is None:
        process.terminate()
        _wait_then_kill(process)


def normalized_exit_code(return_code: int) -> int:
    """Translate Unix signal return codes to conventional shell exit codes."""

    return return_code if return_code >= 0 else min(255, 128 - return_code)


def prune_rotated_logs(log_directory: Path, keep: int = 5) -> None:
    """Keep a bounded number of old Logdy JSONL files between development runs."""

    candidates = sorted(
        (
            path
            for path in log_directory.glob("dev*.jsonl")
            if path.name != "dev.jsonl" and path.is_file()
        ),
        key=lambda path: path.stat().st_mtime_ns,
        reverse=True,
    )
    for stale in candidates[keep:]:
        stale.unlink(missing_ok=True)


def run_observed(
    root: Path,
    command: Sequence[str],
    ui_port: int,
    input_port: int,
) -> int:
    """Run a command, tee its combined output to Logdy, and preserve its exit code."""

    if not command:
        raise ObservabilityError("observed development run requires a child command")
    if ui_port == input_port:
        raise ObservabilityError("Logdy UI and input ports must be different")
    assert_port_available(ui_port, "Logdy UI")
    assert_port_available(input_port, "Logdy input")

    executable = ensure_logdy(root)
    log_directory = root / "run" / "logs"
    log_directory.mkdir(parents=True, exist_ok=True)
    prune_rotated_logs(log_directory)
    persisted_log = log_directory / "dev.jsonl"
    logdy_command = [
        str(executable),
        "socket",
        str(input_port),
        "--ip=127.0.0.1",
        "--ui-ip=127.0.0.1",
        f"--port={ui_port}",
        "--no-analytics",
        "--no-updates",
        "--max-message-count=10000",
        f"--append-to-file={persisted_log}",
        "--rotate-file-size=100M",
    ]
    logdy = subprocess.Popen(logdy_command, cwd=root, **_process_group_options())
    stream: socket.socket | None = None
    client: subprocess.Popen[bytes] | None = None
    try:
        stream = wait_for_socket(logdy, input_port)
        print(
            f"event=logdy_ready component=dev-observability url=http://127.0.0.1:{ui_port}",
            file=sys.stderr,
            flush=True,
        )
        client = subprocess.Popen(
            list(command),
            cwd=root,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            **_process_group_options(),
        )
        if client.stdout is None:
            raise ObservabilityError("client stdout pipe was not created")
        logdy_connected = True
        for line in iter(client.stdout.readline, b""):
            sys.stdout.buffer.write(line)
            sys.stdout.buffer.flush()
            if logdy_connected:
                try:
                    stream.sendall(line)
                except OSError as error:
                    logdy_connected = False
                    print(
                        "event=logdy_stream_failed component=dev-observability "
                        f"error={error}",
                        file=sys.stderr,
                        flush=True,
                    )
        return normalized_exit_code(client.wait())
    except KeyboardInterrupt:
        if client is not None:
            interrupt_client(client)
        return 130
    finally:
        if stream is not None:
            stream.close()
        if client is not None and client.poll() is None:
            interrupt_client(client)
        stop_logdy(logdy)


def argument_parser() -> argparse.ArgumentParser:
    """Build the developer-tool CLI contract."""

    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="action", required=True)
    subparsers.add_parser("install", help="install and verify the pinned Logdy binary")

    run_parser = subparsers.add_parser("run", help="run a command with the Logdy Web viewer")
    run_parser.add_argument(
        "--ui-port",
        type=bounded_port,
        default=os.environ.get("LATTICEAXIOM_LOG_UI_PORT", str(DEFAULT_UI_PORT)),
    )
    run_parser.add_argument(
        "--input-port",
        type=bounded_port,
        default=os.environ.get("LATTICEAXIOM_LOG_INPUT_PORT", str(DEFAULT_INPUT_PORT)),
    )
    run_parser.add_argument("command", nargs=argparse.REMAINDER)

    open_parser = subparsers.add_parser("open", help="open the local Logdy UI")
    open_parser.add_argument(
        "--ui-port",
        type=bounded_port,
        default=os.environ.get("LATTICEAXIOM_LOG_UI_PORT", str(DEFAULT_UI_PORT)),
    )
    return parser


def fail(error: Exception) -> NoReturn:
    """Exit with one searchable developer-observability failure event."""

    print(
        f"event=dev_observability_failed component=dev-observability error={error}",
        file=sys.stderr,
        flush=True,
    )
    raise SystemExit(2)


def main(arguments: Sequence[str] | None = None) -> int:
    """Execute the selected observability developer action."""

    if sys.version_info < (3, 12):
        raise SystemExit("logdy_dev.py requires Python 3.12 or newer")
    options = argument_parser().parse_args(arguments)
    root = workspace_root()
    try:
        if options.action == "install":
            executable = ensure_logdy(root)
            print(executable)
            return 0
        if options.action == "open":
            url = f"http://127.0.0.1:{options.ui_port}"
            if not webbrowser.open(url):
                raise ObservabilityError(f"could not open the default browser for {url}")
            return 0
        if options.action == "run":
            command = options.command
            if command and command[0] == "--":
                command = command[1:]
            return run_observed(root, command, options.ui_port, options.input_port)
        raise ObservabilityError(f"unsupported action: {options.action}")
    except (ObservabilityError, OSError) as error:
        fail(error)


if __name__ == "__main__":
    raise SystemExit(main())
