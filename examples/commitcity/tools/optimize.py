"""Shrinks a Meshy GLB for commitcity and normalizes its placement.

usage: python3 -I optimize.py IN.glb OUT.glb [texture_px]
The texture is re-encoded at texture_px (default 512) and the root node is
scaled so the footprint's longer side is 1 unit, centred on x/z, with the base at y = 0.
"""
import json
import os
import struct
import subprocess
import sys
import tempfile


def read_glb(path):
    data = open(path, "rb").read()
    magic, version, _ = struct.unpack_from("<4sII", data, 0)
    assert magic == b"glTF" and version == 2
    offset = 12
    doc, binary = None, b""
    while offset < len(data):
        length, kind = struct.unpack_from("<II", data, offset)
        chunk = data[offset + 8 : offset + 8 + length]
        if kind == 0x4E4F534A:
            doc = json.loads(chunk)
        elif kind == 0x004E4942:
            binary = chunk
        offset += 8 + length
    return doc, binary


def write_glb(path, doc, binary):
    text = json.dumps(doc, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    binary += b"\0" * (-len(binary) % 4)
    total = 12 + 8 + len(text) + 8 + len(binary)
    with open(path, "wb") as out:
        out.write(struct.pack("<4sII", b"glTF", 2, total))
        out.write(struct.pack("<II", len(text), 0x4E4F534A) + text)
        out.write(struct.pack("<II", len(binary), 0x004E4942) + binary)


def shrink(image, mime, size):
    suffix = ".png" if mime == "image/png" else ".jpg"
    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "in" + suffix)
        dst = os.path.join(tmp, "out.jpg")
        open(src, "wb").write(image)
        subprocess.run(
            ["sips", "-Z", str(size), "-s", "format", "jpeg", "-s", "formatOptions", "85", src, "--out", dst],
            check=True,
            capture_output=True,
        )
        return open(dst, "rb").read()


def main():
    src, dst = sys.argv[1], sys.argv[2]
    size = int(sys.argv[3]) if len(sys.argv) > 3 else 512
    doc, binary = read_glb(src)
    assert not doc.get("extensionsRequired"), doc.get("extensionsRequired")

    views = doc["bufferViews"]
    replaced = {}
    for image in doc.get("images", []):
        view = image["bufferView"]
        start = views[view].get("byteOffset", 0)
        raw = binary[start : start + views[view]["byteLength"]]
        replaced[view] = shrink(raw, image.get("mimeType"), size)
        image["mimeType"] = "image/jpeg"

    rebuilt = b""
    for index, view in enumerate(views):
        start = view.get("byteOffset", 0)
        chunk = replaced.get(index, binary[start : start + view["byteLength"]])
        rebuilt += b"\0" * (-len(rebuilt) % 4)
        view["byteOffset"] = len(rebuilt)
        view["byteLength"] = len(chunk)
        rebuilt += chunk
    doc["buffers"] = [{"byteLength": len(rebuilt)}]

    lo, hi = [float("inf")] * 3, [float("-inf")] * 3
    for mesh in doc["meshes"]:
        for primitive in mesh["primitives"]:
            accessor = doc["accessors"][primitive["attributes"]["POSITION"]]
            lo = [min(a, b) for a, b in zip(lo, accessor["min"])]
            hi = [max(a, b) for a, b in zip(hi, accessor["max"])]
    scale = 1.0 / max(hi[0] - lo[0], hi[2] - lo[2])
    cx, cz = (lo[0] + hi[0]) / 2, (lo[2] + hi[2]) / 2
    matrix = [scale, 0, 0, 0, 0, scale, 0, 0, 0, 0, scale, 0, -cx * scale, -lo[1] * scale, -cz * scale, 1]
    assert len(doc["nodes"]) == 1 and "children" not in doc["nodes"][0], "expected one flat node"
    node = doc["nodes"][0]
    node.pop("translation", None), node.pop("rotation", None), node.pop("scale", None)
    node["matrix"] = matrix
    write_glb(dst, doc, rebuilt)
    print(f"{os.path.basename(dst)}: {os.path.getsize(dst) // 1024} KiB, height {(hi[1] - lo[1]) * scale:.2f}")


main()
