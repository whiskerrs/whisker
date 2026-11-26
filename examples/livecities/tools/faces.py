"""Crops each avatar's head from its Meshy refine thumbnail into a square marker icon.

usage: python3 -I faces.py OUT_DIR   (run in tools/, after meshy.py refine has downloaded out/NAME-refine.png)
Uses macOS `sips`.
"""
import os
import subprocess
import sys

# Left, top and side of the head in each 512 px thumbnail.
HEADS = {
    "mina": (135, 45, 250),
    "kai": (135, 50, 250),
    "sora": (140, 35, 255),
    "taro": (110, 40, 250),
}


def main():
    out = sys.argv[1]
    for name, (left, top, side) in HEADS.items():
        dst = os.path.join(out, f"{name}.png")
        subprocess.run(
            ["sips", "-c", str(side), str(side), "--cropOffset", str(top), str(left),
             f"out/{name}-refine.png", "--out", dst],
            check=True, capture_output=True,
        )
        subprocess.run(["sips", "-Z", "128", dst], check=True, capture_output=True)


if __name__ == "__main__":
    main()
