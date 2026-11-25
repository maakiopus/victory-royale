"""Find and run a Series X hardware comparison for VIctory. Python 3.9+."""
import argparse
from datetime import datetime, timedelta, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import textwrap
import zipfile

REPO = "ligmaSec/victory-royale"
PACKAGE = "victory-proton-x86_64.tar.gz"
ROOT = Path(__file__).resolve().parent


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            value.update(block)
    return value.hexdigest()


def read_request(path):
    request = json.loads(path.read_text(encoding="utf-8"))
    if (request["version"] != 1 or type(request["pr"]) is not int or request["pr"] < 1
            or not isinstance(request["scenario"], str) or not request["scenario"].strip()
            or not request["scenario"].isprintable()):
        raise ValueError("Unrecognized test request.")
    for side in ("main", "candidate"):
        build = request[side]
        if not (type(build["run"]) is int and build["run"] > 0
                and re.fullmatch(r"[0-9a-f]{40}", build["revision"])
                and re.fullmatch(r"[0-9a-f]{64}", build["sha256"])):
            raise ValueError(f"Invalid {side} build identity.")
    return request


def stage_build(request, side, folder, packages):
    build = request[side]
    prefix = "" if side == "main" else f"pr-{request['pr']}-"
    artifact = f"victory-proton-x86_64-{prefix}{build['revision']}"
    folder.mkdir()
    archive = packages / (build["sha256"] + ".tar.gz")
    if not archive.is_file():
        if not shutil.which("gh"):
            raise ValueError("The request is missing its packages. Extract the entire kit or install GitHub CLI.")
        subprocess.run(["gh", "run", "download", str(build["run"]), "-R", REPO,
                        "-n", artifact, "-D", str(folder)], check=True)
        archive = folder / PACKAGE
    if digest(archive) != build["sha256"]:
        raise ValueError(f"{side} package checksum differs from the request.")
    # Copy known distribution members only. Never unpack arbitrary paths or links.
    with tarfile.open(archive, "r:gz") as bundle:
        for name in ("victory.exe", "victory-proton.sh", "REVISION", "LICENSE"):
            member = bundle.getmember("victory/" + name)
            if not member.isfile():
                raise ValueError(f"Unexpected {side} package member: {name}")
            with (folder / name).open("wb") as out:
                shutil.copyfileobj(bundle.extractfile(member), out)
    if (folder / "REVISION").read_text().strip() != build["revision"]:
        raise ValueError(f"{side} package contains a different revision.")


def choose_title():
    try:
        from tkinter import TclError, Tk, filedialog
    except ImportError:
        window = None
    else:
        try:
            window = Tk()
        except TclError:
            window = None
    if window is not None:
        try:
            window.withdraw()
            chosen = filedialog.askopenfilename(title="Select your game executable",
                                                filetypes=[("Executable", "*.exe"), ("All files", "*")])
        finally:
            window.destroy()
        if not chosen:
            raise ValueError("Selection cancelled.")
    else:
        chosen = input("Game executable path: ").strip().strip("\"'")
    return Path(chosen).expanduser()


def launch(side, folder, title, results, windows):
    env = os.environ.copy()
    # Ignore a developer override that could silently substitute a different build.
    env.pop("VICTORY_EXE", None)
    env["VICTORY_TRACE_PIPELINES"] = "1"
    cache = folder / "cache"
    env["VICTORY_CACHE_DIR"] = str(cache) if windows else "Z:" + str(cache).replace("/", "\\")
    args = [str(title), "--console", "series-x"]
    if windows:
        command = [str(folder / "victory.exe"), "run", str(title), "--execute", "--console", "series-x"]
    else:
        env.update(VKD3D_SHADER_CACHE_PATH=str(folder / "vkd3d"), DXVK_STATE_CACHE_PATH=str(folder / "dxvk"))
        command = ["bash", str(folder / "victory-proton.sh"), "--report", "--runtime", "pinned", *args]
    console = results / (side + ".log")
    report = None
    with console.open("wb") as log, subprocess.Popen(command, cwd=title.parent, env=env,
                                                       stdout=subprocess.PIPE, stderr=subprocess.STDOUT) as process:
        with process.stdout:
            for line in process.stdout:
                log.write(line)
                if not windows:
                    found = re.fullmatch(rb"Report: (.+) \(launcher exit [0-9]+\)", line.rstrip(b"\r\n"))
                    if found:
                        report = Path(os.fsdecode(found[1]))
        status = process.wait()
    (results / (side + "-exit.txt")).write_text(f"{status}\n")
    if not windows:
        if report is None or not report.is_file():
            raise ValueError(f"The {side} launcher did not produce a report. See the console log in the partial report.")
        shutil.copyfile(report, results / (side + ".tar.gz"))
    print(f"{side} exited ({status}).", flush=True)


def github_rows(endpoint, query):
    if not shutil.which("gh"):
        raise ValueError("Published requests need GitHub CLI. Install gh and run gh auth login, "
                         "or pass a downloaded kit ZIP to this command.")
    result = subprocess.run(["gh", "api", "--paginate", f"repos/{REPO}/{endpoint}", "--jq", query],
                            capture_output=True, encoding="utf-8")
    if result.returncode:
        raise ValueError(f"GitHub lookup failed: {result.stderr.strip()}\n"
                         "Check gh auth status and your connection, or use a downloaded kit ZIP.")
    return [json.loads(line) for line in result.stdout.splitlines() if line]


def published_requests(number):
    pulls = github_rows("pulls?state=open&base=main&per_page=100",
                        ".[] | {number, title, head: .head.sha}")
    open_prs = {pr["number"]: pr for pr in pulls if number is None or pr["number"] == number}
    if not open_prs:
        return {}
    # The workflow retains kits for seven days. Trust successful dispatches on main.
    since = (datetime.now(timezone.utc) - timedelta(days=8)).date().isoformat()
    runs = github_rows("actions/workflows/hardware-request.yml/runs?branch=main&event=workflow_dispatch"
                       f"&status=success&created=>={since}&per_page=100", ".workflow_runs[] | .id")
    available = {}
    for run_id in sorted(runs, reverse=True):
        artifacts = github_rows(f"actions/runs/{run_id}/artifacts?per_page=100",
                                '.artifacts[] | select(.expired == false) | {name}')
        for artifact in artifacts:
            match = re.fullmatch(r"victory-series-x-pr-([1-9][0-9]*)", artifact["name"])
            if match:
                pr = int(match[1])
                if pr in open_prs and pr not in available:
                    available[pr] = (open_prs[pr], run_id, artifact["name"])
        if len(available) == len(open_prs):
            break
    return available


def select_published(number, list_only, folder):
    available = published_requests(number)
    if number is None:
        if not available:
            print("There are no published Series X requests for open PRs.")
            return None
        print("Available Series X hardware tests:")
        for pr, (pull, _, _) in sorted(available.items()):
            print(f"  #{pr}  {pull['title']}")
        if list_only or not sys.stdin.isatty():
            print("To run one, pass its PR number to this command.")
            return None
        choice = input("PR number (Enter to cancel): ").strip().lstrip("#")
        if not choice:
            return None
        if not choice.isascii() or not choice.isdecimal() or int(choice) not in available:
            raise ValueError("Choose one of the listed PR numbers.")
        number = int(choice)
    if number not in available:
        raise ValueError(f"No published kit for open PR #{number}. Ask the requester to publish a fresh kit.")
    pull, run_id, artifact = available[number]
    print(f"Downloading the Series X test for PR #{number}…", flush=True)
    subprocess.run(["gh", "run", "download", str(run_id), "-R", REPO, "-n", artifact, "-D", str(folder)],
                   check=True)
    manifest = folder / "request.json"
    if not manifest.is_file():
        raise ValueError("The published kit is missing its request.json.")
    request = read_request(manifest)
    if request["pr"] != number:
        raise ValueError("The published kit names a different PR.")
    parents = github_rows("git/commits/" + request["candidate"]["revision"], ".parents[] | {sha}")
    if [parent["sha"] for parent in parents] != [request["main"]["revision"], pull["head"]]:
        raise ValueError(f"PR #{number} changed after this kit was built. Ask the requester to publish a fresh kit.")
    return manifest


def unpack_kit(path, folder, plan):
    manifest = folder / "request.json"
    with zipfile.ZipFile(path) as kit:
        if "request.json" not in kit.namelist():
            raise ValueError("This ZIP has no hardware test request.json.")
        manifest.write_bytes(kit.read("request.json"))
        request = read_request(manifest)
        if not plan:
            packages = folder / "packages"
            packages.mkdir()
            for checksum in {request[side]["sha256"] for side in ("main", "candidate")}:
                name = checksum + ".tar.gz"
                with kit.open("packages/" + name) as source, (packages / name).open("wb") as target:
                    shutil.copyfileobj(source, target)
    return manifest


def run():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("request", nargs="?", help="PR number, test kit ZIP or request JSON")
    parser.add_argument("--list", action="store_true", help="list published Series X requests")
    parser.add_argument("--output-dir", type=Path, help="parent directory for the unique report")
    parser.add_argument("--plan", action="store_true", help="preview the selected test without launching")
    parser.add_argument("--title", type=Path, help="use this executable instead of the file chooser")
    options = parser.parse_args()
    if options.list and options.request:
        parser.error("--list takes no request")
    with tempfile.TemporaryDirectory(prefix="victory-request-") as temporary:
        folder = Path(temporary)
        argument = options.request
        manifest = ROOT / "request.json"
        if options.list or (argument is None and not manifest.is_file()):
            manifest = select_published(None, options.list, folder)
        elif argument is not None:
            if re.fullmatch(r"#?[1-9][0-9]*", argument):
                manifest = select_published(int(argument.lstrip("#")), False, folder)
            else:
                path = Path(argument).expanduser()
                if not path.is_file():
                    raise ValueError(f"Cannot find {path}. Pass a PR number or an existing kit ZIP or request JSON.")
                manifest = unpack_kit(path, folder, options.plan) if path.suffix.lower() == ".zip" else path
        if manifest is not None:
            run_request(manifest, options)


def instruction_block(heading, message, path_line=None):
    width = max(20, min(72, shutil.get_terminal_size().columns - 1))
    rule = "=" * width
    message = textwrap.fill(message, width, break_long_words=False, break_on_hyphens=False)
    lines = ["", rule, textwrap.fill(heading, width), rule, message]
    if path_line:
        lines.extend(["", path_line])
    print("\n".join(lines), end="\n\n", flush=True)


def run_request(manifest, options):
    request = read_request(manifest)
    print(f"VIctory PR #{request['pr']} | Series X | two launches", flush=True)
    if options.plan:
        for side in ("main", "candidate"):
            build = request[side]
            print(f"{side}: {build['revision'][:12]}  https://github.com/{REPO}/actions/runs/{build['run']}")
        instruction_block("TEST INSTRUCTIONS", request["scenario"])
        return
    windows = sys.platform == "win32"
    if not windows and not sys.platform.startswith("linux"):
        raise ValueError("This kit supports Windows and Linux/Proton.")
    for binary in ([] if windows else ["bash", "umu-run"]):
        if not shutil.which(binary):
            raise ValueError(f"Missing {binary}. See the requirements in README.txt.")
    title = (options.title or choose_title()).expanduser().resolve()
    if not title.is_file() or title.suffix.lower() != ".exe":
        raise ValueError("Select the game's .exe file.")
    temp = Path(tempfile.gettempdir()).resolve()
    output = (options.output_dir or temp).expanduser().resolve()
    for directory in (output, temp, title.parent):
        if any((parent / ".victory-root").exists() for parent in (directory, *directory.parents)):
            raise ValueError("Use a game, temporary directory and report destination outside the VIctory checkout.")
    with tempfile.TemporaryDirectory(prefix="victory-builds-") as work:
        for side in ("main", "candidate"):
            print(f"Checking {side}…", flush=True)
            stage_build(request, side, Path(work) / side, manifest.resolve().parent / "packages")
        results = Path(work) / "results"
        results.mkdir()
        shutil.copyfile(manifest, results / "request.json")
        complete = False
        try:
            for number, side in enumerate(("main", "candidate"), start=1):
                instruction_block(f"RUN {number} OF 2  |  {side.upper()}  |  WHAT TO DO", request["scenario"])
                launch(side, Path(work) / side, title, results, windows)
            complete = True
        finally:
            if any(results.glob("*.log")):
                output.mkdir(parents=True, exist_ok=True)
                destination = Path(tempfile.mkdtemp(prefix=f"victory-pr{request['pr']}-", dir=output))
                bundle = destination / "report.zip"
                with zipfile.ZipFile(bundle, "x", zipfile.ZIP_DEFLATED, compresslevel=1) as out:
                    for path in sorted(results.iterdir()):
                        out.write(path, path.name)
                label = "Upload" if complete else "Partial report"
                heading = "REPORT READY" if complete else "TEST INTERRUPTED"
                instruction_block(heading,
                                  "Send this ZIP privately to the person who requested the test. Keep it zipped.",
                                  f"{label}: {bundle}")


if __name__ == "__main__":
    try:
        run()
    except (OSError, ValueError, TypeError, KeyError, EOFError, subprocess.CalledProcessError, tarfile.TarError, zipfile.BadZipFile) as error:
        sys.exit(str(error))
