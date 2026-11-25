"""Meshy text-to-3d driver for the commitcity models.

usage: MESHY_API_KEY=... python3 -I meshy.py preview NAME... | refine NAME... | status | wait
Task ids are kept in tasks.json and downloads go to out/, both next to the current directory.
"""
import json
import os
import sys
import time
import urllib.request

STATE = "tasks.json"
OUT = "out"
KEY = os.environ["MESHY_API_KEY"]
API = "https://api.meshy.ai/openapi/v2/text-to-3d"

STYLE = (
    "Stylized hand-crafted game asset in an Edo period Japanese castle town. "
    "Warm natural wood, white plaster, dark grey kawara roof tiles, soft muted colors, "
    "clean readable shapes, no background"
)
ON_BASE = ", standing alone on a small square base, no ground plane beyond the base."
ON_GROUND = ", standing directly on the ground with no base, no plinth, no platform and no ground plane."

MODELS = {
    "ishidoro": ("a single weathered stone lantern (ishi-doro)", 3000, ON_GROUND),
    "machiya": ("a small one-story wooden machiya townhouse with a latticed front, a dark blue noren curtain and a dark tiled roof", 5000, ON_GROUND),
    "kominka": ("a small one-story Japanese farmhouse with a thick thatched roof", 5000, ON_GROUND),
    "chaya": ("a small roadside tea house with an open front, a red felt bench and a red paper parasol", 5000, ON_GROUND),
    "shoka": ("a two-story Edo merchant shop house with wooden lattice windows, a hanging wooden shop sign and a tiled roof", 6000, ON_GROUND),
    "kura": ("a two-story white plaster storehouse (kura) with black and white namako tile walls and a heavy tiled roof", 5000, ON_GROUND),
    "ryokan": ("a two-story wooden inn with a balcony and hanging red paper lanterns", 6000, ON_GROUND),
    "jinja": ("a small Shinto shrine hall with vermilion pillars and a copper green roof", 6000, ON_BASE),
    "hondo": ("a Buddhist temple main hall with a sweeping curved tiled roof", 7000, ON_BASE),
    "shoro": ("a Buddhist bell tower (shoro) with a large bronze bell hanging under a curved roof", 5000, ON_BASE),
    "goju": ("a tall five-story wooden pagoda", 8000, ON_BASE),
    "tenshu": ("a Japanese castle keep (tenshu) with white walls and layered grey curved roofs on a sloped stone base", 9000, ON_BASE),
    "torii": ("a vermilion Shinto torii gate", 2000, ON_BASE),
    "sakura": ("a cherry blossom tree in full pink bloom with a dark gnarled trunk", 6000, ON_GROUND),
    "matsu": ("a Japanese black pine tree with a twisting trunk and layered cloud-pruned dark green foliage", 5000, ON_GROUND),
    "kido": ("an Edo period wooden town ward gate (kido): two thick dark wooden posts joined by a beam with a small tiled roof, double wooden doors standing open", 3000, ON_GROUND),
    "sugi": ("a tall slender Japanese cedar tree (sugi) with a straight reddish trunk and a narrow conical crown of dark green foliage", 1500, ON_GROUND),
    "yanagi": ("a weeping willow tree with a stout trunk and long drooping pale green branches", 4000, ON_GROUND),
    "hashi": ("a long arched wooden taiko drum bridge with simple wooden railings, seen from the side", 4000, ON_GROUND),
}


def request(method, url, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Authorization", "Bearer " + KEY)
    req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.loads(resp.read())


def load():
    if os.path.exists(STATE):
        return json.load(open(STATE))
    return {}


def save(state):
    tmp = STATE + ".tmp"
    json.dump(state, open(tmp, "w"), indent=2)
    os.replace(tmp, STATE)


def preview(names):
    state = load()
    for name in names:
        subject, polycount, footing = MODELS[name]
        task = request("POST", API, {
            "mode": "preview",
            "prompt": f"{subject}. {STYLE}{footing}",
            "ai_model": "latest",
            "should_remesh": True,
            "topology": "triangle",
            "target_polycount": polycount,
            "target_formats": ["glb"],
        })["result"]
        state.setdefault(name, {})["preview"] = task
        save(state)
        print(name, "preview", task)


def refine(names):
    state = load()
    for name in names:
        subject, _, footing = MODELS[name]
        task = request("POST", API, {
            "mode": "refine",
            "preview_task_id": state[name]["preview"],
            "enable_pbr": False,
            "texture_resolution": "2k",
            "texture_prompt": f"{subject}. {STYLE}{footing}",
            "target_formats": ["glb"],
        })["result"]
        state[name]["refine"] = task
        save(state)
        print(name, "refine", task)


def fetch(url, path):
    urllib.request.urlretrieve(url, path)


def status(wait=False):
    while True:
        state = load()
        pending = 0
        for name, entry in state.items():
            for kind in ("preview", "refine"):
                task_id = entry.get(kind)
                if not task_id or entry.get(kind + "_done"):
                    continue
                task = request("GET", f"{API}/{task_id}")
                print(name, kind, task["status"], task.get("progress"), task.get("consumed_credits"))
                if task["status"] == "SUCCEEDED":
                    os.makedirs(OUT, exist_ok=True)
                    thumb = task.get("thumbnail_url")
                    if thumb:
                        fetch(thumb, os.path.join(OUT, f"{name}-{kind}.png"))
                    glb = task.get("model_urls", {}).get("glb")
                    if glb:
                        fetch(glb, os.path.join(OUT, f"{name}-{kind}.glb"))
                    entry[kind + "_done"] = True
                    save(state)
                elif task["status"] in ("FAILED", "CANCELED"):
                    entry[kind + "_error"] = task.get("task_error")
                    entry[kind + "_done"] = True
                    save(state)
                else:
                    pending += 1
        if not wait or pending == 0:
            return
        time.sleep(15)


if __name__ == "__main__":
    command, args = sys.argv[1], sys.argv[2:]
    if command == "preview":
        preview(args)
    elif command == "refine":
        refine(args)
    elif command == "status":
        status()
    elif command == "wait":
        status(wait=True)
