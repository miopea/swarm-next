#!/usr/bin/env python3
"""Real API/SQLite recovery drill; systemctl is replaced by an owned-process adapter.

No live database, token, service or terminal host is used. Evidence is retained
in a new private directory beneath the operator's home.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request


def control_client():
    with socket.socket(socket.AF_UNIX) as client:
        client.connect(os.environ["SWARM_DRILL_SOCKET"])
        client.sendall(json.dumps(sys.argv[1:]).encode() + b"\n")
        with client.makefile("rb") as stream:
            result = json.loads(stream.readline())
    print(result["output"], end="")
    return result["code"]


def main():
    release = Path(sys.argv[1]).resolve(strict=True)
    package = Path(sys.argv[2]).resolve(strict=True)
    assert (release / "bin/swarm-api").is_file()
    assert (release / "bin/swarmctl").is_file()
    root = Path(tempfile.mkdtemp(prefix=".swarm-recovery-drill-", dir=Path.home()))
    print(f"Private isolated recovery evidence: {root}", flush=True)
    state = root / "state/swarm"
    install = root / "install/swarm"
    config = root / "config/swarm"
    workspace = root / "workspace"
    for directory in (state, install, config, workspace, root / "units", root / "bin"):
        directory.mkdir(parents=True)
    managed_release = install / "releases/drill"
    (managed_release / "bin").mkdir(parents=True)
    (managed_release / "bin/swarmctl").symlink_to(release / "bin/swarmctl")
    shutil.copyfile(release / "VERSION", managed_release / "VERSION")
    (install / "current").symlink_to(managed_release, target_is_directory=True)
    # No inherited environment or real HOME/configuration reaches this API.
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    assert port not in (8765, 8766)
    base = f"http://127.0.0.1:{port}"
    database = state / "swarm.sqlite3"
    api_env = {
        "PATH": "/usr/bin:/bin", "SWARM_API_BIND": f"127.0.0.1:{port}",
        "SWARM_DATABASE_PATH": str(database), "SWARM_WORKSPACE_ROOTS": str(workspace),
        "XDG_CONFIG_HOME": str(config), "SWARM_OPERATOR_CONFIG_PATH": str(config / "operator.env"),
        "SWARM_AGENT_CONFIG_ROOT": str(root / "agents"),
        "SWARM_TERMINAL_SOCKET": str(root / "no-terminal-host.sock"),
    }
    process = None
    api_log = (root / "api.log").open("ab")

    def start():
        nonlocal process
        assert process is None or process.poll() is not None
        process = subprocess.Popen([str(release / "bin/swarm-api")], cwd=root,
                                   env=api_env, stdout=api_log, stderr=api_log)

    def stop():
        if process is not None and process.poll() is None:
            process.terminate()
            process.wait(timeout=15)

    def request(path, body=None):
        payload = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(base + path, data=payload,
                                     headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=3) as response:
            return json.load(response)

    def ready():
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            assert process is not None and process.poll() is None, "isolated API exited"
            try:
                request("/health")
                return
            except OSError:
                time.sleep(0.1)
        raise TimeoutError("isolated API did not become ready")

    server = socket.socket(socket.AF_UNIX)
    socket_path = root / "ctl.sock"
    server.bind(str(socket_path))
    server.listen(1)
    commands = []

    def serve_control():
        while True:
            connection, _ = server.accept()
            with connection, connection.makefile("rb") as stream:
                arguments = json.loads(stream.readline())
                commands.append(arguments)
                result = {"code": 0, "output": ""}
                try:
                    assert arguments[0] == "--user"
                    assert arguments[2] == "swarm-api.service", "non-API service requested"
                    verb = arguments[1]
                    if verb == "stop":
                        stop()
                    elif verb == "start":
                        start()
                    elif verb == "show":
                        result["output"] = "active\n" if process and process.poll() is None else "inactive\n"
                    else:
                        raise ValueError(verb)
                except Exception as error:
                    result = {"code": 1, "output": str(error)}
                connection.sendall(json.dumps(result).encode() + b"\n")

    threading.Thread(target=serve_control, daemon=True).start()
    shim = root / "systemctl"
    shim.symlink_to(Path(__file__).resolve())
    env = dict(os.environ, SWARM_INSTALL_ROOT=str(install), SWARM_STATE_ROOT=str(state),
               SWARM_CONFIG_ROOT=str(config), SWARM_SYSTEMD_USER_ROOT=str(root / "units"),
               SWARM_BIN_ROOT=str(root / "bin"), SWARM_WORKSPACE_ROOT=str(workspace),
               SWARM_SYSTEMCTL_BIN=str(shim), SWARM_DRILL_SOCKET=str(socket_path),
               SWARM_CURL_BIN="/usr/bin/curl", SWARM_HEALTH_URL=base + "/health",
               SWARM_HEALTH_ATTEMPTS="30")
    try:
        start()
        ready()
        task = request("/api/v1/tasks", {"title": "Isolated recovery marker", "workspace": str(workspace)})
        stop()
        backup = root / "known-good.sqlite3"
        shutil.copyfile(database, backup)
        subprocess.run([str(release / "bin/swarmctl"), "verify-database", str(backup)], check=True, stdout=subprocess.DEVNULL)
        original_hash = hashlib.sha256(backup.read_bytes()).hexdigest()
        # Destructive bytes are confined to this newly created disposable DB.
        assert database.resolve().is_relative_to(root)
        database.write_bytes(b"Deliberately corrupt disposable SQLite database\n")
        corrupt_hash = hashlib.sha256(database.read_bytes()).hexdigest()
        start()
        assert process.wait(timeout=30) != 0, "corrupted API unexpectedly started"
        subprocess.run(["sh", str(package), "restore-offline", str(backup)], env=env, check=True, timeout=90)
        ready()
        assert any(row["id"] == task["id"] and row["title"] == task["title"] for row in request("/api/v1/tasks"))
        assert hashlib.sha256(backup.read_bytes()).hexdigest() == original_hash
        archives = list((state / "backups").glob("offline-restore-*"))
        assert len(archives) == 1
        assert hashlib.sha256((archives[0] / "swarm.sqlite3").read_bytes()).hexdigest() == corrupt_hash
        assert (archives[0] / "original-copy-complete.txt").is_file()
        assert all(arguments[2] == "swarm-api.service" for arguments in commands)
        print(json.dumps({"result": "passed", "scope": "real_api_sqlite_offline_restore",
                          "task_restored": task["id"], "source_unchanged": True,
                          "corruption_preserved": True, "real_systemd_used": False,
                          "live_hive_touched": False}), flush=True)
    finally:
        stop()
        api_log.close()


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--user":
        sys.exit(control_client())
    main()
