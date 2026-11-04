"""Prints each glb's bounds as Rust table rows: ("name", [min], [max])."""

import json
import os
import struct
import sys

for path in sys.argv[1:]:
    data = open(path, "rb").read()
    length = struct.unpack("<I", data[12:16])[0]
    gltf = json.loads(data[20 : 20 + length])
    low, high = [1e9] * 3, [-1e9] * 3
    for mesh in gltf["meshes"]:
        for primitive in mesh["primitives"]:
            accessor = gltf["accessors"][primitive["attributes"]["POSITION"]]
            low = [min(a, b) for a, b in zip(low, accessor["min"])]
            high = [max(a, b) for a, b in zip(high, accessor["max"])]
    name = os.path.basename(path)[:-4]
    print(f'    ("{name}", [{low[0]:.3}, {low[1]:.3}, {low[2]:.3}], [{high[0]:.3}, {high[1]:.3}, {high[2]:.3}]),')
