"""Building oxide-run commands and running scripts: the parts every conductor shares.

A conductor (a file like abstract.py) builds a list of oxide-run commands (dicts, or "# comment"
strings), saves it with write_script, and paints it with run_script:

    cmds = [{"command": "new_painting", "width": 1200, "height": 900}, color_command("#C2151B"), ...]
    write_script(cmds, Path("my-painting.jsonl"))
    run_script(Path("my-painting.jsonl").resolve())      # paints it with oxide-run (through cargo)
"""

import json
import subprocess
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent  # the folder this file is in
REPO = SCRIPT_DIR.parent.parent  # repo root, where cargo runs oxide-run


def brush_command(brush, **settings):
    """The oxide-run command that selects `brush` (a dict from brushes.list_brushes).

    `settings` are extra set_brush values applied after the brush, such as
    size=40 or oiliness=0.3. Only override size and paint settings (oiliness,
    loading, length, thickness): shape settings like roundness or streaks
    would undo the brush you picked.
    """
    return {"command": "set_brush", "preset": brush["name"], **settings}


def color_command(color):
    """Return the oxide-run command that makes `color` (a hex string) the paint color.

    Every stroke after this command paints in that color until the next
    set_color. Example: color_command("#4F7329") returns
    {"command": "set_color", "hex": "#4F7329"}.
    """
    return {"command": "set_color", "hex": color}


def finish(png_path, oxide_path):
    """The two commands every painting ends with: export a PNG and save the editable .oxide file.
    (Give absolute paths, so oxide-run saves them where you expect.)"""
    return [{"command": "export", "path": str(png_path)},
            {"command": "save", "path": str(oxide_path)}]


def write_script(cmds, path):
    """Save the commands from generate() to `path` as a .jsonl file (one command per line).

    That file is what oxide-run reads. "# comment" strings are written as
    they are (oxide-run ignores them, but they make the file readable) and
    each dict is converted to one line of JSON.
    """
    with open(path, "w") as f:
        for c in cmds:
            f.write((c if isinstance(c, str) else json.dumps(c)) + "\n")


def read_script(path):
    """Read a .jsonl script (one written by write_script, or recorded in the app with
    ⏺ Record) back into a list of commands: a dict for each command and a "# comment"
    string for each comment line. Empty lines are skipped. The opposite of write_script.
    """
    cmds = []
    for number, line in enumerate(Path(path).read_text().splitlines(), start=1):
        line = line.strip()
        if not line:
            continue
        if line.startswith("#") or line.startswith("//"):
            cmds.append(line)
            continue
        try:
            cmds.append(json.loads(line))
        except json.JSONDecodeError as e:
            raise ValueError(f"{path}, line {number}: not a command ({e})") from None
    return cmds


def run_script(path):
    """Run the .jsonl script at `path` through oxide-run, which paints it.

    Returns True if everything worked, False if oxide-run reported an
    error. This builds oxide-run on first use, so the first call can be slow.
    Only used when you pass --run.
    """
    # Run `cargo run ... oxide-run -- <script>` from the repo root, capturing
    # its output instead of showing it so we can pick out any errors.
    result = subprocess.run(
        ["cargo", "run", "-q", "--release", "-p", "oxide-agent", "--bin", "oxide-run", "--", str(path)],
        cwd=REPO, capture_output=True, text=True)
    # oxide-run prints one JSON reply per command; show the ones that failed.
    failures = [line for line in result.stdout.splitlines() if '"ok":false' in line]
    for line in failures:
        print("  " + line, file=sys.stderr)
    # A failure with no failed command means something else broke (for
    # example, cargo couldn't build), so show cargo's own error text.
    if result.returncode != 0 and not failures:
        print(result.stderr, file=sys.stderr)
    return result.returncode == 0
