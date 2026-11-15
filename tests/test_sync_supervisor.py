"""Multi-vault supervisor tests with a fake official CLI; no network or credentials."""

import json
import os
import signal
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class SupervisorTests(unittest.TestCase):
    def test_starts_one_process_per_registered_vault_and_writes_markers(self):
        with tempfile.TemporaryDirectory(prefix="lilvault-supervisor-") as temporary:
            root = Path(temporary)
            registry = root / "registry"
            log = root / "started.log"
            vaults_json = root / "vaults.json"
            vaults_json.write_text(json.dumps({"vaults": [
                {"path": "/vaults/agents"}, {"path": "/vaults/brain"},
                {"path": "/outside/ignored"},
            ]}))
            fake = root / "ob"
            fake.write_text(
                "#!/bin/sh\n"
                "case \"$1\" in\n"
                " sync-list-local) cat \"$VAULTS_JSON\" ;;\n"
                " sync) printf '%s\\n' \"$3\" >> \"$START_LOG\"; "
                "trap 'exit 0' TERM INT; while :; do sleep 1; done ;;\n"
                " *) exit 2 ;;\n"
                "esac\n"
            )
            fake.chmod(0o700)
            env = {**os.environ, "LILVAULT_VAULT_REGISTRY": str(registry),
                   "START_LOG": str(log), "VAULTS_JSON": str(vaults_json)}
            process = subprocess.Popen(
                ["node", str(ROOT / "deploy/sync-supervisor.mjs"), str(fake)],
                env=env,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
                text=True,
            )
            try:
                deadline = time.monotonic() + 8
                while time.monotonic() < deadline:
                    if log.exists() and len(log.read_text().splitlines()) >= 2:
                        break
                    time.sleep(0.1)
                self.assertTrue(log.exists(), "supervisor did not launch sync children")
                self.assertEqual(set(log.read_text().splitlines()),
                                 {"/vaults/agents", "/vaults/brain"})
                self.assertEqual({path.name for path in registry.iterdir()},
                                 {"agents", "brain"})
                vaults_json.write_text(json.dumps({"vaults": [{"path": "/vaults/agents"}]}))
                deadline = time.monotonic() + 8
                while time.monotonic() < deadline and (registry / "brain").exists():
                    time.sleep(0.1)
                self.assertFalse((registry / "brain").exists())
                self.assertTrue(process.poll() is None)
            finally:
                process.send_signal(signal.SIGTERM)
                process.wait(timeout=12)
                if process.stderr:
                    process.stderr.close()
            self.assertEqual(process.returncode, 0)


if __name__ == "__main__":
    unittest.main()
