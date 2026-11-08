#!/usr/bin/env python3
"""Usage: merge_demo_project.py SCENEMERGE DEMO_PROJECT_DIR WORK_DIR"""

import os
import re
import shutil
import subprocess
import sys

def git(cwd, *args, check=True):
    r = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True)
    if check and r.returncode != 0:
        sys.exit(f"git {' '.join(args)} failed:\n{r.stdout}\n{r.stderr}")
    return r

def scenes(root):
    for d, dirs, files in os.walk(root):
        dirs[:] = [x for x in dirs if x not in (".git", ".godot")]
        for f in files:
            if f.endswith(".tscn"):
                yield os.path.join(d, f)

def read(p):
    with open(p, encoding="utf-8", newline="") as f:
        return f.read()

def write(p, s):
    with open(p, "w", encoding="utf-8", newline="") as f:
        f.write(s)

def add_root_child(text, name):
    """Adds `[node name=NAME type="Node" parent="."]` after the last node."""
    lines = text.split("\n")
    last_node = max((i for i, l in enumerate(lines) if l.startswith("[node ")), default=None)
    if last_node is None:
        return text
    end = len(lines)
    for i in range(last_node + 1, len(lines)):
        if lines[i].startswith("[") and (lines[i].startswith("[connection") or lines[i].startswith("[editable")):
            end = i
            break
    while end > last_node + 1 and lines[end - 1] == "":
        end -= 1
    block = ["", f'[node name="{name}" type="Node" parent="."]']
    return "\n".join(lines[:end] + block + lines[end:])

EXT_HEADER = re.compile(r'^(\[ext_resource\b[^\n]*?\bid=")([^"]+)(")', re.M)
EXT_REF = re.compile(r'ExtResource\(\s*"?([A-Za-z0-9_]+)"?\s*\)')

def renumber_ext_ids(text):
    ids = [m.group(2) for m in EXT_HEADER.finditer(text)]
    mapping = {old: f"{i + 1}_r{abs(hash(old)) % 99999:05d}" for i, old in enumerate(reversed(ids))}
    text = EXT_HEADER.sub(lambda m: m.group(1) + mapping.get(m.group(2), m.group(2)) + m.group(3), text)
    return EXT_REF.sub(lambda m: f'ExtResource("{mapping.get(m.group(1), m.group(1))}")', text)

def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    exe, demo, work = sys.argv[1], sys.argv[2], sys.argv[3]
    if os.path.exists(work):
        shutil.rmtree(work)
    shutil.copytree(demo, work, ignore=shutil.ignore_patterns(".godot", ".git"))
    git(work, "init", "-q", "-b", "main")
    for k, v in [("user.name", "CI"), ("user.email", "ci@example.invalid"),
                 ("core.autocrlf", "false"), ("commit.gpgsign", "false")]:
        git(work, "config", k, v)
    cmd = '"' + os.path.abspath(exe).replace("\\", "/") + '"'
    r = subprocess.run([exe, "install", "--command", cmd], cwd=work, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(r.stderr)
    git(work, "add", "-A")
    git(work, "commit", "-q", "-m", "base")

    files = sorted(scenes(work))
    git(work, "checkout", "-q", "-b", "feature")
    for p in files:
        write(p, add_root_child(renumber_ext_ids(read(p)), "ScenemergeTheirs"))
    git(work, "commit", "-q", "-am", "feature")
    git(work, "checkout", "-q", "main")
    for p in files:
        write(p, add_root_child(read(p), "ScenemergeOurs"))
    git(work, "commit", "-q", "-am", "main")

    r = git(work, "merge", "--no-edit", "feature", check=False)
    if r.returncode != 0:
        sys.exit(f"git merge failed:\n{r.stdout}\n{r.stderr}")
    bad = []
    for p in files:
        t = read(p)
        if "<<<<<<<" in t or '"ScenemergeOurs"' not in t or '"ScenemergeTheirs"' not in t:
            bad.append(p)
    if bad:
        sys.exit("bad merge results:\n" + "\n".join(bad))
    print(f"merged {len(files)} scene(s) cleanly in {work}")

if __name__ == "__main__":
    main()
