# Commit City

A GitHub user's year of contributions, built as an Edo-period Japanese castle
town with `whisker-scene`. Each day of the contribution calendar is one lot on
the main street, the weekday deciding its row: quiet days stay empty or grow a
pine, a cherry tree or a stone lantern, and busier days raise farmhouses,
townhouses, tea houses, merchant houses, storehouses and inns. The busiest day
earns a castle with a moat, the busiest week a walled temple with a pagoda, and
the longest streak a shrine. A ward gate spans the street where each month
begins. Around the town lie a river, rice paddies, cedar woods and a volcano,
generated from a fixed noise seed in `src/land.rs`.

- Drag to move, right-drag or twist two fingers to turn, scroll or pinch to
  zoom. On the web, WASD or the arrow keys move, Q and E turn, R and F zoom.
- Tap a building, or scrub the calendar at the bottom, to see that day.
- **Walk the year** strolls down the street from the first week to the last;
  **Overview** returns to the whole town.
- Type any GitHub username, or one of the presets, to build their town.

## Run

```sh
cargo run -p whisker-cli --bin whisker -- run web --manifest-path examples/commitcity/Cargo.toml
cargo run -p whisker-cli --bin whisker -- run ios --manifest-path examples/commitcity/Cargo.toml
cargo run -p whisker-cli --bin whisker -- run android --manifest-path examples/commitcity/Cargo.toml
```

## Data

Contribution counts come from
`https://whisker.rs/api/github/contributions/{user}`, which reads the public
calendar on the user's GitHub profile, so only public contributions count and
no token is needed. It answers
`{"user": "...", "total": 1234, "days": [{"date": "2026-11-24", "count": 3}, ...]}`.
The presets ship with snapshots in `assets/snapshots/`, so the first town
appears immediately and a preset still renders when the API is unreachable. To
refresh them:

```sh
cd tools
python3 -I snapshot.py itome torvalds dtolnay sindresorhus yyx990803
```

## Models

The buildings, trees, gates and bridges in `assets/models/` were generated with
[Meshy](https://www.meshy.ai/) text-to-3D.
`tools/` holds the scripts that made them: `meshy.py` holds
the prompts and drives the preview and refine tasks, and `optimize.py` shrinks
each result to a 512 px texture and normalizes it so its footprint's longer
side is one unit, centred, with its base at `y = 0`. `optimize.py` uses macOS
`sips` to re-encode textures.

```sh
cd tools
MESHY_API_KEY=... python3 -I meshy.py preview machiya
MESHY_API_KEY=... python3 -I meshy.py wait
MESHY_API_KEY=... python3 -I meshy.py refine machiya
MESHY_API_KEY=... python3 -I meshy.py wait
python3 -I optimize.py out/machiya-refine.glb ../assets/models/machiya.glb
```

A preview costs 20 Meshy credits and a refine 10.

The models are stored with Git LFS; run `git lfs install` before cloning, or
`git lfs pull` after, or the build fails to parse them.
