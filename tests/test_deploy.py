"""Compose checks; opt in to disposable container tests with LILVAULT_DOCKER_TESTS=1."""

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
        if subprocess.run(["docker", "compose", "version"], capture_output=True).returncode:
            raise unittest.SkipTest("Docker Compose plugin unavailable")

    def render(self, remove=None, **overrides):
        env = {"PATH": os.environ["PATH"], "HOME": str(Path.home()),
               "LILVAULT_ROOT": "/test/lilvault-fixture"}
        env.update(overrides)
        if remove:
            env.pop(remove)
        return subprocess.run(["docker", "compose", "--file", str(ROOT / "deploy/compose.yaml"),
                               "config", "--format", "json"], env=env, text=True, capture_output=True)

    def test_stack_uses_public_images_and_nonroot_app_services(self):
        result = self.render()
        self.assertEqual(result.returncode, 0, result.stderr)
        services = json.loads(result.stdout)["services"]
        self.assertEqual(set(services), {"initialize", "gateway", "sync"})
        for name, service in services.items():
            self.assertNotIn("build", service)
            self.assertTrue(service["image"].startswith("ghcr.io/lilco-dev/lilvault"))
            self.assertTrue(service["image"].endswith(":latest"))
            self.assertEqual(service["pull_policy"], "always")
            self.assertTrue(service["read_only"])
            self.assertEqual(service["cap_drop"], ["ALL"])
            self.assertIn("no-new-privileges:true", service["security_opt"])
            self.assertNotIn("privileged", service)
            if name != "initialize":
                self.assertEqual(service["restart"], "unless-stopped")
                self.assertEqual(service["user"], "568:568")
                self.assertTrue(service["init"])
                self.assertEqual(service["depends_on"]["initialize"]["condition"], "service_completed_successfully")
        init = services["initialize"]
        self.assertEqual(init["user"], "0:0")
        self.assertEqual(init["restart"], "no")
        self.assertEqual(init["network_mode"], "none")
        self.assertEqual(set(init["cap_add"]), {"CHOWN", "FOWNER", "DAC_OVERRIDE"})
        self.assertNotIn("LILVAULT_HERMES_TOKEN", init["environment"])
        port = services["gateway"]["ports"][0]
        self.assertEqual(port["published"], "8737")
        self.assertEqual(port["target"], 8737)
        self.assertNotIn("host_ip", port)
        self.assertNotIn("ports", services["sync"])

    def test_auth_and_runtime_cache_are_not_mounted_in_gateway(self):
        services = json.loads(self.render().stdout)["services"]
        gateway = services["gateway"]
        self.assertEqual({v["target"] for v in gateway["volumes"]},
                         {"/etc/lilvault", "/agents", "/data/vaults"})
        self.assertEqual(set(gateway["environment"]), {"RUST_LOG"})
        self.assertTrue(next(v for v in gateway["volumes"] if v["target"] == "/etc/lilvault")["read_only"])
        self.assertFalse(next(v for v in gateway["volumes"] if v["target"] == "/agents").get("read_only", False))
        for service in services.values():
            for volume in service["volumes"]:
                self.assertNotIn("docker.sock", volume["source"])
                if volume["type"] == "bind":
                    self.assertTrue(volume["source"].startswith("/test/lilvault-fixture"))
                else:
                    self.assertTrue(volume["volume"]["nocopy"])
        self.assertIn("/home/node", {v["target"] for v in services["sync"]["volumes"]})
        self.assertIn("/opt/obsidian", {v["target"] for v in services["sync"]["volumes"]})
        stack = next(v for v in services["initialize"]["volumes"] if v["target"] == "/stack")
        self.assertFalse(stack.get("bind", {}).get("create_host_path", False))

    def test_overrides_apply_without_agent_secrets_in_the_stack(self):
        services = json.loads(self.render(LILVAULT_UID="3000", LILVAULT_GID="3001",
                                         LILVAULT_IMAGE_TAG="0.1.0").stdout)["services"]
        self.assertEqual(services["gateway"]["user"], "3000:3001")
        self.assertTrue(services["sync"]["image"].endswith(":0.1.0"))
        rendered = json.dumps(services)
        self.assertNotIn("TOKEN", rendered)
        self.assertNotIn("token_env", rendered)

    def test_missing_required_settings_fail_closed(self):
        self.assertNotEqual(self.render(remove="LILVAULT_ROOT").returncode, 0)


@unittest.skipUnless(os.environ.get("LILVAULT_DOCKER_TESTS") == "1", "Opt-in real Docker image tests")
class ImageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.docker = []
        for prefix in [["docker"], ["sudo", "-n", "docker"]]:
            if subprocess.run(prefix + ["info"], capture_output=True, timeout=15).returncode == 0:
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
        return subprocess.run(cls.docker + list(args), input=stdin, text=True, capture_output=True, check=True, timeout=240)

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

    def initialize(self, stack, runtime, **overrides):
        env = {"LILVAULT_UID": "3000", "LILVAULT_GID": "3000"}
        env.update(overrides)
        args = ["run", "--rm", "--network", "none", "--user", "0:0", "--read-only",
                "--cap-drop", "ALL", "--cap-add", "CHOWN", "--cap-add", "FOWNER",
                "--cap-add", "DAC_OVERRIDE", "--security-opt", "no-new-privileges:true",
                "--entrypoint", "/usr/local/bin/lilvault-init",
                "--mount", f"type=volume,source={stack},target=/stack,volume-nocopy",
                "--mount", f"type=volume,source={runtime},target=/runtime,volume-nocopy"]
        for key, value in env.items():
            args.extend(["--env", key + "=" + value])
        return self.run_docker(*args, self.gateway_image)

    def fixture_shell(self, stack, script):
        return self.run_docker("run", "--rm", "--network", "none", "--entrypoint", "/bin/sh", "--user", "0:0",
                               "--mount", f"type=volume,source={stack},target=/stack", self.gateway_image, "-c", script)

    def wait_for_log(self, name, expected):
        logs = subprocess.Popen(self.docker + ["logs", "--follow", name], stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, bufsize=0)
        try:
            assert logs.stdout is not None
            deadline = time.monotonic() + 20
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not select.select([logs.stdout], [], [], remaining)[0]:
                    self.fail("Container did not report expected readiness")
                line = logs.stdout.readline()
                if not line:
                    self.fail("Container exited before readiness")
                if expected in line:
                    return
        finally:
            logs.terminate()
            logs.wait(timeout=10)
            if logs.stdout:
                logs.stdout.close()

    def test_build_context_allowlist_excludes_private_files_inside_source_directories(self):
        import tempfile
        with tempfile.TemporaryDirectory(prefix="lilvault-context-") as temporary:
            context = Path(temporary)
            shutil.copyfile(ROOT / ".dockerignore", context / ".dockerignore")
            for name in ["Cargo.toml", "Cargo.lock", "src/main.rs", "src/vault.rs",
                         "deploy/initialize.sh", "deploy/add-agent.sh", "deploy/sync-entrypoint.sh",
                         "deploy/.env", "src/private.json", "vaults/note.md"]:
                path = context / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("fixture-not-a-secret\n")
            dockerfile = context / "Dockerfile"
            dockerfile.write_text("FROM " + self.gateway_image + "\nUSER 0:0\nCOPY . /probe/\n"
                                  "RUN test -f /probe/src/main.rs && test -f /probe/src/vault.rs "
                                  "&& test -f /probe/deploy/initialize.sh "
                                  "&& test -f /probe/deploy/add-agent.sh "
                                  "&& test -f /probe/deploy/sync-entrypoint.sh "
                                  "&& test ! -e /probe/deploy/.env && test ! -e /probe/src/private.json "
                                  "&& test ! -e /probe/vaults\n")
            image = self.prefix + "-context"
            try:
                self.run_docker("build", "--file", str(dockerfile), "--tag", image, str(context))
            except subprocess.CalledProcessError as error:
                self.fail(error.stdout + error.stderr)
            finally:
                subprocess.run(self.docker + ["image", "rm", image], capture_output=True, timeout=30)

    def test_initializer_is_idempotent_without_recursing_or_overwriting_custom_config(self):
        stack, runtime = self.volume("stack"), self.volume("runtime")
        self.initialize(stack, runtime)
        self.fixture_shell(stack, 'printf untouched > /stack/config/custom.toml; '
                                 'printf keep > /stack/vaults/agents/keep.md; chmod 600 /stack/vaults/agents/keep.md')
        self.initialize(stack, runtime, LILVAULT_MCP_HOST="gateway.example:9443")
        data = self.fixture_shell(stack, 'stat -c "%u:%g:%a" /stack /stack/agents /stack/config/gateway.generated.toml '
                                        '/stack/vaults/agents/keep.md; '
                                        'test "$(head -c 9 /stack/config/custom.toml)" = untouched; '
                                        'grep -F "gateway.example:9443" /stack/config/gateway.generated.toml; '
                                        'grep -F keys_dir /stack/config/gateway.generated.toml; '
                                        '! grep -F token_env /stack/config/gateway.generated.toml').stdout
        self.assertIn("3000:3000:700", data)
        self.assertIn("3000:3000:600", data)
        self.assertIn("0:0:600", data)
        self.assertIn("keys_dir", data)

    def test_initializer_rejects_unsafe_parameters_and_targets(self):
        stack, runtime = self.volume("stack"), self.volume("runtime")
        for overrides in [{"LILVAULT_UID": "0"}, {"LILVAULT_GID": "0"}, {"LILVAULT_UID": "../bad"},
                          {"LILVAULT_MCP_HOST": 'x\"]\n[[agents]]'}, {"LILVAULT_MCP_HOST": "https://example"}]:
            with self.subTest(overrides=overrides), self.assertRaises(subprocess.CalledProcessError):
                self.initialize(stack, runtime, **overrides)
        self.initialize(stack, runtime)
        self.fixture_shell(stack, 'rm /stack/config/gateway.generated.toml; '
                                 'ln -s /etc/passwd /stack/config/gateway.generated.toml')
        with self.assertRaises(subprocess.CalledProcessError) as error:
            self.initialize(stack, runtime)
        self.assertIn("unsafe generated", error.exception.stderr)

    def test_gateway_after_automatic_setup_has_secure_filesystem_mcp_and_health(self):
        stack, runtime = self.volume("stack"), self.volume("runtime")
        self.initialize(stack, runtime)
        self.fixture_shell(stack, "ln -s /etc/passwd /stack/vaults/agents/escape.md; "
                                  f"printf '%s\\n' {TOKEN} > /stack/agents/first.token; "
                                  f"printf '%s\\n' {'d' * 64} > /stack/agents/reader.ro.token; "
                                  "chown 3000:3000 /stack/agents/*.token; chmod 600 /stack/agents/*.token")
        name = self.prefix + "-gateway"
        self.containers.append(name)
        self.run_docker("run", "--detach", "--name", name, "--init", "--user", "3000:3000",
                        "--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges:true",
                        "--tmpfs", "/tmp:rw,noexec,nosuid,size=16m",  # noqa: S108 - container-local memory mount.
                        "--publish", "127.0.0.1::8737",
                        "--mount", f"type=volume,source={stack},volume-subpath=vaults,target=/data/vaults,volume-nocopy",
                        "--mount", f"type=volume,source={stack},volume-subpath=config,target=/etc/lilvault,readonly,volume-nocopy",
                        "--mount", f"type=volume,source={stack},volume-subpath=agents,target=/agents,volume-nocopy",
                        self.gateway_image, "/etc/lilvault/gateway.generated.toml")
        self.wait_for_log(name, b"lilvault REST listening")
        state = json.loads(self.run_docker("inspect", name).stdout)[0]
        port = state["NetworkSettings"]["Ports"]["8737/tcp"][0]
        self.assertEqual(port["HostIp"], "127.0.0.1")
        health = self.run_docker("exec", name, "curl", "--fail", "--silent", "http://127.0.0.1:8737/readyz")
        self.assertEqual(json.loads(health.stdout)["sync"], "unknown")

        def host_port():
            state = json.loads(self.run_docker("inspect", name).stdout)[0]
            return state["NetworkSettings"]["Ports"]["8737/tcp"][0]["HostPort"]

        def call(tool, args, token=TOKEN):
            env = {"PATH": os.environ["PATH"], "HOME": str(Path.home()), "LILVAULT_TOKEN": token,
                   "LILVAULT_URL": "http://127.0.0.1:" + host_port() + "/mcp"}
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

        readonly = call("write_file", {"vault_id": "agents", "path": "ro.md", "content": "x", "create_only": True}, token="d" * 64)
        self.assertNotEqual(readonly.returncode, 0)
        self.assertEqual(call("read_file", {"vault_id": "agents", "path": "check.md"}, token="d" * 64).returncode, 0)

        second = "e" * 64
        added = self.run_docker("exec", "--interactive", name, "lilvault-add-agent", "second", stdin=second + "\n")
        self.assertIn("Restart the gateway", added.stdout)
        refused = subprocess.run(self.docker + ["exec", "--interactive", name, "lilvault-add-agent", "second"],
                                 input=second + "\n", text=True, capture_output=True, timeout=30)
        self.assertNotEqual(refused.returncode, 0)
        short = subprocess.run(self.docker + ["exec", "--interactive", name, "lilvault-add-agent", "third"],
                               input="abc\n", text=True, capture_output=True, timeout=30)
        self.assertNotEqual(short.returncode, 0)
        self.assertEqual(call("list_vaults", {}, token=second).returncode, 1)

        self.run_docker("restart", name)
        deadline = time.monotonic() + 20
        while call("list_vaults", {}, token=second).returncode != 0:
            if time.monotonic() > deadline:
                self.fail("Newly added agent key was not loaded after restart")
            time.sleep(0.5)

    def test_public_runner_installs_outside_image_reuses_cache_offline_and_waits_for_setup(self):
        stack, runtime = self.volume("stack"), self.volume("runtime")
        self.initialize(stack, runtime)
        self.run_docker("run", "--rm", "--network", "none", "--entrypoint", "/bin/sh", self.sync_image,
                        "-c", "test ! -d /usr/local/lib/node_modules/obsidian-headless")
        args = ["--init", "--user", "3000:3000", "--read-only", "--cap-drop", "ALL",
                "--security-opt", "no-new-privileges:true",
                "--tmpfs", "/tmp:rw,noexec,nosuid,size=64m",  # noqa: S108 - container-local memory mount.
                "--mount", f"type=volume,source={stack},volume-subpath=sync-auth,target=/home/node,volume-nocopy",
                "--mount", f"type=volume,source={stack},volume-subpath=vaults/agents,target=/vaults/agents,volume-nocopy",
                "--mount", f"type=volume,source={runtime},target=/opt/obsidian,volume-nocopy"]
        first = self.run_docker("run", "--rm", *args, self.sync_image, "sync-list-local", "--json")
        self.assertEqual(json.loads(first.stdout)["vaults"], [])
        second = self.run_docker("run", "--rm", "--network", "none", *args, self.sync_image, "sync-list-local", "--json")
        self.assertEqual(json.loads(second.stdout)["vaults"], [])
        self.assertNotIn("Installing pinned", second.stderr)
        name = self.prefix + "-sync"
        self.containers.append(name)
        self.run_docker("run", "--detach", "--name", name, "--network", "none", *args, self.sync_image)
        self.wait_for_log(name, b"Waiting for setup.")
        console = self.run_docker("exec", name, "ob", "sync-list-local", "--json")
        self.assertEqual(json.loads(console.stdout)["vaults"], [])
        self.assertTrue(json.loads(self.run_docker("inspect", name).stdout)[0]["State"]["Running"])
        self.run_docker("stop", "--time", "5", name)


if __name__ == "__main__":
    unittest.main()
