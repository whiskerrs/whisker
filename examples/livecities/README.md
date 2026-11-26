# Live Cities

A playful live map of people out walking in five cities, built with
`whisker-map` and `whisker-scene`. It opens on a spinning globe; tap a city
bubble or a chip at the bottom to fly there. Everyone walking there is a face
bubble, and bubbles that would overlap gather into a capsule showing two of the
faces and how many are inside. The bubbles bob as their walker walks, wiggle
with a "hi!" when they wave and breathe with a "zzz" while they rest, all with
whisker's CSS keyframe animations. Zoom in to street level and everyone in view
becomes an animated 3D avatar.

- Tap a bubble to focus on that person: the camera moves down to street level
  and they step out of the bubble as an animated 3D avatar, hidden behind the buildings they
  pass. The card shows which street they are on and how far they have walked
  today. **Follow** keeps the camera on them; **Wave** makes them wave back.
- Tap a capsule to zoom into it.
- The moon button switches to the night map; **Globe** returns to the globe.

The people are simulated. Each city's walkers take a random walk along the
streets read from the map's own `transportation` and `transportation_name`
tiles once the city has loaded, so they only walk where there are roads.

## Run

```sh
cargo run -p whisker-cli --bin whisker -- run web --manifest-path examples/livecities/Cargo.toml
cargo run -p whisker-cli --bin whisker -- run ios --manifest-path examples/livecities/Cargo.toml
cargo run -p whisker-cli --bin whisker -- run android --manifest-path examples/livecities/Cargo.toml
cargo run -p whisker-cli --bin whisker -- run macos --manifest-path examples/livecities/Cargo.toml
```

`whisker run web` serves a debug build, which skins the avatars far too slowly
to be smooth; use `whisker build web` to see the web at full speed.

## Map

Tiles, glyphs and sprites come from [OpenFreeMap](https://openfreemap.org/),
with data © [OpenStreetMap](https://www.openstreetmap.org/copyright)
contributors. `assets/style-day.json` and `assets/style-night.json` are
OpenFreeMap's Liberty style recolored by `tools/style.py`:

```sh
python3 -I tools/style.py assets
```

## Avatars

The four avatars in `assets/models/` were generated, rigged and animated with
[Meshy](https://www.meshy.ai/). `tools/meshy.py` holds the prompts and drives
each step; `tools/optimize.py` shrinks the texture to 512 px and names the
clips `walk`, `idle` and `wave`; `tools/faces.py` crops each head from its
refine thumbnail into the bubble icons in `assets/faces/`. `optimize.py` and
`faces.py` use macOS `sips`.

```sh
cd tools
MESHY_API_KEY=... python3 -I meshy.py preview mina
MESHY_API_KEY=... python3 -I meshy.py wait
MESHY_API_KEY=... python3 -I meshy.py refine mina
MESHY_API_KEY=... python3 -I meshy.py wait
MESHY_API_KEY=... python3 -I meshy.py rig mina
MESHY_API_KEY=... python3 -I meshy.py wait
MESHY_API_KEY=... python3 -I meshy.py animate mina
MESHY_API_KEY=... python3 -I meshy.py wait
python3 -I optimize.py out/mina-animate.glb ../assets/models/mina.glb
python3 -I faces.py ../assets/faces
```

An avatar costs about 44 Meshy credits across the four steps.

The models are stored with Git LFS; run `git lfs install` before cloning, or
`git lfs pull` after, or the build fails to parse them.
