"""Meshy driver for the Live Cities avatars: text-to-3d, then rigging and animation.

usage: MESHY_API_KEY=... python3 -I meshy.py preview|refine|rig|animate NAME... | wait
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
HOST = "https://api.meshy.ai/openapi"

STYLE = (
    "Cute chibi avatar for a playful social map app, like a vinyl toy: a big round head, "
    "a small rounded body, short limbs, smooth simple shapes, bright saturated pop colors, "
    "soft glossy finish, friendly smiling face. Full body, standing in a T-pose, facing forward, "
    "no base, no background."
)

MODELS = {
    "mina": "a girl with a short black bob, a yellow beanie, a mint green hoodie, white shorts and pink sneakers",
    "kai": "a boy with spiky brown hair, a backwards orange cap, a bright blue windbreaker, black pants and white sneakers",
    "sora": "a girl with pink hair in two buns, a purple puffer jacket, a yellow skirt and white boots",
    "taro": "a chubby boy with round glasses, messy black hair, a red and white striped sweater, blue jeans and green sneakers",
}

# Library ids: 30 Casual_Walk, 0 Idle, 28 Big_Wave_Hello.
ACTIONS = [30, 0, 28]


def request(method, url, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Authorization", "Bearer " + KEY)
    req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.loads(resp.read())


def load():
    return json.load(open(STATE)) if os.path.exists(STATE) else {}


def save(state):
    json.dump(state, open(STATE + ".tmp", "w"), indent=2)
    os.replace(STATE + ".tmp", STATE)


KINDS = {
    "preview": "v2/text-to-3d",
    "refine": "v2/text-to-3d",
    "rig": "v1/rigging",
    "animate": "v1/animations",
}


def start(kind, names):
    state = load()
    for name in names:
        entry = state.setdefault(name, {})
        prompt = f"{MODELS[name]}. {STYLE}"
        if kind == "preview":
            body = {
                "mode": "preview",
                "prompt": prompt,
                "ai_model": "latest",
                "pose_mode": "t-pose",
                "should_remesh": True,
                "topology": "triangle",
                "target_polycount": 8000,
            }
        elif kind == "refine":
            body = {
                "mode": "refine",
                "preview_task_id": entry["preview"],
                "enable_pbr": False,
                "texture_prompt": prompt,
            }
        elif kind == "rig":
            body = {"input_task_id": entry["refine"], "height_meters": 1.2}
        else:
            body = {"rig_task_id": entry["rig"], "action_ids": ACTIONS}
        entry[kind] = request("POST", f"{HOST}/{KINDS[kind]}", body)["result"]
        for key in (kind + "_done", kind + "_error"):
            entry.pop(key, None)
        save(state)
        print(name, kind, entry[kind])


def downloads(kind, task):
    result = task.get("result") if isinstance(task.get("result"), dict) else {}
    if kind in ("preview", "refine"):
        return {"png": task.get("thumbnail_url"), "glb": task.get("model_urls", {}).get("glb")}
    if kind == "rig":
        walking = (result.get("basic_animations") or {}).get("walking_glb_url")
        return {"glb": result.get("rigged_character_glb_url"), "walk.glb": walking}
    return {"glb": result.get("animation_glb_url")}


def wait(once=False):
    while True:
        state = load()
        pending = 0
        for name, entry in state.items():
            for kind, path in KINDS.items():
                task_id = entry.get(kind)
                if not task_id or entry.get(kind + "_done"):
                    continue
                task = request("GET", f"{HOST}/{path}/{task_id}")
                print(name, kind, task["status"], task.get("progress"), task.get("consumed_credits"))
                if task["status"] == "SUCCEEDED":
                    os.makedirs(OUT, exist_ok=True)
                    for suffix, url in downloads(kind, task).items():
                        if url:
                            urllib.request.urlretrieve(url, os.path.join(OUT, f"{name}-{kind}.{suffix}"))
                    entry[kind + "_done"] = True
                elif task["status"] in ("FAILED", "CANCELED"):
                    entry[kind + "_error"] = task.get("task_error")
                    entry[kind + "_done"] = True
                else:
                    pending += 1
                save(state)
        if once or pending == 0:
            return
        time.sleep(15)


if __name__ == "__main__":
    command, args = sys.argv[1], sys.argv[2:]
    if command == "wait":
        wait()
    elif command == "status":
        wait(once=True)
    else:
        start(command, args or list(MODELS))
