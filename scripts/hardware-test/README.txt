Run this Series X comparison

Extract the whole kit. Open a terminal in the folder containing this file.
Use the command for your system.

  Linux/Proton    python3 run-test.py
  Windows         py run-test.py

Select your game's .exe when prompted. Before each launch, read WHAT TO DO.
Run the scenario on main, close the game as instructed, then repeat on the
candidate with the same settings and window size. Each build gets a fresh
translation cache. Add --plan to preview the instructions and build revisions.

At REPORT READY, send the intact report.zip at the Upload: path privately
to the requester. It stays in a new system temporary folder after exit.
Send it before the system cleans that folder. If you see Partial report:,
send that file and tell the requester which run stopped.

Both verified CI builds are included. You need Python 3.9 or newer. Linux
also needs Bash and UMU (umu-run) and may download pinned vkd3d-proton on
first use. Keep the game executable outside the VIctory checkout.

From a repository checkout, use python3 scripts/hardware-test/run-test.py
(or py on Windows) to choose a published test. This menu requires a
signed-in GitHub CLI. You can also pass the PR number, a downloaded kit ZIP
or a request JSON. A JSON without bundled packages needs GitHub CLI to
download the builds.

Use --title "path to your game.exe" to skip the chooser, or --output-dir
"path to folder" to choose where the runner creates its report folder.
