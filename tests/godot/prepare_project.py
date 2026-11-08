#!/usr/bin/env python3
"""Usage: prepare_project.py OUT_DIR [--cases DIR] [--scenes DIR ...]"""

import argparse
import base64
import json
import os
import re
import shutil
import sys

PNG_1x1 = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg=="
)
SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8"/></svg>\n'

CONCRETE = {
    "Texture2D": "ImageTexture",
    "Texture": "ImageTexture",
    "Material": "StandardMaterial3D",
    "Shape3D": "BoxShape3D",
    "Shape2D": "RectangleShape2D",
    "Mesh": "BoxMesh",
    "Font": "FontVariation",
    "AudioStream": "AudioStreamGenerator",
}

EXT_RE = re.compile(r'^\[ext_resource\b[^\]]*\]', re.M)

def field(header: str, key: str):
    m = re.search(r'\b' + key + r'="((?:[^"\\]|\\.)*)"', header)
    return m.group(1) if m else None

def stub(project: str, res_path: str, type_name: str) -> None:
    if not res_path.startswith("res://"):
        return
    rel = res_path[len("res://"):]
    target = os.path.join(project, *rel.split("/"))
    if os.path.exists(target):
        return
    os.makedirs(os.path.dirname(target) or project, exist_ok=True)
    ext = os.path.splitext(rel)[1].lower()
    if ext == ".gd":
        text = "extends Object\n"
    elif ext in (".tscn", ".scn"):
        text = '[gd_scene format=3]\n\n[node name="Stub" type="Node"]\n'
    elif ext in (".gdshader", ".shader"):
        text = "shader_type canvas_item;\n"
    elif ext in (".tres", ".res"):
        t = CONCRETE.get(type_name, type_name or "Resource")
        text = f'[gd_resource type="{t}" format=3]\n\n[resource]\n'
    elif ext == ".png":
        with open(target, "wb") as f:
            f.write(PNG_1x1)
        return
    elif ext == ".svg":
        text = SVG
    else:
        print(f"warning: no stub for {res_path} ({type_name})", file=sys.stderr)
        return
    with open(target, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)

def add_file(project: str, src: str, dest_rel: str) -> None:
    dest = os.path.join(project, "merged", *dest_rel.split("/"))
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    shutil.copyfile(src, dest)
    text = open(src, encoding="utf-8").read()
    for m in EXT_RE.finditer(text):
        h = m.group(0)
        stub(project, field(h, "path") or "", field(h, "type") or "")

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--cases", default=None)
    ap.add_argument("--scenes", action="append", default=[])
    a = ap.parse_args()

    if os.path.exists(a.out):
        shutil.rmtree(a.out)
    os.makedirs(a.out)
    with open(os.path.join(a.out, "project.godot"), "w", encoding="utf-8", newline="\n") as f:
        f.write('config_version=5\n\n[application]\n\nconfig/name="scenemerge-integration"\n')
    here = os.path.dirname(os.path.abspath(__file__))
    shutil.copyfile(os.path.join(here, "check_load.gd"), os.path.join(a.out, "check_load.gd"))

    count = 0
    if a.cases:
        for name in sorted(os.listdir(a.cases)):
            d = os.path.join(a.cases, name)
            report = os.path.join(d, "expected-report.json")
            if not os.path.isfile(report):
                continue
            r = json.load(open(report, encoding="utf-8"))
            if not r.get("clean"):
                continue
            for ext in ("tscn", "tres"):
                p = os.path.join(d, f"expected.{ext}")
                if os.path.isfile(p):
                    add_file(a.out, p, f"{name}.{ext}")
                    count += 1
    for scenes in a.scenes:
        base = os.path.basename(os.path.normpath(scenes))
        for root, dirs, files in os.walk(scenes):
            dirs[:] = [x for x in dirs if x not in (".git", ".godot")]
            for fn in files:
                if fn.endswith((".tscn", ".tres")):
                    src = os.path.join(root, fn)
                    rel = os.path.relpath(src, scenes).replace(os.sep, "/")
                    add_file(a.out, src, f"{base}/{rel}")
                    count += 1
    print(f"prepared {count} merged file(s) in {a.out}")
    return 0 if count else 1

if __name__ == "__main__":
    sys.exit(main())
