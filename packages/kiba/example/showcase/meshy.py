"""Generates the showcase's models with Meshy's text-to-3D API.

usage: python3 meshy.py <output dir> [name ...]

Reads the key from ~/.config/nightshift/meshy_api_key. Each model in MODELS is
previewed, textured with PBR maps, and saved as <name>.glb; existing files are skipped.
"""

import json
import os
import sys
import time
import urllib.request

API = "https://api.meshy.ai/openapi/v2/text-to-3d"

# name: (prompt, texture prompt, target triangles, model for texturing)
MODELS = {
    "warehouse": (
        "two-story old red brick industrial warehouse with large steel-framed multi-pane windows, metal awning over a roll-up loading door, iron fire escape",
        "weathered red brick, dark steel window frames, warm yellow light glowing inside the windows, rusty corrugated metal awning, faded wooden door with yellow and black hazard stripes",
        20000,
        "meshy-6",
    ),
    "smokestack": (
        "tall industrial factory smokestack chimney, cylindrical, with a ladder and red and white bands near the top",
        "sooty concrete and steel, faded red and white bands, rust streaks",
        6000,
        "meshy-6-lite",
    ),
    "storage_tank": (
        "large cylindrical industrial oil storage tank with a railing around the top and a spiral staircase",
        "weathered pale grey painted steel, a faded red stripe, rust streaks, yellow railing",
        12000,
        "meshy-6-lite",
    ),
    "gas_sphere": (
        "spherical industrial gas storage tank standing on steel legs with cross bracing",
        "brushed silver painted steel, rivets, light rust",
        10000,
        "meshy-6-lite",
    ),
    "pipe_bridge": (
        "long steel truss pipe bridge over a road, carrying several large pipes, on lattice towers",
        "dark grey painted steel, rust, yellow and black hazard stripes on the towers",
        15000,
        "meshy-6-lite",
    ),
    "cable_spool": (
        "large wooden cable reel spool lying on its side wrapped with thick rope",
        "weathered brown wood, coarse beige hemp rope",
        6000,
        "meshy-6-lite",
    ),
    "barrel": (
        "rusty steel oil drum barrel",
        "rusty dark red painted steel, scratches, dents",
        3000,
        "meshy-6-lite",
    ),
    "crate": (
        "wooden shipping crate with plank sides and corner braces",
        "weathered pale pine wood planks, stencilled markings",
        2000,
        "meshy-6-lite",
    ),
    "ammo_crate": (
        "military metal ammunition crate with latches and handles",
        "olive green painted metal, chipped paint, scratches",
        3000,
        "meshy-6-lite",
    ),
    "utility_pole": (
        "old wooden utility pole with a crossarm, insulators, a transformer box and an arm holding a street lamp",
        "dark weathered wood, grey metal transformer, black cables, warm lamp glass",
        6000,
        "meshy-6-lite",
    ),
    "barrier": (
        "concrete jersey road barrier block",
        "weathered grey concrete with moss, faded yellow and black diagonal stripes",
        2000,
        "meshy-6-lite",
    ),
    "oak": (
        "large old oak tree with a thick gnarled mossy trunk and a broad full leafy green canopy",
        "rough grey brown bark covered in green moss, bright green sunlit leaves",
        25000,
        "meshy-6-lite",
    ),
    "spruce": (
        "tall spruce pine tree with dense dark green needles, conical shape",
        "dark green needles, reddish brown bark",
        15000,
        "meshy-6-lite",
    ),
    "boulder": (
        "large mossy granite boulder",
        "grey granite with lichen and thick green moss on top",
        5000,
        "meshy-6-lite",
    ),
    "river_rocks": (
        "cluster of smooth rounded river stones of various sizes",
        "wet grey and brown smooth river stones, some moss",
        5000,
        "meshy-6-lite",
    ),
    "fern": (
        "lush green fern plant with many fronds",
        "vivid green fern fronds",
        6000,
        "meshy-6-lite",
    ),
    "grass": (
        "clump of tall wild grass with a few small white wildflowers",
        "green and yellow-green grass blades, small white flowers",
        4000,
        "meshy-6-lite",
    ),
    "mountain": (
        "rugged alpine mountain peak with snow-capped summit and rocky cliffs, a landscape backdrop",
        "grey rock cliffs, white snow on the summit, dark green forest at the base",
        10000,
        "meshy-6-lite",
    ),
}


def request(method, url, body=None):
    with open(os.path.expanduser("~/.config/nightshift/meshy_api_key")) as file:
        key = file.read().strip()
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Authorization", f"Bearer {key}")
    req.add_header("Content-Type", "application/json")
    for attempt in range(6):
        try:
            with urllib.request.urlopen(req, timeout=60) as response:
                return json.loads(response.read())
        except urllib.error.HTTPError as error:
            if error.code != 429 or attempt == 5:
                raise RuntimeError(f"{method} {url}: {error.code} {error.read()[:300]}")
            time.sleep(10 * (attempt + 1))


def wait(task_ids):
    done = {}
    while len(done) < len(task_ids):
        time.sleep(10)
        for name, task_id in task_ids.items():
            if name in done:
                continue
            task = request("GET", f"{API}/{task_id}")
            if task["status"] in ("SUCCEEDED", "FAILED", "CANCELED"):
                done[name] = task
                print(f"{name}: {task['status']} {task.get('task_error') or ''}", flush=True)
    return done


def main():
    output = sys.argv[1]
    os.makedirs(output, exist_ok=True)
    names = sys.argv[2:] or list(MODELS)
    names = [n for n in names if not os.path.exists(os.path.join(output, f"{n}.glb"))]
    previews = {}
    for name in names:
        prompt, _, triangles, model = MODELS[name]
        previews[name] = request("POST", API, {
            "mode": "preview",
            "prompt": prompt,
            "ai_model": model,
            "should_remesh": True,
            "topology": "triangle",
            "target_polycount": triangles,
            "target_formats": ["glb"],
        })["result"]
    previewed = wait(previews)
    refines = {}
    for name, task in previewed.items():
        if task["status"] != "SUCCEEDED":
            continue
        _, texture, _, model = MODELS[name]
        refines[name] = request("POST", API, {
            "mode": "refine",
            "preview_task_id": task["id"],
            "texture_prompt": texture,
            "enable_pbr": True,
            "ai_model": model,
            "target_formats": ["glb"],
        })["result"]
    for name, task in wait(refines).items():
        if task["status"] == "SUCCEEDED":
            urllib.request.urlretrieve(task["model_urls"]["glb"], os.path.join(output, f"{name}.glb"))
            print(f"{name}: saved, {task.get('consumed_credits')} credits", flush=True)


if __name__ == "__main__":
    main()
