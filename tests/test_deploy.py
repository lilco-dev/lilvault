"""Compose checks; set LILVAULT_DOCKER_TESTS=1 for disposable image smoke tests."""

import json
import os
import select
import shutil
import subprocess
import sys
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TOKEN = "c" * 64  # Fake fixture only.


class ComposeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not shutil.which("docker"):
            raise unittest.SkipTest("Docker Compose CLI unavailable")
        result = subprocess.run(["docker", "compose", "version"], capture_output=True)
        if result.returncode:
            raise unittest.SkipTest("Docker Compose plugin unavailable")

    def render(self, remove=None):
        env = {"PATH": os.environ["PATH"], "HOME": str(Path.home()),
               "LILVAULT_ROOT": "/tmp/lilvault-stack-fixture", "LILVAULT_UID": "3000",
               "LILVAULT_GID": "3000", "LILVAULT_HERMES_TOKEN": TOKEN}
        if remove:
            env.pop(remove)
        return subprocess.run(["docker", "compose", "--file", str(ROOT / "deploy/compose.yaml"),
                               "config", "--format", "json"], env=env, text=True, capture_output=True)

    def test_stack_is_private_nonroot_and_restarts_both_services(self):
        result = self.render()
        self.assertEqual(result.returncode, 0, result.stderr)
        services = json.loads(result.stdout)["services"]
        self.assertEqual(set(services), {"gateway", "sync"})
        for service in services.values():
            self.assertNotIn("build", service)
            self.assertEqual(service["restart"], "unless-stopped")
            self.assertEqual(service["user"], "3000:3000")
            self.assertTrue(service["read_only"])
            self.assertTrue(service["init"])
            self.assertEqual(service["cap_drop"], ["ALL"])
            self.assertIn("no-new-privileges:true", service["security_opt"])
            self.assertNotIn("privileged", service)
        port = services["gateway"]["ports"][0]
        self.assertEqual(port["host_ip"], "127.0.0.1")
        self.assertEqual(port["target"], 8737)
        self.assertNotIn("ports", services["sync"])

    def test_auth_storage_is_not_mounted_in_gateway(self):
        result = self.render()
        self.assertEqual(result.returncode, 0, result.stderr)
        services = json.loads(result.stdout)["services"]
        gateway = services["gateway"]["volumes"]
        self.assertEqual({v["target"] for v in gateway}, {"/etc/lilvault/config.toml", "/data/vaults"})
        config = next(v for v in gateway if v["target"] == "/etc/lilvault/config.toml")
        self.assertTrue(config["read_only"])
        for service in services.values():
            for volume in service["volumes"]:
                self.assertFalse(volume["bind"]["create_host_path"])
                self.assertTrue(volume["source"].startswith("/tmp/lilvault-stack-fixture/"))
                self.assertNotIn("docker.sock", volume["source"])
        self.assertIn("/home/node", {v["target"] for v in services["sync"]["volumes"]})
        self.assertNotIn("environment", services["sync"])

    def test_missing_required_settings_fail_closed(self):
        for setting in ["LILVAULT_ROOT", "LILVAULT_HERMES_TOKEN"]:
            with self.subTest(setting=setting):
                self.assertNotEqual(self.render(remove=setting).returncode, 0)


@unittest.skipUnless(os.environ.get("LILVAULT_DOCKER_TESTS") == "1", "Opt-in real Docker image tests")
class ImageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.docker = []
        for prefix in [["docker"], ["sudo", "-n", "docker"]]:
            result = subprocess.run(prefix + ["info"], capture_output=True, timeout=15)
            if result.returncode == 0:
                cls.docker = prefix
                break
        if not cls.docker:
            raise RuntimeError("Docker image tests requested but daemon access unavailable")
        cls.gateway_image = "lilvault-gateway:0.1.0"
        cls.sync_image = "lilvault-sync:0.0.14-node26.13.0"
        for image in [cls.gateway_image, cls.sync_image]:
            cls.run_docker("image", "inspect", image)

    @classmethod
    def run_docker(cls, *args, stdin=None):
        return subprocess.run(cls.docker + list(args), input=stdin, text=True,
                              capture_output=True, check=True, timeout=45)

    def setUp(self):
        import uuid
        self.prefix = "lilvault-image-test-" + uuid.uuid4().hex[:10]
        self.volumes = []
        self.containers = []
        self.addCleanup(self.cleanup)

    def cleanup(self):
        for name in self.containers:
            subprocess.run(self.docker + ["rm", "--force", name], capture_output=True, timeout=30)
        for name in self.volumes:
            subprocess.run(self.docker + ["volume", "rm", name], capture_output=True, timeout=30)

    def volume(self, suffix):
        name = self.prefix + "-" + suffix
        self.run_docker("volume", "create", "--label", "lilvault.test=true", name)
        self.volumes.append(name)
        return name

    def initialize_volumes(self, mounts, script, stdin=None):
        args = ["run", "--rm", "--network", "none", "--user", "0:0", "--entrypoint", "/bin/sh"]
        if stdin is not None:
            args.append("--interactive")
        for volume, target in mounts:
            args.extend(["--mount", f"type=volume,source={volume},target={target}"])
        self.run_docker(*args, self.gateway_image, "-c", script, stdin=stdin)

    def wait_for_listener(self, name):
        logs = subprocess.Popen(self.docker + ["logs", "--follow", name], stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, bufsize=0)
        try:
            assert logs.stdout is not None
            deadline = time.monotonic() + 20
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not select.select([logs.stdout], [], [], remaining)[0]:
                    self.fail("Gateway did not report readiness")
                line = logs.stdout.readline()
                if not line:
                    self.fail("Gateway exited before readiness")
                if b"lilvault REST listening" in line:
                    return
        finally:
            logs.terminate()
            logs.wait(timeout=10)
            if logs.stdout:
                logs.stdout.close()

    def test_gateway_nondefault_uid_filesystem_mcp_and_health(self):
        data, config = self.volume("data"), self.volume("config")
        text = ('[server]\nlisten="0.0.0.0:8737"\n[[vaults]]\nid="agents"\n'
                'path="/data/vaults/agents"\n[[agents]]\nname="fixture"\n'
                'token_env="LILVAULT_HERMES_TOKEN"\npermissions=["agents:read-write"]\n')
        self.initialize_volumes([(data, "/data"), (config, "/config")],
                                'cat > /config/config.toml && mkdir /data/agents && '
                                'ln -s /etc/passwd /data/agents/escape.md && '
                                'chown -R 3000:3000 /data /config && '
                                'chmod 700 /data /data/agents /config && chmod 600 /config/config.toml', text)
        name = self.prefix + "-gateway"
        self.containers.append(name)
        self.run_docker("run", "--detach", "--name", name, "--init", "--user", "3000:3000",
                        "--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges:true",
                        "--tmpfs", "/tmp:rw,noexec,nosuid,size=16m", "--publish", "127.0.0.1::8737",
                        "--env", "LILVAULT_HERMES_TOKEN=" + TOKEN,
                        "--mount", f"type=volume,source={data},target=/data/vaults",
                        "--mount", f"type=volume,source={config},target=/etc/lilvault,readonly",
                        self.gateway_image)
        self.wait_for_listener(name)
        state = json.loads(self.run_docker("inspect", name).stdout)[0]
        port = state["NetworkSettings"]["Ports"]["8737/tcp"][0]
        self.assertEqual(port["HostIp"], "127.0.0.1")
        health = self.run_docker("exec", name, "curl", "--fail", "--silent", "http://127.0.0.1:8737/readyz")
        self.assertEqual(json.loads(health.stdout)["sync"], "unknown")
        env = {"PATH": os.environ["PATH"], "HOME": str(Path.home()), "LILVAULT_TOKEN": TOKEN,
               "LILVAULT_URL": "http://127.0.0.1:" + port["HostPort"] + "/mcp"}

        def call(tool, args):
            return subprocess.run([sys.executable, str(ROOT / "scripts/lilvault"), "call", tool,
                                   json.dumps(args)], env=env, capture_output=True, text=True, timeout=20)

        write = call("write_file", {"vault_id": "agents", "path": "check.md", "content": "Container check\n", "create_only": True})
        self.assertEqual(write.returncode, 0, write.stderr + write.stdout)
        revision = json.loads(write.stdout)["revision"]
        read = call("read_file", {"vault_id": "agents", "path": "check.md"})
        self.assertEqual(read.returncode, 0, read.stderr)
        self.assertEqual(json.loads(read.stdout)["revision"], revision)
        self.assertEqual(json.loads(read.stdout)["content"], "Container check\n")
        duplicate = call("write_file", {"vault_id": "agents", "path": "check.md", "content": "No", "create_only": True})
        self.assertNotEqual(duplicate.returncode, 0)
        escaped = call("read_file", {"vault_id": "agents", "path": "escape.md"})
        self.assertNotEqual(escaped.returncode, 0)
        self.assertNotIn("root:", escaped.stdout)

    def test_headless_nondefault_uid_private_home_and_readonly_image(self):
        auth = self.volume("auth")
        self.initialize_volumes([(auth, "/home/node")], "chown -R 3000:3000 /home/node && chmod 700 /home/node")
        result = self.run_docker("run", "--rm", "--user", "3000:3000", "--network", "none",
                                 "--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges:true",
                                 "--tmpfs", "/tmp:rw,noexec,nosuid,size=64m",
                                 "--mount", f"type=volume,source={auth},target=/home/node",
                                 self.sync_image, "sync-list-local", "--json")
        self.assertEqual(json.loads(result.stdout)["vaults"], [])


if __name__ == "__main__":
    unittest.main()
