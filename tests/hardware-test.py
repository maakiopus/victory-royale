"""Exercise the hardware request with synthetic CI packages and title paths."""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock
import zipfile

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts" / "hardware-test"


def load(name):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


runner = load("run-test")
prepare = load("prepare")


class HardwareRequest(unittest.TestCase):
    def setUp(self):
        work = tempfile.TemporaryDirectory()
        self.addCleanup(work.cleanup)
        self.root = Path(work.name).resolve()
        self.package = self.root / runner.PACKAGE
        self.revision = "a" * 40
        with tarfile.open(self.package, "w:gz") as bundle:
            for name in ("victory.exe", "victory-proton.sh", "LICENSE", "REVISION"):
                data = self.revision.encode() if name == "REVISION" else b"fixture"
                member = tarfile.TarInfo("victory/" + name)
                member.size = len(data)
                bundle.addfile(member, io.BytesIO(data))
        self.packages = self.root / "packages"
        self.packages.mkdir()
        self.build = {"run": 12, "revision": self.revision,
                      "sha256": hashlib.sha256(self.package.read_bytes()).hexdigest()}
        self.request = {"version": 1, "pr": 7, "scenario": "Close the window.",
                        "main": self.build.copy(), "candidate": self.build.copy()}
        self.manifest = self.root / "request.json"
        self.manifest.write_text(json.dumps(self.request))
        self.title = self.root / "space & title.exe"
        self.title.touch()
        self.output = self.root / "out"

    def fake_download(self, args, check):
        self.assertEqual(args[:6], ["gh", "run", "download", "12", "-R", runner.REPO])
        shutil.copy(self.package, Path(args[-1]) / runner.PACKAGE)

    def test_bundled_build_offline(self):
        shutil.copy(self.package, self.packages / (self.build["sha256"] + ".tar.gz"))
        with mock.patch.object(subprocess, "run", side_effect=AssertionError), \
                mock.patch.object(shutil, "which", side_effect=AssertionError):
            runner.stage_build(self.request, "main", self.root / "main", self.packages)

    def test_verify_both_packages(self):
        with mock.patch.object(subprocess, "run", side_effect=self.fake_download) as gh, \
                mock.patch.object(shutil, "which", return_value="gh"):
            for side in ("main", "candidate"):
                runner.stage_build(self.request, side, self.root / side, self.packages)
                self.assertEqual((self.root / side / "REVISION").read_text(), self.revision)
            self.assertIn("pr-7-", gh.call_args.args[0][7])
            self.request["main"]["sha256"] = "0" * 64
            with self.assertRaisesRegex(ValueError, "checksum"):
                runner.stage_build(self.request, "main", self.root / "bad", self.packages)
            self.request["candidate"]["revision"] = "b" * 40
            with self.assertRaisesRegex(ValueError, "different revision"):
                runner.stage_build(self.request, "candidate", self.root / "wrong", self.packages)

    def test_preview_offline(self):
        with mock.patch.object(sys, "argv", ["runner", str(self.manifest), "--plan"]), \
                mock.patch.object(shutil, "which", side_effect=AssertionError), \
                contextlib.redirect_stdout(io.StringIO()) as out:
            runner.run()
        self.assertIn("Close the window.", out.getvalue())
        self.assertIn("actions/runs/12", out.getvalue())

    def test_checkout_menu(self):
        with mock.patch.object(runner, "ROOT", self.root / "checkout"), \
                mock.patch.object(sys, "argv", ["runner"]), \
                mock.patch.object(sys.stdin, "isatty", return_value=True), \
                mock.patch("builtins.input", return_value=""), \
                mock.patch.object(runner, "run_request", side_effect=AssertionError), \
                mock.patch.object(runner, "github_rows") as github, contextlib.redirect_stdout(io.StringIO()) as out:
            github.side_effect = [[{"number": 7, "title": "First scene", "head": "current"}],
                                  [10, 12], [], [{"name": "victory-series-x-pr-9"},
                                                   {"name": "victory-series-x-pr-7"}]]
            runner.run()
            self.assertIn("#7  First scene", out.getvalue())
            self.assertNotIn("#9", out.getvalue())
            self.assertIn("hardware-request.yml/runs?branch=main&event=workflow_dispatch&status=success",
                          github.call_args_list[1].args[0])
            self.assertIn("runs/12/artifacts", github.call_args_list[2].args[0])
            self.assertIn("expired == false", github.call_args_list[2].args[1])
            github.side_effect = [[]]
            runner.run()
            self.assertIn("no published Series X", out.getvalue())

    def test_published_request(self):
        available = {7: ({"title": "First scene", "head": "current"}, 12, "victory-series-x-pr-7")}
        def download(args, check):
            self.assertEqual(args[:-1], ["gh", "run", "download", "12", "-R", runner.REPO,
                                         "-n", "victory-series-x-pr-7", "-D"])
            shutil.copy(self.manifest, Path(args[-1]) / "request.json")
        parents = [{"sha": self.revision}, {"sha": "current"}]
        with mock.patch.object(runner, "ROOT", self.root / "checkout"), \
                mock.patch.object(sys, "argv", ["runner", "7", "--plan"]), \
                mock.patch.object(subprocess, "run", side_effect=download), \
                mock.patch.object(runner, "published_requests", return_value=available), \
                mock.patch.object(runner, "github_rows", return_value=parents), \
                mock.patch.object(runner, "launch", side_effect=AssertionError), \
                contextlib.redirect_stdout(io.StringIO()) as out:
            runner.run()
            self.assertIn("Close the window.", out.getvalue())
            with mock.patch.object(sys, "argv", ["runner", "--plan"]), \
                    mock.patch.object(sys.stdin, "isatty", return_value=True), \
                    mock.patch("builtins.input", return_value="7"):
                runner.run()
            parents[1]["sha"] = "old"
            with self.assertRaisesRegex(ValueError, "changed after"):
                runner.run()
            self.manifest.write_text(json.dumps(dict(self.request, pr=8)))
            with self.assertRaisesRegex(ValueError, "different PR"):
                runner.run()

    def test_checkout_zip_and_extracted_kit_offline(self):
        archive = self.root / "Series X kit.zip"
        name = self.build["sha256"] + ".tar.gz"
        with zipfile.ZipFile(archive, "w") as kit:
            kit.write(self.manifest, "request.json")
            kit.write(self.package, "packages/" + name)
            kit.writestr("../ignored", "unused")
        def check_manifest(manifest, options):
            self.assertEqual(runner.read_request(manifest), self.request)
            runner.stage_build(self.request, "candidate", self.root / "checked", manifest.parent / "packages")
        with mock.patch.object(runner, "ROOT", self.root), \
                mock.patch.object(subprocess, "run", side_effect=AssertionError), \
                mock.patch.object(shutil, "which", side_effect=AssertionError):
            with mock.patch.object(sys, "argv", ["runner", "--plan"]), contextlib.redirect_stdout(io.StringIO()):
                runner.run()
            with mock.patch.object(sys, "argv", ["runner", str(archive)]), \
                    mock.patch.object(runner, "run_request", side_effect=check_manifest):
                runner.run()

    def run_pair(self, launch):
        args = ["runner", str(self.manifest), "--title", str(self.title), "--output-dir", str(self.output)]
        with mock.patch.object(sys, "argv", args), mock.patch.object(shutil, "which", return_value="tool"), \
                mock.patch.object(subprocess, "run", side_effect=self.fake_download), \
                mock.patch.object(runner, "launch", side_effect=launch), contextlib.redirect_stdout(io.StringIO()):
            runner.run()

    def test_instructions_at_launch_and_private_zip_in_temp(self):
        shutil.copy(self.package, self.packages / (self.build["sha256"] + ".tar.gz"))
        temp = self.root / "system temp with spaces"
        temp.mkdir()
        stdout = io.StringIO()

        def launched(side, folder, title, results, windows):
            number = 1 if side == "main" else 2
            lines = stdout.getvalue().rstrip().splitlines()
            self.assertEqual(lines[-3], f"RUN {number} OF 2  |  {side.upper()}  |  WHAT TO DO")
            self.assertEqual(lines[-1], self.request["scenario"])
            (results / (side + ".log")).write_text(side)

        args = ["runner", str(self.manifest), "--title", str(self.title)]
        with mock.patch.object(sys, "argv", args), mock.patch.object(tempfile, "tempdir", str(temp)), \
                mock.patch.dict(os.environ, {"COLUMNS": "52"}), \
                mock.patch.object(shutil, "which", return_value="tool"), \
                mock.patch.object(runner, "launch", side_effect=launched), contextlib.redirect_stdout(stdout):
            runner.run()
        report, = temp.rglob("*.zip")
        self.assertEqual(list(temp.iterdir()), [report.parent])
        self.assertIn("REPORT READY", stdout.getvalue())
        self.assertIn("Send this ZIP privately", stdout.getvalue())
        self.assertEqual(stdout.getvalue().rstrip().splitlines()[-1], f"Upload: {report}")
        with zipfile.ZipFile(report) as archive:
            self.assertEqual(set(archive.namelist()), {"request.json", "main.log", "candidate.log"})

    def test_fresh_pair_and_partial(self):
        def launched(side, folder, title, results, windows):
            self.assertEqual(title, self.title)
            (results / (side + ".log")).write_text(side)

        self.run_pair(launched)
        self.run_pair(launched)
        bundles = list(self.output.glob("*/report.zip"))
        self.assertEqual(len(bundles), 2)
        with zipfile.ZipFile(bundles[0]) as archive:
            self.assertEqual(set(archive.namelist()), {"request.json", "main.log", "candidate.log"})

        def fail(side, *args):
            if side == "candidate":
                raise ValueError("test interruption")
            launched(side, *args)

        with self.assertRaisesRegex(ValueError, "interruption"):
            self.run_pair(fail)
        partial, = set(self.output.glob("*/report.zip")) - set(bundles)
        with zipfile.ZipFile(partial) as archive:
            self.assertEqual(set(archive.namelist()), {"request.json", "main.log"})

    def test_platform_commands_and_cache_paths(self):
        for windows in (False, True):
            with self.subTest(windows=windows):
                results = self.root / ("windows" if windows else "linux")
                results.mkdir()
                report = self.root / "current.tar.gz"
                report.write_bytes(b"this launch")
                # Capture argv and environment, but exercise actual pipe/exit handling.
                popen = subprocess.Popen
                def start(args, **kwargs):
                    self.assertEqual(kwargs["cwd"], self.title.parent)
                    self.assertNotIn("VICTORY_EXE", kwargs["env"])
                    self.assertIn(str(self.title), args)
                    self.assertEqual(args[-2:], ["--console", "series-x"])
                    if windows:
                        self.assertEqual(args[:2], [str(self.root / "victory.exe"), "run"])
                        self.assertIn("--execute", args)
                    else:
                        self.assertEqual(args[:5], ["bash", str(self.root / "victory-proton.sh"),
                                                    "--report", "--runtime", "pinned"])
                        self.assertEqual(kwargs["env"]["VKD3D_SHADER_CACHE_PATH"], str(self.root / "vkd3d"))
                    return popen([sys.executable, "-c", "import sys; print(sys.argv[1]); sys.exit(42)",
                                  f"Report: {report} (launcher exit 42)"], **kwargs)
                with mock.patch.dict(os.environ, {"VICTORY_EXE": "old.exe"}), \
                        mock.patch.object(subprocess, "Popen", side_effect=start):
                    runner.launch("main", self.root, self.title, results, windows)
                self.assertEqual((results / "main-exit.txt").read_text().strip(), "42")
                if not windows:
                    self.assertEqual((results / "main.tar.gz").read_bytes(), b"this launch")

    def test_reject_wrong_merge_parent_and_expired_builds(self):
        artifact = {"name": "pr-7-" + self.revision, "expired": False}
        for expired in (False, True):
            artifact["expired"] = expired
            replies = [{"workflow_runs": [{"id": 12}]}, {"artifacts": [artifact]}]
            if not expired:
                replies.append({"parents": [{"sha": "other"}, {"sha": "wrong head"}]})
            with mock.patch.object(prepare, "api", side_effect=replies), \
                    mock.patch.object(subprocess, "run", side_effect=AssertionError), \
                    self.assertRaisesRegex(ValueError, "No unexpired successful"):
                prepare.find_build("pull_request", self.revision, "pr-7-")


if __name__ == "__main__":
    unittest.main()
