"""Recolors OpenFreeMap's Liberty style into the Live Cities day and night styles.

usage: python3 -I style.py OUT_DIR
Writes OUT_DIR/style-day.json and OUT_DIR/style-night.json; tiles, glyphs and sprites stay on OpenFreeMap.
"""
import json
import os
import sys
import urllib.request

LIBERTY = "https://tiles.openfreemap.org/styles/liberty"

DAY = {
    "land": "#f7f1e3",
    "residential": "#f2ead6",
    "park": "#b6ec98",
    "wood": "#a3e086",
    "sand": "#ffe9a8",
    "water": "#7cd4ff",
    "minor": "#ffffff",
    "major": "#ffe27a",
    "motorway": "#ffb067",
    "casing": "#eadfca",
    "path": "#ffffff",
    "rail": "#cbc4e6",
    "building": "#ece4fb",
    "extrusion": ["#fffaf2", "#f3eaff", "#dccfff"],
    "text": "#4b3f72",
    "halo": "#ffffff",
    "water_text": "#2a77b8",
}

NIGHT = {
    "land": "#1d1a3f",
    "residential": "#221f4a",
    "park": "#1f4d47",
    "wood": "#1c4541",
    "sand": "#3d3550",
    "water": "#2c3f9e",
    "minor": "#3c3670",
    "major": "#ff7fc8",
    "motorway": "#ffb067",
    "casing": "#2a2656",
    "path": "#36316a",
    "rail": "#4d4785",
    "building": "#2c2860",
    "extrusion": ["#2f2a66", "#3f3690", "#5a4bc4"],
    "text": "#ece7ff",
    "halo": "#1d1a3f",
    "water_text": "#9fc3ff",
}

DROPPED = {
    "natural_earth",
    "road_one_way_arrow",
    "road_one_way_arrow_opposite",
    "poi_r7",
    "poi_r1",
    "poi_r20",
    "poi_transit",
    "highway-shield-non-us",
    "highway-shield-us-interstate",
    "road_shield_us",
    "airport",
}


def color_for(layer, palette):
    """The palette entry a layer's main color takes, or None to leave it."""
    name, kind = layer["id"], layer["type"]
    if kind == "background":
        return palette["land"]
    if name.startswith(("park", "landcover_grass", "landuse_pitch", "landuse_track", "landuse_cemetery")):
        return palette["park"]
    if name == "landcover_wood":
        return palette["wood"]
    if name == "landcover_sand":
        return palette["sand"]
    if name.startswith(("landuse_residential", "landuse_school", "landuse_hospital", "aeroway_fill")):
        return palette["residential"]
    if name == "water" or name.startswith("waterway"):
        return palette["water"]
    if name == "building":
        return palette["building"]
    if "rail" in name:
        return palette["rail"]
    if name.endswith("casing"):
        return palette["casing"]
    if "path_pedestrian" in name:
        return palette["path"]
    if "motorway" in name:
        return palette["motorway"]
    if any(k in name for k in ("secondary_tertiary", "trunk_primary", "_link")):
        return palette["major"]
    if any(k in name for k in ("minor", "service_track", "street", "aeroway_runway", "aeroway_taxiway")):
        return palette["minor"]
    return None


def recolor(style, palette):
    style = json.loads(json.dumps(style))
    style["sources"].pop("ne2_shaded", None)
    layers = []
    for layer in style["layers"]:
        if layer["id"] in DROPPED:
            continue
        paint = layer.setdefault("paint", {})
        kind = layer["type"]
        color = color_for(layer, palette)
        if kind == "fill-extrusion":
            low, mid, high = palette["extrusion"]
            paint["fill-extrusion-color"] = [
                "interpolate", ["linear"], ["get", "render_height"], 0, low, 30, mid, 120, high,
            ]
            paint["fill-extrusion-opacity"] = 0.92
        elif kind == "symbol":
            water = layer.get("source-layer") in ("water_name", "waterway")
            paint["text-color"] = palette["water_text"] if water else palette["text"]
            paint["text-halo-color"] = palette["halo"]
            paint["text-halo-width"] = 1.5
            font = layer.get("layout", {}).get("text-font")
            if font:
                layer["layout"]["text-font"] = ["Noto Sans Bold"]
        elif color is not None:
            key = {"background": "background-color", "fill": "fill-color", "line": "line-color"}.get(kind)
            if key:
                paint[key] = color
                paint.pop("fill-outline-color", None)
                paint.pop("fill-pattern", None)
                if kind == "fill":
                    paint["fill-opacity"] = 1
        if kind == "line" and layer.get("source-layer") == "boundary":
            paint["line-color"] = palette["rail"]
        layers.append(layer)
    style["layers"] = layers
    return style


def main():
    out = sys.argv[1]
    request = urllib.request.Request(LIBERTY, headers={"User-Agent": "whisker-livecities-style"})
    liberty = json.loads(urllib.request.urlopen(request, timeout=30).read())
    for name, palette in (("day", DAY), ("night", NIGHT)):
        path = os.path.join(out, f"style-{name}.json")
        with open(path, "w") as f:
            json.dump(recolor(liberty, palette), f, separators=(",", ":"))
        print(path)


if __name__ == "__main__":
    main()
