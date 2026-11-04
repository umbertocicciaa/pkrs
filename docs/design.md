# pkrs — Engine Design Document (MVP v1)

> Status: **Approved** (see [Review & sign-off](#review--sign-off)) · Tracks issue [#1](https://github.com/umbertocicciaa/pkrs/issues/1)
>
> This document settles the architecture of `pkrs` before code is written. Each section is meant
> to be cut directly into implementation issues (see [§16 Roadmap](#16-roadmap--milestones)).

## Decisions at a glance

| Topic | Decision |
|---|---|
| Base stack | **macroquad** (window, loop, 2D rendering) + **kira** (audio) + **gilrs** (gamepad) |
| Architecture | **Game state stack + plain structs** (no ECS) |
| External dependencies | **Ports & Adapters + Facade**: every third-party runtime crate sits behind an engine-owned trait (port), implemented by an adapter, and exposed to game code through a subsystem facade (see [§4.5](#45-external-dependency-isolation-ports-adapters--facades)) |
| Workspace | **Multiple focused crates** (see [§4.3](#43-crate-layout)) |
| Data format | **RON** via `serde` |
| Maps | **Tiled** (`.tmx`) via the `tiled` crate |
| Scripting | **Lua 5.4, vendored**, embedded via `mlua` |
| Plugin hooks | **Priority-ordered chain**: handlers can modify values and cancel events |
| Versioning | **Semver engine** pinned in `pkrs.toml` + separate integer **scripting API level** |
| Internal resolution | **Per project**, default **240×160**, integer-scaled and letterboxed |
| Saves | **RON with a version header**, step-by-step migrations, write-temp-then-rename |
| Distribution | **Prebuilt runtime per OS + a single `.pak` archive** per game, no Rust toolchain needed |
| v1 platforms | **Linux, Windows, macOS only** (WASM dropped for v1; **Android** is a post-v1 target, e.g. handhelds) |
| Shaders (v1+) | **Data-driven custom materials** on tiles/sprites (animated tiles, day/night tints, translucent water) via a small shader/material API in `pkrs-render` |
| Plugins (v1) | **Lua only**; **native Rust plugins** are a planned extension for authors who want performance or engine-adjacent hooks |
| Multiplayer | **Out of scope for v1**, but battle/world/save boundaries are shaped so a future **listen-server or dedicated server** binary can share verification and replay rules |

---

## 0. Build vs. reuse

The first question: *what do we actually need to build?*

### 0.1 Candidates

| Option | What we get | What we'd still build | Verdict |
|---|---|---|---|
| **Bevy** (ship pkrs as a Bevy plugin set) | Renderer, ECS, asset server, audio, input, hot reload, large ecosystem | Game layer, Lua bridge | ❌ ECS-first design is overkill for a turn-based game with few entities. Frequent breaking releases mean constant migration churn for every pkrs game. Heavy compile times. Bridging Lua into Bevy's scheduler/world borrows is awkward. |
| **macroquad** | Window, main loop, 2D batched rendering, render targets, textures, basic input, file loading, GLSL materials | Game layer, audio (via kira), gamepad (via gilrs), custom shader pipeline (§5.6) | ✅ **Chosen.** |
| **ggez** | Similar to macroquad, built on wgpu | Same as macroquad | ❌ Slower release cadence. No clear advantage over macroquad for this genre. |
| **winit + wgpu + kira** | Maximum control | Everything: sprite batcher, render pipeline, text, texture management | ❌ Most code for the least gain. A GBA-style renderer doesn't need custom GPU pipelines in v1. |

### 0.2 Why macroquad

- **Matches the genre.** Pixel-perfect 2D at a tiny internal resolution needs a render target, nearest-neighbour filtering and a batched `draw_texture_ex`. macroquad gives us exactly that.
- **Cross-platform with no system deps.** Builds on Linux, Windows and macOS with plain `cargo build` (plus the usual X11/ALSA dev headers on Linux). v1 targets desktop only; mobile (Android) is evaluated after the desktop MVP is stable.
- **Small surface.** Fast compile times and an API that is easy to wrap behind our own traits.
- **We own the game layer**, which we need anyway: state stack, grid movement, battles and scripting are all pkrs-specific.

Supplements:
- **kira** for audio. macroquad's audio is minimal: no crossfades, tweens or clock-synced transitions. We disable macroquad's `audio` feature.
- **gilrs** for gamepads. macroquad has no gamepad support, and gilrs is polled once per frame alongside macroquad's keyboard state.

### 0.3 What we'd lose by switching later

macroquad, like every other third-party runtime dependency, sits behind engine-owned **ports** (traits) implemented by **adapters** and exposed through subsystem **facades** (§4.5). Game code, Lua and battle logic never touch macroquad types. A later move to wgpu/winit (e.g. for shaders, or multi-window tooling) would then cost:
- Writing new adapters for the `PlatformBackend` and `RenderBackend` ports (window, loop, sprite batch, render target, materials).
- Writing a new file I/O adapter used by `pkrs-assets` (already routed through our own VFS, so this is small).
- **Not** the facades, the state stack, world, battle, scripting, data or save code, nor the main loop in `pkrs-runtime`, which only talks to the `Platform` facade.

Known macroquad limitations we accept for v1: a **global context** (single window, main-thread only). Shader flexibility is limited compared to raw wgpu, but macroquad's material system is enough for v1 custom tile/sprite effects; we wrap it behind our own material API so a backend swap does not rewrite game projects (§5.6).

### 0.4 Lua compatibility check

- `mlua` with features `lua54` + `vendored` compiles the Lua C sources with `cc`. That works on Linux (gcc/clang), Windows (MSVC) and macOS (clang) with no system Lua.
- mlua and macroquad don't interact. The Lua state lives on the main thread (macroquad is main-thread anyway), so we **don't** need mlua's `send` feature.
- **Desktop-first:** vendored Lua + macroquad is validated on Linux, Windows and macOS. **WASM is not a v1 target** (dropped; see §17). **Android** may reuse the same runtime later with a different platform backend; we avoid design choices that assume web-only or single-player-only forever.

### 0.5 Engine vs. game project split

```mermaid
flowchart LR
  subgraph Engine["Engine (Rust, shipped once per version)"]
    CLI["pkrs CLI<br/>new · run · check · build · upgrade"]
    RT["pkrs-runtime<br/>prebuilt player binary per OS"]
  end
  subgraph Project["Game project (no Rust)"]
    T["pkrs.toml"]
    A["assets/"]
    D["data/ (RON)"]
    M["maps/ (Tiled)"]
    S["scripts/ (Lua)"]
    P["plugins/ (Lua)"]
  end
  CLI -- "run: launches runtime on folder" --> RT
  CLI -- "build: packs .pak + copies runtime" --> Dist["dist/&lt;os&gt;/MyGame"]
  RT -- "loads" --> Project
```

- **Engine** = Rust code: the `pkrs` CLI plus the `pkrs-runtime` player binary. It's installed once (e.g. `cargo install pkrs-cli`, or a downloaded release), and the CLI fetches and caches prebuilt runtimes per OS and version.
- **Game project** = data only: TOML, RON, Tiled maps, images, audio and Lua. **Making a game never requires a Rust toolchain.**
- Anything a game needs that the engine doesn't provide is added through **Lua plugins** (§9), never by forking the engine.

---

## 1. Scope & non-goals

### 1.1 In scope (v1)
- **Genre:** Gen 1–3 style creature-collecting RPGs (GBA-era feel), single-player.
- **Platforms (required for v1):** Linux (x86_64), Windows (x86_64), macOS (x86_64 + arm64).
- **Platforms (post-v1, planned):** Android (e.g. ARM64 handhelds). WASM and other targets are not planned for the first release.
- **Game size:** the same engine handles everything from a **one-shot story** (one map, a handful of scenes, maybe no battles) to a **full fan game** (multiple regions, full creature roster, story, post-game).
- Top-down tile overworld, grid movement, turn-based 1v1 battles (with doubles possible as a plugin later), menus, dialogue, data-driven content, Lua scripting and plugins, saves.

### 1.2 Non-goals for v1
- 3D rendering or perspective cameras (see §1.4 for how we avoid blocking a far-future 3D map mode).
- **Playable multiplayer** (online battles, trading, co-op overworld). v1 is single-player only, but §4.4 records the constraints we follow so multiplayer does not require a rewrite later.
- An **in-engine editor GUI**. v1 uses **Tiled** for maps and plain text editors for RON/Lua. The asset pipeline and map format are chosen so an internal editor can replace Tiled later without breaking projects.
- **In-engine data authoring** (visual editors for moves, abilities, species, …). Content stays in RON files for v1.
- Mobile and console targets in v1 (Android is explicitly **post-v1**, not abandoned).
- **Native-Rust plugins** in v1. Plugins ship as **Lua** only; Rust dynamic plugins are a later, optional path for advanced authors (§9.9).
- Shipping any third-party franchise IP (names, characters, sprites, music, data or trademarked mechanic names). See §1.3.

### 1.3 IP policy
The engine is generic. The repo contains **no** third-party franchise names, trademarks, sprites, music or data, and the docs do not name any commercial creature-collecting franchise. Engine terminology is neutral ("creature", "species", "move", "trainer"). Templates and demo projects use original or CC-licensed assets, with licenses recorded in each asset folder (`LICENSES.md`).

### 1.4 Long-term product direction (avoid technical debt)

v1 is **write-oriented**: authors edit Tiled maps, RON data and Lua in external tools. Long term, pkrs should become **click-oriented** as well: integrated (or bundled) tooling for maps, encounters, dialogue flow and game data, without forcing everyone off text files on day one.

Design rules so that future does not fight v1:

| Future capability | What we do in v1 |
|---|---|
| **Internal map editor** (replace Tiled) | Keep Tiled as interchange: load/save `.tmx`/`.tsx`, document layer/property conventions in code (`pkrs-world`), store no editor-only secrets in the binary map model. |
| **3D / perspective maps** (far future) | Map model separates **logical grid** (collision, warps, encounters) from **visual layers**. v1 visuals are 2D tiles; logical data stays backend-agnostic. |
| **Game data editor** | RON schemas in `pkrs-data` are the source of truth; validation is shared by `pkrs check` and runtime. A future GUI is a front-end on the same schemas, not a parallel format. |
| **Click-first workflows** | CLI and file layout stay stable; future UI generates the same `pkrs.toml`, `data/`, `maps/` tree. |

---

## 2. Project model & `pkrs` CLI

### 2.1 Project layout

```
my-game/
  pkrs.toml          # manifest (see 2.2)
  assets/
    sprites/         # PNG sprite sheets + optional .ron animation descriptors
    tilesets/        # PNG + Tiled .tsx
    audio/bgm/       # OGG
    audio/sfx/       # OGG/WAV
    fonts/           # bitmap fonts (PNG + .ron glyph metrics) or BMFont .fnt
    ui/              # window skins, cursors, icons
    shaders/         # optional GLSL for custom tile/sprite materials (§5.6)
  data/
    species/*.ron
    moves/*.ron
    items/*.ron
    abilities/*.ron
    types.ron        # type chart
    trainers/*.ron
    encounters/*.ron
    text/<lang>/*.ron   # dialogue/strings (localizable)
  maps/
    *.tmx            # Tiled maps
    world.ron        # map connections (or Tiled .world file)
  scripts/
    main.lua         # optional game-level init (new game setup, global events)
    maps/<map>.lua   # per-map events
    cutscenes/*.lua
  plugins/
    <plugin-name>/
      plugin.toml
      init.lua
      ...
  saves/             # dev-only saves when using `pkrs run` (git-ignored)
```

Every folder except `pkrs.toml` is optional, so a one-shot story can contain just a map, a tileset, a sprite sheet and one script.

### 2.2 `pkrs.toml`

```toml
[game]
name        = "Lighthouse Tale"
id          = "com.example.lighthouse"   # used for save/config paths and the macOS bundle id
version     = "0.1.0"
authors     = ["Jane Doe"]

[engine]
version     = "0.3"          # semver requirement on the engine/runtime
api         = 1              # scripting API level the game scripts target

[display]
width       = 240            # internal resolution (default 240x160)
height      = 160
scale       = 3              # initial window scale
fullscreen  = false

[start]
map         = "lighthouse_1f"
position    = [5, 7]
facing      = "down"
script      = "scripts/main.lua"   # optional

[features]                   # engine subsystems the game uses
battles     = true           # false for pure story games, which skips loading battle data
party       = true

[plugins]
enabled     = ["resonance", "day-night"]   # load order hint; final order is resolved (§9.4)

[locale]
default     = "en"
available   = ["en", "it"]

[input]                      # default bindings (§10): action = [key names]; players rebind per-user (§14.3)
confirm     = ["z", "enter"]
cancel      = ["x", "backspace"]
"resonance:toggle" = ["r"]   # plugin actions are namespaced `plugin:action`
```

**Loading & validation (`pkrs_core::manifest`).** The manifest model lives in `pkrs-core` so the runtime and the CLI share it. Loading has two stages: (1) TOML parsing, value types and unknown keys (typos are errors), reported with the field path and line/column; (2) validation of ranges, id formats and semver, which reports *every* problem at once, each with its dotted field path (`display.scale`, `plugins.enabled[1]`). Only `[game]` (`name`, `id`), `[engine]` (`version`, `api`) and `[start]` (`map`, `position`) are required. Defaults: `game.version = "0.1.0"`, `display` 240×160 at scale 3 windowed, `start.facing = "down"`, all `features` on, no plugins, `locale.default = "en"` with `available = [default]`, no `[input]` overrides. Rules: `game.id` is dot-separated ASCII segments; `engine.api ≥ 1`; `display.width/height` in 1–4096 and `scale` in 1–16; `start.map` is a content id; `start.position` is non-negative; `start.script` is a project-relative `.lua` path with no `..`; plugin ids are lowercase with `_`/`-`; locale tags are BCP-47-style and `available` must contain `default`; lists have no duplicates.

### 2.3 CLI commands

| Command | Purpose |
|---|---|
| `pkrs new <name> [--template one-shot\|full-game]` | Scaffold a project from an embedded template. Pins `engine.version` to the CLI's version. |
| `pkrs run [--release] [--map <id>] [--warp x,y]` | Run the project from loose files with dev features: script hot-reload, debug overlay (F3), verbose logs. |
| `pkrs check` | Validate `pkrs.toml`, all RON data (schema + cross-references), Tiled maps (required layers/properties), Lua syntax, plugin manifests and dependency graph. Non-zero exit on errors, so it's CI-friendly. |
| `pkrs build --target <linux\|windows\|macos\|all> [--out dist/]` | Run `check`, pack the project into `game.pak`, then fetch the matching **prebuilt runtime** and assemble a distributable (§14). Works from any host OS. |
| `pkrs upgrade [--to <version>]` | Bump `engine.version`, run project migrations (e.g. renamed manifest keys), and report scripting-API deprecations. |
| `pkrs plugin new <name>` | Scaffold a plugin folder with a manifest and hook examples. |

### 2.4 Templates
- **one-shot**: one small map, the player, two NPCs, a cutscene with dialogue and choices, an ending screen. `features.battles = false`. Shows that no battle data is required.
- **full-game**: two connected maps plus an interior, a starter-selection script, three original species, eight moves, a type chart, items, one trainer battle, wild encounters, a party/bag/save menu and one example plugin.

Templates are embedded in the CLI binary with `include_dir`, so `pkrs new` works offline.

### 2.5 Engine version pinning & upgrades
- `engine.version` is a semver requirement. `pkrs run`/`build` pick the newest installed or downloadable runtime that satisfies it.
- While pkrs is `0.x`, minor bumps may break things. From 1.0, normal semver applies.
- `engine.api` (scripting API level) is **independent** of engine semver. See §9.7.
- `pkrs upgrade` is the only command that edits `pkrs.toml`. It prints a changelog of anything that needs manual attention.

---

## 3. Core loop & time

### 3.1 Fixed timestep (*Fix Your Timestep*)

The windowing backend (macroquad in v1) drives the outer loop, but `pkrs-runtime` only sees the `Platform` facade (§4.5). Inside it we run a fixed-step accumulator:

```rust
const DT: f64 = 1.0 / 60.0;          // simulation tick (60 Hz, matches GBA-era timing)
const MAX_FRAME: f64 = 0.25;         // clamp: avoid the "spiral of death" after a stall

let mut acc = 0.0;
loop {
    let frame = platform.frame_time().min(MAX_FRAME);
    acc += frame;

    input.poll(&mut platform);       // keyboard + gamepad adapters, once per rendered frame
    while acc >= DT {
        input.begin_tick();          // edge detection is per tick (pressed/released)
        game.update(DT, &mut ctx);   // state stack, scripts, world, battle
        acc -= DT;
    }
    let alpha = acc / DT;            // interpolation factor
    game.render(alpha, &mut renderer);
    platform.next_frame().await;
}
```

- **Logic runs at exactly 60 Hz** regardless of display rate, so walk speed, animation timing and script `wait(frames)` are deterministic.
- **Rendering interpolates** positions between the previous and current tick with `alpha`. Each movable thing stores `prev_pos` and `pos`. This keeps 120/144 Hz displays smooth.
- Script waits are expressed in **ticks** (`wait(30)` = half a second), never in wall-clock time.

### 3.2 Frame pacing
- VSync on by default (macroquad's default swap interval). A config option allows an uncapped frame rate for debugging.
- Slow frames: accumulated time is clamped to `MAX_FRAME`, so the game slows down instead of running dozens of catch-up ticks.
- The window losing focus pauses the simulation (configurable) and ducks audio.
- Policy lives in the `Platform` facade (`pkrs-platform`): `frame_time()` is clamped to `MAX_FRAME` and reports `0` while paused by focus loss (`PlatformConfig::pause_on_focus_lost`), and focus changes surface as `PlatformEvent::FocusLost` / `FocusGained`. The macroquad adapter derives focus from miniquad's minimize/restore events, which fire on focus changes on X11, macOS and wasm; **miniquad does not report focus on Windows**, so there the window is treated as always focused until the adapter gains a native hook.

### 3.3 Deterministic update order
Within a tick:
1. Input → action state (§10).
2. Script scheduler: resume coroutines whose wait conditions are met (§9.3).
3. State stack: `top.update(ctx)`. Lower states only receive `update_background` if they opt in (e.g. overworld water animation under a dialogue box).
4. Deferred state transitions (push/pop/replace) are applied **after** the update, never mid-update.
5. Event queue flush: engine events are dispatched to Lua listeners in emission order.
6. Audio commands are flushed to the `AudioBackend` port (kira adapter in v1).

The game RNG is a seeded `rand_pcg::Pcg64` stored in the save. Battles get their own sub-seed (§7.2).

---

## 4. Architecture

### 4.1 State stack + plain structs (no ECS)

**Justification:** a creature-collecting RPG has a few dozen entities per map at most, its logic is overwhelmingly **modal** (overworld, battle, menu, dialogue), and battles are turn-based. ECS strengths (cache-friendly iteration over thousands of entities, parallel systems) don't pay off here, while ECS costs (indirection, borrow juggling, opaque data for Lua) do. Plain structs map 1:1 to Lua userdata and to save data.

```rust
pub trait GameState {
    fn id(&self) -> &'static str;
    fn on_enter(&mut self, ctx: &mut Ctx) {}
    fn on_exit(&mut self, ctx: &mut Ctx) {}
    fn on_resume(&mut self, ctx: &mut Ctx) {}          // the state above it was popped
    fn update(&mut self, ctx: &mut Ctx) -> Transition;
    fn update_background(&mut self, _ctx: &mut Ctx) {} // opt-in, when not on top
    fn render(&self, r: &mut Renderer, alpha: f32);
    fn is_opaque(&self) -> bool { true }                // false: render states below first
}

pub enum Transition { None, Push(Box<dyn GameState>), Pop, Replace(Box<dyn GameState>), PopTo(&'static str), Quit }
```

```mermaid
stateDiagram-v2
  direction LR
  [*] --> Title
  Title --> Overworld: New game / Continue
  Overworld --> Dialogue: push (talk / script say)
  Dialogue --> Overworld: pop
  Overworld --> Menu: push (Start)
  Menu --> Party: push
  Party --> Menu: pop
  Menu --> Overworld: pop
  Overworld --> Battle: push (encounter / trainer)
  Battle --> Dialogue: push (battle text)
  Dialogue --> Battle: pop
  Battle --> Overworld: pop (result → script)
```

- **Input and update are routed to the top state only.** Rendering walks down to the nearest opaque state, then draws upward, so a dialogue box draws over the overworld.
- Built-in states: `Title`, `Overworld`, `Dialogue`, `Choice`, `Menu`, `Party`, `Bag`, `Summary`, `Battle`, `Transition` (fades and wipes), `SaveScreen`.
- **Lua-defined states:** plugins can register new UI screens as states (§9.5). They implement the same lifecycle through a Lua table.

### 4.2 Shared context
`Ctx` is a struct of borrowed services handed to each state: `assets`, `audio`, `input`, `script`, `data` (immutable content database), `world_state` (flags, variables, party, bag: the save-relevant state), `rng`, `events` (queue). Each service is a **facade** (§4.5); none of them exposes a third-party type. There are **no globals**, apart from the macroquad context, which stays hidden inside the macroquad adapters of `pkrs-platform` and `pkrs-render`.

### 4.3 Crate layout

As decided, a multi-crate workspace. Each crate exists to enforce a **dependency boundary**, and the justification is listed per crate.

```
crates/
  pkrs-core       # shared types: ids, Direction, GridPos, Ctx traits, events, errors, GameRng port
  pkrs-platform   # Platform facade + ports (window, loop, keyboard, gamepad, file I/O, user paths); macroquad/gilrs/directories adapters
  pkrs-assets     # Assets facade: VFS (loose folder | .pak), handles, caching; zip + notify adapters
  pkrs-data       # RON schemas (species, moves, items…), loading, validation, cross-ref checks
  pkrs-render     # Renderer facade + RenderBackend port: render target, sprite batch, atlases, layers, camera, text, materials; macroquad adapter
  pkrs-audio      # Audio facade + AudioBackend port: BGM/SFX, crossfades, ducking; kira adapter
  pkrs-world      # map model, collision, grid movement, warps, connections, NPCs; MapImporter port + Tiled adapter
  pkrs-battle     # PURE battle logic: no macroquad, no mlua, no I/O; deterministic & unit-tested
  pkrs-script     # mlua runtime, sandbox, Lua API bindings, coroutine scheduler, plugin loader
  pkrs-save       # save model, versioning, migrations, atomic writes, per-OS paths
  pkrs-ui         # state-stack UI states: dialogue, menus, window skins (uses render + input)
  pkrs-runtime    # BINARY: the game player; wires everything; state stack; main loop
  pkrs-cli        # BINARY: `pkrs` command: new/run/check/build/upgrade/plugin
```

```mermaid
flowchart TD
  core[pkrs-core]
  platform[pkrs-platform] --> core
  assets[pkrs-assets] --> core
  data[pkrs-data] --> core
  render[pkrs-render] --> platform & assets
  audio[pkrs-audio] --> assets
  world[pkrs-world] --> data & assets
  battle[pkrs-battle] --> core & data
  save[pkrs-save] --> core & data
  ui[pkrs-ui] --> render & platform
  script[pkrs-script] --> world & battle & data & save & ui & audio
  runtime[pkrs-runtime] --> script & render & audio & ui & save & world & platform
  cli[pkrs-cli] --> data & assets & world & script
```

Key boundaries:
- **`pkrs-battle` must not depend on macroquad, mlua or any I/O.** It exposes a `BattleHooks` trait, and `pkrs-script` implements that trait by calling Lua. Pure logic stays unit-testable and deterministic.
- **Every third-party runtime crate is imported by exactly one owning crate**, and only inside its adapter module (§4.5). Only `pkrs-platform` and `pkrs-render` import macroquad, which keeps a future backend swap small (§0.3).
- `pkrs-cli` reuses `pkrs-data`, `pkrs-world` and `pkrs-script` for `pkrs check` (it parses Lua without running the game).

### 4.4 Multiplayer-ready boundaries (future, not v1)

Multiplayer is not implemented in v1, but core crates are shaped so a later milestone can add:

- **`pkrs-server` (future binary):** authoritative simulation for linked battles (and eventually overworld sync). Built from the same `pkrs-battle` + `pkrs-world` logic as the client, with no macroquad/mlua in the hot path unless hooks are explicitly enabled.
- **Client modes:** **host** (embedded listen server + local player) or **connect** to a remote server process. Games ship the client runtime only; server is an optional second binary produced by `pkrs build --server` (exact packaging TBD).
- **Integrity & verification:** battles use a **deterministic event log** (`BattleEvent` stream + inputs + config hash). Two peers (or a spectator) can re-run the same seed and inputs to verify outcomes. Single-player uses a **local RNG policy**; networked battles use a **authoritative server RNG policy** (§7.2) so replays and anti-cheat rules are defined once.
- **Save model:** overworld flags/variables and per-map event state remain in the save (§6.5); server authoritative state for battles is separate from the long-lived save file.

### 4.5 External dependency isolation (Ports, Adapters & Facades)

**Decided:** third-party crates are wrapped with the **Ports & Adapters** pattern (hexagonal architecture), combined with a **Facade** per subsystem. This is the standard way to keep an engine core independent of vendors: GoF *Adapter* converts a library's interface into the one the engine expects, GoF *Facade* gives game code a small, stable entry point over a whole subsystem, and the *port* (an engine-owned trait) is what makes adapters swappable and testable (*Dependency Inversion*).

```mermaid
flowchart LR
  subgraph Game["Game code (states, world, battle, Lua bindings)"]
    G[uses facades only]
  end
  subgraph Facades["Facades (stable engine API)"]
    FP[Platform] & FR[Renderer] & FA[Audio] & FI[Input] & FS[Assets]
  end
  subgraph Ports["Ports (engine-owned traits)"]
    P1[PlatformBackend] & P2[RenderBackend] & P3[AudioBackend] & P4[GamepadSource] & P5[Vfs / FileWatcher]
  end
  subgraph Adapters["Adapters (one module per vendor crate)"]
    A1[MacroquadPlatform] & A2[MacroquadRenderer] & A3[KiraAudio] & A4[GilrsGamepad] & A5[ZipPak / NotifyWatcher]
    N[Null* adapters for tests & --headless]
  end
  G --> Facades --> Ports
  A1 -.implements.-> P1
  A2 -.implements.-> P2
  A3 -.implements.-> P3
  A4 -.implements.-> P4
  A5 -.implements.-> P5
  N -.implements.-> Ports
```

**Roles:**
- **Port** — a trait owned by pkrs, expressed in engine vocabulary only (`TextureId`, `SoundId`, `GridPos`, our error enums). It defines the *minimum* the engine needs from a backend, not a mirror of the vendor API.
- **Adapter** — implements one port on top of one vendor crate (`MacroquadRenderer: RenderBackend`, `KiraAudio: AudioBackend`, …). It lives in a `backend/<vendor>.rs` module of the owning crate, behind a cargo feature, and is the **only** place that vendor's types appear. It converts vendor errors into the crate's `thiserror` enum.
- **Facade** — the public, game-facing service (`Renderer`, `Audio`, `Input`, `Assets`, `Platform`) that holds a port implementation and adds engine policy on top: batching, layers and pixel-perfect scaling in `Renderer`; crossfades, ducking and the per-tick command queue in `Audio`; action mapping and buffering in `Input`. These facades are what `Ctx` hands to states (§4.2) and what the Lua API binds to (§9.1).
- **Null adapters** — `NullRenderer`, `NullAudio`, `NullGamepad`, `MemoryVfs` implement the same ports, giving headless CI and unit tests for free (§15.2, §17 #8).

**Dependency map (v1):**

| Vendor crate | Port | Adapter | Facade seen by game code | Owning crate |
|---|---|---|---|---|
| `macroquad` (window, loop, keyboard, file I/O) | `PlatformBackend` | `MacroquadPlatform` | `Platform`, `Input` | `pkrs-platform` |
| `macroquad` (GL, textures, materials) | `RenderBackend` | `MacroquadRenderer` | `Renderer` | `pkrs-render` |
| `gilrs` | `GamepadSource` | `GilrsGamepad` | `Input` | `pkrs-platform` |
| `directories` | `UserPaths` | `DirectoriesPaths` | `Platform::paths()` | `pkrs-platform` |
| `kira` | `AudioBackend` | `KiraAudio` | `Audio` | `pkrs-audio` |
| `zip` | `Vfs` | `PakVfs` (and `FolderVfs`) | `Assets` | `pkrs-assets` |
| `notify` | `FileWatcher` | `NotifyWatcher` | `Assets` hot reload | `pkrs-assets` |
| `tiled` | `MapImporter` | `TiledImporter` | `pkrs-world::Map` model | `pkrs-world` |
| `mlua` | `BattleHooks` (from `pkrs-battle`) + `ScriptHost` | `LuaScriptHost`, `LuaBattleHooks` | `ScriptHost` | `pkrs-script` |
| `rand_pcg` | `GameRng` | `Pcg64Rng` | `ctx.rng` | `pkrs-core` |
| `toml` | manifest codec in `pkrs-core` | `manifest::toml_codec` | `ProjectManifest` | `pkrs-core` |
| `semver` | — (engine newtypes) | `Version`, `VersionReq` | `ProjectManifest.engine`, plugin versions (§9.4) | `pkrs-core` |
| `ron` | codec functions in `pkrs-data` / `pkrs-save` | `RonCodec` | typed structs, `pkrs_save::Value` | `pkrs-data`, `pkrs-save` |
| `texture_packer` | `AtlasPacker` | `TexturePackerAdapter` | `pkrs build` pipeline | `pkrs-cli` |

**Not wrapped (allowed directly), and why:** `serde` (derive framework; it *is* the abstraction), `serde_path_to_error` (serde helper that only adds the failing field path to errors; never in public APIs), `thiserror` (compile-time only), `tracing` (already a logging facade), `anyhow`, `miette`, `include_dir` (binaries only, never in library APIs), `insta` (dev-only). Adding a new crate to this list requires a note in this section.

**Rules:**
1. **No vendor type in any public signature** of a pkrs library crate. Facades and ports use engine types only.
2. **One owning crate per vendor crate**, enforced in CI with `cargo deny` `[bans]` using `wrappers = [...]` (e.g. `macroquad` may only be a dependency of `pkrs-platform` and `pkrs-render`).
3. **Ports are minimal and engine-shaped.** Grow them when a feature needs it; never expose a vendor feature "just in case".
4. **One backend per build**, selected by cargo feature. Facades store the port as `Box<dyn Port>`: call counts are low (hundreds per frame, sprite draws are batched inside the adapter), so dynamic dispatch is acceptable; revisit with profiling, not upfront.
5. **Swapping a vendor = writing an adapter.** No change to facades, game code, Lua API or project files.

---

## 5. Rendering

### 5.1 Pixel-perfect output
- All game rendering goes to a **render target** of `display.width × display.height` (default 240×160) with `FilterMode::Nearest`.
- The final blit to the window uses the **largest integer scale** that fits, centered, with **letterbox** bars (color configurable). An optional "fit" mode (non-integer, still nearest) is available for fullscreen on odd displays.
- Camera positions are rounded to whole internal pixels when drawing, while the interpolated value is kept for smoothness. This avoids sub-pixel shimmer.
- The UI is drawn into the same target at native resolution, so text is pixel-crisp.

### 5.2 Sprites, atlases, batching
- macroquad already batches draw calls that share a texture. We maximize this by packing sprites into **texture atlases**:
  - Dev (`pkrs run`): images are loaded individually. Simple, and supports hot reload.
  - Release (`pkrs build`): the packer builds atlases per category (characters, tilesets, UI) through the `AtlasPacker` port (`texture_packer` adapter, §4.5) and stores rects in the `.pak` index.
- **Layers** (draw order): `Ground → GroundDecor → Entities (y-sorted) → Overhead → Weather/FX → UI → Transition`.
- **Y-sorting:** entities in the `Entities` layer are sorted by their feet Y (tile row, then sub-pixel offset), then by a stable id to avoid flicker.

### 5.3 Camera
- Follows the player's interpolated position, centered on the player tile.
- Clamped to map bounds unless the map is smaller than the screen, in which case it's centered. Connected maps (§6.4) extend the clamp region so walking across a seam doesn't jerk the camera.
- Scriptable: `camera.pan_to(x, y, ticks)`, `camera.follow(npc)`, `camera.shake(...)`.

### 5.4 Sprite animation
- Animation descriptor (RON) per sheet: frame size, and named clips with frames + ticks per frame + loop flag.
- **Direction-aware** clips (`walk_down`, `walk_up`, `walk_left`, `walk_right`; `right` may mirror `left` via `flip_x`).
- Grid walk cycles are synced to movement: one step = half a walk cycle, alternating feet.

### 5.5 Text
- **Bitmap fonts:** BMFont `.fnt` (text format) or our own RON glyph map. Variable-width glyphs are supported. *We build this ourselves because macroquad's TTF rendering blurs at low resolution; a bitmap font renderer is about 200 lines.*
- Dialogue box features: typewriter reveal (configurable speed), inline control codes (`{color:red}`, `{pause:20}`, `{player}`, `{var:rival_name}`), auto-wrap to box width, page breaks, and a "more" arrow.
- All text goes through the localization table (`data/text/<lang>/`).

### 5.6 Custom shaders & materials (creators)

Authors need effects that plain blitting cannot express: **animated tiles** (e.g. flickering lanterns), **day/night colour cycling**, **translucent water**, simple palette swaps. v1 exposes this through **materials**, not raw engine internals:

- **Tile/sprite materials** reference a small built-in shader library (`pkrs/shaders/*.glsl` in the engine) or a **project shader** under `assets/shaders/` (loaded via VFS, validated by `pkrs check`).
- **Material descriptor (RON)** on a tileset, layer, or sprite: `shader`, `uniforms` (scalars, colours, time-driven flags), optional `atlas_region` for multi-tile animations.
- **Uniforms:** engine provides `time_ticks`, `map_origin`, and plugin-registered globals (e.g. day-night phase from a plugin). No arbitrary GPU code from Lua in v1; Lua sets parameters, Rust/materials apply them.
- **API surface:** `render.set_global_uniform(name, value)` from Rust hooks; plugins use Lua wrappers. Custom shaders must declare a **capability profile** (max uniforms, texture slots) so `pkrs check` can reject unsupported combinations.
- **Backend note:** implemented by the `MacroquadRenderer` adapter using macroquad materials in v1; exposed only as `pkrs-render::Material` through the `RenderBackend` port (§4.5), so a wgpu adapter later reuses the same descriptors.

---

## 6. World & maps

### 6.1 Tiled as the map editor (v1)
- Maps are authored in **Tiled** and loaded by the `TiledImporter` adapter (built on the `tiled` crate) into the engine's own map model through the `MapImporter` port (§4.5) (`.tmx` + `.tsx`). No in-engine editor in v1; formats and layer conventions are stable so a future pkrs map editor can read/write the same files (§1.4).
- `pkrs check` validates engine conventions (required layers, property types).
- `.tmj` (JSON) support depends on the `tiled` crate version. See §17.

### 6.2 Layer conventions

| Tiled layer | Kind | Meaning |
|---|---|---|
| `ground`, `ground2…` | Tile layer | Drawn below entities |
| `overhead` | Tile layer | Drawn above entities (tree tops, roofs) |
| `collision` | Tile layer (special tileset) or tile property `solid=true` | Blocked tiles; directional blocking via `block_n/s/e/w` |
| `terrain` | Tile property `terrain=` | `grass`, `water`, `ledge_s`, `ice`, `tall_grass`, … → movement rules + encounter type |
| `objects` | Object layer | `warp`, `npc`, `trigger`, `sign`, `item`, `spawn` objects with custom properties |
| `encounters` | Object layer (rects) | `table = "route1_grass"`, `rate = 20` |

Object properties reference scripts by name: an `npc` with `script = "maps/route1.lua:old_man"`, a `trigger` with `on_enter = "cutscenes/rival_intro"`.

### 6.3 Grid movement
- The player and NPCs occupy one tile. A step takes **16 ticks walking** and **8 ticks running** (configurable per project), with position interpolated over the step.
- **Facing:** tapping a direction turns the player without moving (a tap shorter than N ticks only turns), as in classic GBA-era RPGs.
- **Rules evaluated before a step:** bounds/connection → collision (tile + directional) → entity occupancy → terrain rule:
  - `ledge_<dir>`: can only be crossed in that direction, playing a hop animation over 2 tiles.
  - `water`: requires the `surf` field state (granted by a field action, see §9.5).
  - `ice`: forced slide until blocked.
  - Custom terrains: plugins can register terrain rules (`world.register_terrain("mud", rule_fn)`).
- **Step events:** `on_step` fires after each completed step (encounter roll, triggers, step counters for plugins).

### 6.4 Transitions & connections
- **Warps:** `warp` objects specify `to_map`, `to_x`, `to_y` and optional `facing`/`transition` (`fade`, `none`, `door`). Entering one pushes a `Transition` state, swaps the map, then pops.
- **Seamless connections:** `maps/world.ron` (or Tiled's `.world` file) declares `north/south/east/west` neighbours with an offset. The renderer draws the neighbour's edge strip, and walking past the edge swaps the active map without a fade, as in classic GBA-era RPGs.
- On map change: the old map's Lua event handlers are unloaded, the new map's script is loaded, and BGM crossfades if the track differs (§11).

### 6.5 Save-aware events & persistent map state

Story progression must survive saves: if the player already triggered an event, the world must behave accordingly after load.

- **Global flags & variables** (`game.flag`, `game.var`) live in `WorldState` and serialize with the save (§12).
- **Per-map persistent state** (recommended for one-shot triggers): `MapState` keyed by `map_id`, stored in the save:
  - **Object state:** NPC hidden/moved, item picked up, door opened — keyed by Tiled object `id` or stable `uid` property.
  - **Trigger state:** `trigger` objects with `once = true` record `fired` so `on_enter` does not run again.
  - **Script counters:** optional `game.map_var(map, name)` for map-local variables without polluting global namespace.
- **Engine events** (`map_enter`, `step`, `interact`, …) consult persisted state before running scripts. Scripts should set flags or map state at the end of a branch, not rely on in-memory-only globals.
- **`pkrs check`** warns on triggers marked `once` whose scripts never set a flag or map state (static heuristic where possible).

---

## 7. Battle system

### 7.1 Turn state machine & phases

The high-level loop matches classic turn-based RPGs. Internally, each turn is split into **phases** (similar in spirit to the event ordering used by open-source competitive battle simulators): hooks fire at phase boundaries so gimmicks (once-per-battle transformations, type-shifting mechanics, one-shot signature moves, custom per-turn forms) slot in without new core code for each mechanic.

```mermaid
stateDiagram-v2
  [*] --> Start: on_battle_start
  Start --> SelectActions
  SelectActions --> PreTurn: all sides chose
  PreTurn --> ResolveOrder: on_pre_turn hooks (forme changes, items)
  ResolveOrder --> ExecuteAction
  ExecuteAction --> FaintCheck
  FaintCheck --> ExecuteAction: more actions queued
  FaintCheck --> Replace: a side must switch in
  Replace --> ExecuteAction
  FaintCheck --> EndOfTurn: queue empty
  EndOfTurn --> FaintCheck2: weather/status/residual
  FaintCheck2 --> SelectActions: battle continues
  FaintCheck2 --> End: a side has no usable creatures / run / capture
  End --> [*]: on_battle_end → BattleResult
```

**Phase hooks (API level 1, additive over time):** `on_battle_start`, `on_turn_start`, `on_pre_turn` (before speed calc — forme/item activation), `on_action_order`, per-action substeps (§7.4), `on_mid_turn` (between actions if needed), `on_end_of_turn`, `on_battle_end`. Plugins that introduce a new gimmick typically attach to `on_pre_turn`, `on_before_move`, `on_damage_calc`, and custom actions.

### 7.2 Pure, deterministic core (`pkrs-battle`)

- `Battle::new(setup: BattleSetup, config: BattleConfig, hooks: &mut dyn BattleHooks)` → the battle steps through `step(input) -> Vec<BattleEvent>`.
- **Input** is a set of chosen actions (`Fight{move_slot, target}`, `Switch{slot}`, `Item{id, target}`, `Run`, `Custom{id, payload}`, plus plugin-defined payloads).
- **Output** is an append-only **`BattleEvent` log** (`MoveUsed`, `FormeChanged`, `Damage{…}`, `StatusApplied`, `StatChanged`, `Fainted`, `Message{key, args}`, …). The **UI layer plays events back**; logic never waits on rendering. The same log is used for **replay verification** (§4.4).
- **RNG policies (decided early to avoid debt):**
  - **`RngPolicy::Local`** (default single-player): battle RNG is derived from the save's game RNG (`Pcg64`). Wild encounters and offline trainer battles use this. Lua hooks use `ctx:random(n)`; `math.random` is removed in the sandbox.
  - **`RngPolicy::Authoritative { seed, stream_id }`** (future networked battles): only the server advances the battle RNG; clients submit actions and apply the event log. Clients do not roll damage locally. Single-player and multiplayer code paths share the same `step()` implementation; only the RNG source differs.
  - **`RngPolicy::Fixed { seed }`** (debug, tests, `"Battle Factory"` style challenges): fully pinned seed for reproducible gauntlets.
- **Determinism:** for a given `BattleSetup`, `RngPolicy`, hook registration order and player/AI inputs, `step()` produces an identical event stream. **Seeded replay tests** are mandatory (§15.2).

### 7.3 Data model
- **Stats:** HP/Atk/Def/SpA/SpD/Spe, base stats, IVs/EVs (configurable to disable for simpler games), level, nature-like stat modifiers (data-defined, optional).
- **Types:** a data-defined type list and effectiveness matrix (`data/types.ron`). Any number of types is supported.
- **Damage formula:** a Gen-3 style default, implemented in Rust and exposed as a pipeline: `base → modifiers (STAB, type, crit, random, weather, …) → final`. Each modifier stage is a hook point, so plugins can insert or replace modifiers.
- **Moves as data:** `power`, `accuracy`, `pp`, `type`, `category`, `priority`, `target`, `flags`, plus an `effects` list of **built-in effect primitives** (`Damage`, `ApplyStatus`, `StatStage`, `Heal`, `Recoil`, `Drain`, `MultiHit`, `Weather`, `Protect`, `SwitchOut`, …) with parameters. Moves needing bespoke logic set `script = "plugin_id:function"`.
- **Status effects:** major statuses (sleep, poison, burn, paralysis, freeze, data-defined) and volatile statuses (confusion and similar). Each has phase-specific behaviour expressed as effect primitives or Lua hooks.
- **Formats:** v1 battles are **1v1** in core data (`BattleSetup.side_size = 1`). The internal model uses **N active slots per side** so **double battles** ship as a **default plugin** (`plugins/doubles/` in the full-game template) rather than a core rewrite (§17).

### 7.4 `BattleHooks` trait & hook points

`pkrs-battle` owns a **`BattleHooks` trait**: synchronous callbacks invoked at named phases. **`pkrs-script`** implements the trait by dispatching to Lua (and, in the future, to native Rust plugins). Game Lua never implements the trait directly; it registers named handlers via `battle.on(...)`.

**Design rules:**

- Hooks receive a **`BattleHookCtx`** (read-only view of field, sides, active creatures, turn/phase, RNG accessor policy-aware) plus mutable **payload structs** for that hook (e.g. `DamageCalc`, `ActionMenu`).
- Return **`HookResult<T>`**: `Ok(T)` with optional modifications, or **`Cancel { reason }`** where the hook site supports cancellation.
- Handlers run in **priority order** (§9.4). The battle core records which hook ran for replay/debug (`BattleEvent::HookTrace` in dev builds).

**Rust shape (illustrative — exact types live in `pkrs-battle`):**

```rust
pub trait BattleHooks {
    fn on_battle_start(&mut self, ctx: &BattleHookCtx<'_>) -> Result<(), BattleError> { Ok(()) }
    fn on_turn_start(&mut self, ctx: &BattleHookCtx<'_>) -> Result<(), BattleError> { Ok(()) }
    fn on_pre_turn(&mut self, ctx: &mut PreTurnCtx<'_>) -> Result<(), BattleError> { Ok(()) }
    fn on_action_order(&mut self, ctx: &BattleHookCtx<'_>, order: &mut ActionOrder) -> Result<(), BattleError> { Ok(()) }
    fn on_before_move(&mut self, ctx: &BattleHookCtx<'_>, req: &mut MoveRequest) -> Result<HookResult<()>, BattleError> { Ok(HookResult::Continue) }
    fn on_accuracy(&mut self, ctx: &BattleHookCtx<'_>, acc: &mut AccuracyCalc) -> Result<(), BattleError> { Ok(()) }
    fn on_damage_calc(&mut self, ctx: &BattleHookCtx<'_>, dmg: &mut DamageCalc) -> Result<(), BattleError> { Ok(()) }
    fn on_damage_taken(&mut self, ctx: &BattleHookCtx<'_>, hit: &mut DamageTaken) -> Result<HookResult<()>, BattleError> { Ok(HookResult::Continue) }
    fn on_after_move(&mut self, ctx: &BattleHookCtx<'_>, res: &MoveResult) -> Result<(), BattleError> { Ok(()) }
    fn on_status_apply(&mut self, ctx: &BattleHookCtx<'_>, req: &mut StatusApply) -> Result<HookResult<()>, BattleError> { Ok(HookResult::Continue) }
    fn on_switch_in(&mut self, ctx: &BattleHookCtx<'_>, sw: &mut SwitchIn) -> Result<HookResult<()>, BattleError> { Ok(HookResult::Continue) }
    fn on_switch_out(&mut self, ctx: &BattleHookCtx<'_>, sw: &mut SwitchOut) -> Result<HookResult<()>, BattleError> { Ok(HookResult::Continue) }
    fn on_faint(&mut self, ctx: &BattleHookCtx<'_>, f: &FaintEvent) -> Result<(), BattleError> { Ok(()) }
    fn on_end_of_turn(&mut self, ctx: &BattleHookCtx<'_>) -> Result<(), BattleError> { Ok(()) }
    fn on_battle_end(&mut self, ctx: &BattleHookCtx<'_>, result: &mut BattleResult) -> Result<(), BattleError> { Ok(()) }
    fn on_action_menu(&mut self, ctx: &BattleHookCtx<'_>, menu: &mut ActionMenu) -> Result<(), BattleError> { Ok(()) }
    fn on_custom_action(&mut self, ctx: &mut CustomActionCtx<'_>) -> Result<(), BattleError> { Ok(()) }
}
```

**Lua-facing hook list (API level 1)** mirrors the trait. Additional hooks (e.g. `on_forme_change`, `on_item_consumed`, `on_hit_substitute`) are added additively when mechanics need them; gimmick plugins should not require core forks.

| Hook | Can modify | Can cancel |
|---|---|---|
| `on_battle_start(ctx)` | Field setup (via declared side state) | — |
| `on_turn_start(ctx)` | — | — |
| `on_pre_turn(ctx)` | Formes, volatiles, item triggers before move selection resolves | — |
| `on_action_order(ctx, actions)` | Ordering (priority/speed tiebreaks) | — |
| `on_before_move(ctx, user, move, target)` | Move, target, flags | ✅ (flinch, sleep, …) |
| `on_accuracy(ctx, user, move, target, acc)` | Accuracy | — |
| `on_damage_calc(ctx, user, move, target, dmg_info)` | Base power, modifiers, final damage | — |
| `on_damage_taken(ctx, target, amount, source)` | Amount | ✅ |
| `on_after_move(ctx, user, move, target, result)` | — | — |
| `on_status_apply(ctx, target, status)` | Status | ✅ (immunities) |
| `on_switch_in(ctx, creature)` / `on_switch_out` | — | ✅ (trapping) |
| `on_faint(ctx, creature)` | — | — |
| `on_end_of_turn(ctx)` | — | — |
| `on_battle_end(ctx, result)` | Rewards (exp, money) | — |
| `on_action_menu(ctx, side, options)` | Fight menu entries (transformations, signature moves, …) | — |
| `on_custom_action(ctx, id, payload)` | Executes custom actions | — |

Full Rust docs for `BattleHooks`, contexts and payload types are generated into `docs/rust-api/` alongside `docs/lua-api/`.

### 7.5 Trainer AI & custom profiles

- **Built-in tiers:** `random`, `basic` (type-aware), `smart` (type + stat matchups, switching heuristics) — implemented in Rust for speed and determinism.
- **Trainer profiles (data):** `data/trainers/*.ron` references an **`ai_profile`** id:

```ron
Trainer(
  id: "champion_aria",
  party: [ ... ],
  ai_profile: "profiles/champion_hard",   // or inline profile id
)
```

```ron
// data/trainers/profiles/champion_hard.ron
AiProfile(
  id: "champion_hard",
  tier: Smart,
  script: "scripts/ai/champion.lua",       // optional Lua override
  weights: (aggression: 0.8, switch: 0.6, predict: 0.9),
  allow_items: true,
)
```

- **Lua AI:** when `script` is set, the engine calls `choose_action(view) -> Action` in a sandboxed coroutine with a **read-only battle view** (same information a human would have in that format — configurable per profile for "cheating" boss fights).
- **Plugin AI:** plugins register new profiles (`ai.register_profile("gauntlet:boss", fn)`) or replace behaviour for existing ids.
- **Hard challenges:** profiles combine weights, item usage rules, and full Lua for multi-turn planning (e.g. champion sets). All AI decisions go through the same action input path as the player so replays stay valid.

---

## 8. Data-driven content

### 8.1 RON content
Every piece of content is RON, deserialized with `serde` into `pkrs-data` structs:

```ron
// data/species/emberkit.ron
Species(
  id: "emberkit",
  name: "text:species.emberkit",
  types: ["fire"],
  base_stats: (hp: 45, atk: 52, def: 40, spa: 60, spd: 50, spe: 65),
  abilities: ["kindle"],
  learnset: [ (level: 1, move: "scratch"), (level: 7, move: "ember") ],
  evolutions: [ (into: "blazecat", trigger: Level(16)) ],
  sprites: (front: "sprites/creatures/emberkit_front.png", back: "sprites/creatures/emberkit_back.png", icon: "sprites/icons/emberkit.png"),
  catch_rate: 45, exp_yield: 62, growth: MediumFast,
)
```

```ron
// data/moves/ember.ron
Move(
  id: "ember", name: "text:moves.ember", type: "fire", category: Special,
  power: 40, accuracy: 100, pp: 25, priority: 0, target: SingleOpponent,
  effects: [ Damage, ApplyStatus(status: "burn", chance: 10) ],
)
```

- IDs are strings in the files and are interned into `u32` handles after loading.
- A file can hold one item or a list. Folders are merged, and duplicate IDs are an error unless a plugin explicitly `override`s.

### 8.2 Validation
- **Stage 1 (parse):** serde errors are reported with file, line and column (RON gives positions).
- **Stage 2 (schema):** value ranges, required fields, non-empty learnsets, `accuracy ≤ 100`, …
- **Stage 3 (cross-refs):** every referenced ID (move, type, species, item, text key, sprite path, script function) exists.
- Errors are collected rather than fail-fast, so `pkrs check` prints them all at once, using `miette`-style diagnostics with source snippets.
- The runtime runs the same validation at startup. In release builds, invalid content shows an error screen instead of crashing.

### 8.3 Hot reload (debug only)
`pkrs run` watches the project with `notify`. Changes to data/Lua/textures trigger a reload of the affected subsystem at the next tick boundary. Data reload re-runs validation, and on failure it keeps the old data and shows an overlay error. *Nice-to-have; ships in milestone 6 or later.*

---

## 9. Lua scripting & plugin system

**Decided: Lua 5.4, vendored, embedded via `mlua`.**

### 9.1 What Lua can reach

All Lua APIs live in namespaced global tables, created per API level:

| Module | Surface (examples) |
|---|---|
| `game` | `flag(name)`, `set_flag(name, v)`, `var(name)`, `set_var(name, v)`, `party()`, `bag()`, `money()`, `give_creature(...)`, `give_item(id, n)` |
| `map` | `current()`, `warp(map, x, y, facing)`, `tile(x, y)`, `set_tile(layer, x, y, tile)`, `npc(id)`, `spawn_npc(def)`, `register_terrain(name, fn)`, `var(name)` / `set_var` (map-local, persisted §6.5) |
| `npc` (userdata) | `:move(path)`, `:face(dir)`, `:set_sprite(id)`, `:hide()`, `:pos()` |
| `player` | `pos()`, `face(dir)`, `lock()`, `unlock()`, `set_field_state(name, bool)` |
| `ui` | `say(text)`, `choose({opts})`, `yes_no(text)`, `push_screen(id, args)`, `notify(text)` |
| `camera` | `pan_to`, `follow`, `shake`, `fade_out`, `fade_in` |
| `audio` | `play_bgm(id, {fade=})`, `play_sfx(id)`, `stop_bgm`, `jingle(id)` |
| `battle` | `start_wild(species, level)`, `start_trainer(id)` → result. Inside hooks: the battle `ctx` (creatures, field, `random`, `message`) |
| `data` | Read-only queries (`species(id)`, `move(id)`) + plugin registration (`register_move`, `register_item`, …) |
| `events` | `on(name, fn, {priority=})`, `emit(name, payload)` |
| `log` | `info/warn/error` (routed to `tracing`) |
| `time` | `ticks()`, `wait(n)` (coroutine only) |

**Rust-only:** rendering internals, collision resolution, file I/O, save serialization, asset decoding, the main loop and the battle step engine. Lua influences these through declared hooks, never by direct access.

### 9.2 Events (overworld)
Engine events Lua can subscribe to: `game_start`, `map_enter`, `map_exit`, `step`, `interact(npc|sign|tile)`, `trigger_enter`, `battle_end`, `menu_open`, `save_before`, `load_after`, `tick` (discouraged, but available).

### 9.3 Coroutine cutscenes
Event handlers and NPC scripts run as **Lua coroutines** managed by the `pkrs-script` scheduler. Blocking-looking API calls (`say`, `move`, `wait`, `choose`, `battle.start_*`) yield to the engine with a wait condition (dialogue closed, NPC path finished, N ticks elapsed, battle ended) and resume when it's satisfied. No callback chains:

```lua
-- scripts/maps/lighthouse_1f.lua
function old_keeper(npc)
  npc:face_player()
  if not game.flag("keeper_met") then
    ui.say("text:keeper.hello")
    local yes = ui.yes_no("text:keeper.help_me")
    if yes then
      npc:move({ "up", "up", "left" })
      wait(20)
      ui.say("text:keeper.thanks")
      game.give_item("lamp_oil", 1)
      game.set_flag("keeper_met", true)
    end
  else
    ui.say("text:keeper.again")
  end
end
```

- While a "blocking" script runs, the player is locked by default (`player.lock()` happens automatically for `interact` and triggers).
- Several coroutines can run concurrently (e.g. background NPC wander scripts). They are stepped in deterministic order (creation order).

### 9.4 Plugins

**Layout & manifest:**

```
plugins/resonance/
  plugin.toml
  init.lua
  data/moves/*.ron        # optional: plugin-provided content, merged into the database
  assets/...              # optional: plugin-provided assets (namespaced "resonance:...")
```

```toml
# plugin.toml
[plugin]
id          = "resonance"
name        = "Resonance"
version     = "1.2.0"
api         = 1                 # scripting API level required
description = "Once per battle, a creature can Resonate to empower its primary type."

[dependencies]
"status-pack" = "^1.0"          # other plugins, semver ranges

[load]
after       = ["day-night"]     # soft ordering hints (no dependency)
conflicts   = ["overdrive"]
```

**Registration model:** `init.lua` returns, or calls into, a registration API. Plugins can:
- Subscribe to **battle hooks** (§7.4) and **world events** (§9.2) with a priority.
- Register **content**: moves, abilities, items, statuses, terrains, data effect primitives.
- Register **field actions** (e.g. clearing an obstacle, crossing water, breaking a rock) usable from the party menu or by facing a tile.
- Register **battle actions** (new entries in the fight menu) via `on_action_menu`/`on_custom_action`.
- Register **UI screens** (Lua-defined states, see §9.5).
- Store **plugin save data** in a namespaced table (`plugin.save.<key>`) that's persisted with the save.

**Load order & dependency resolution:**
1. Discover all plugins in `plugins/`. Only those in `pkrs.toml [plugins].enabled` are loaded, and an enabled-but-missing plugin is an error.
2. Validate manifests: API level supported (§9.7), dependency versions satisfied, `conflicts` not both enabled.
3. **Topological sort** on `dependencies` + `load.after`. Ties are broken by order in `enabled`, then by plugin id. Cycles are errors.
4. Plugin data is merged into the content DB in load order. Duplicate content IDs are errors unless the later plugin declares `override = true` on that item (logged).
5. Each `init.lua` runs in load order.

**Hook combination (decided: priority-ordered chain):**
- Handlers run in ascending `priority` (default `0`), then in plugin load order.
- Each handler receives the current value and returns a modified value (or `nil` = unchanged). For cancellable hooks, returning `{ cancel = true, message = "…" }` stops the chain and cancels the action.
- This is **deterministic**: the same plugins and order give the same results, which `pkrs check --plugins` can print as the resolved hook table.

### 9.5 Example: a gimmick written entirely as a plugin — *Resonance*

> Once per battle, a trainer can make one creature **Resonate** for 3 turns: its moves matching its primary type deal 1.5×, and it gets a glowing aura. Implemented with zero engine changes.

```lua
-- plugins/resonance/init.lua
local P = plugin.define()           -- binds to manifest: id, api level, save namespace

local DURATION = 3

-- 1. Add a "Resonate" entry to the player's fight menu, once per battle.
battle.on("on_action_menu", function(ctx, side, options)
  if side.is_player and not ctx.side_state(side, "resonance_used") then
    table.insert(options, { id = "resonance:resonate", label = "text:resonance.menu" })
  end
  return options
end)

-- 2. Execute the custom action (it uses the turn, like a switch would).
battle.on("on_custom_action", function(ctx, id, payload)
  if id ~= "resonance:resonate" then return end
  local mon = payload.creature
  ctx.set_side_state(mon.side, "resonance_used", true)
  ctx.set_volatile(mon, "resonating", { turns = DURATION })
  ctx.message("text:resonance.start", { name = mon.name })
  ctx.play_fx("resonance:aura", mon)          -- plugin asset, played by the battle UI
end)

-- 3. Damage modifier: runs in the damage-calc chain.
battle.on("on_damage_calc", function(ctx, user, move, target, dmg)
  local r = ctx.volatile(user, "resonating")
  if r and move.type == user.types[1] then
    dmg.modifiers:push("resonance", 1.5)
  end
  return dmg
end, { priority = 10 })

-- 4. Count down at end of turn.
battle.on("on_end_of_turn", function(ctx)
  for _, mon in ipairs(ctx.active_creatures()) do
    local r = ctx.volatile(mon, "resonating")
    if r then
      r.turns = r.turns - 1
      if r.turns <= 0 then
        ctx.clear_volatile(mon, "resonating")
        ctx.message("text:resonance.end", { name = mon.name })
      end
    end
  end
end)

return P
```

What this proves: a new **battle action**, a new **damage rule**, a new **volatile status** with a timer, new **UI text** and a new **visual effect**, all without touching `pkrs-battle` or any other crate. `pkrs-battle` only needs the generic custom-action and volatile-state support listed in §7.4, which ship with the core.

**Lua-defined UI screen** (e.g. a custom "Resonance log" menu):

```lua
ui.register_screen("resonance:log", {
  on_enter = function(self, args) self.cursor = 1 end,
  update   = function(self, input) if input.pressed("cancel") then return "pop" end end,
  render   = function(self, gfx) gfx.window(8, 8, 224, 144); gfx.text(16, 16, "text:resonance.title") end,
})
```

### 9.6 Safety
- **Sandbox:** only `base` (minus `dofile`, `loadfile`, `load` with binary chunks, `collectgarbage` options other than `count`), `string`, `table`, `math` (with `math.random`/`randomseed` replaced by the engine RNG) and `utf8` are exposed, plus `coroutine` (used internally). **No `io`, `os`, `package`, `debug`, `require` of native modules.** `require` is replaced by a project-relative loader limited to `scripts/` and the plugin's own folder.
- **Per-plugin environments:** each plugin's chunks run with their own `_ENV` table, so plugins can't clobber each other's globals. They communicate via `events.emit` or explicit exported tables.
- **Errors never crash the engine.** Every Lua call goes through a `pcall` boundary. Errors are logged with `file:line` and a traceback, and shown in a dev overlay in `pkrs run`. In release builds:
  - Failed **event/cutscene** coroutine: aborted, player unlocked, error logged.
  - Failed **battle hook** handler: that handler is skipped for this call (the value passes through unchanged), a warning is logged, and the handler is disabled after N consecutive failures.
- **Saves are never corrupted by scripts:** a save is only written from a consistent state between ticks. If the save serializer finds non-serializable plugin save data (functions, userdata), it skips that key and logs, while the rest of the save still writes.
- **Runaway scripts:** an instruction-count hook (`mlua` `set_hook` with `every_nth_instruction`) aborts a single call that exceeds a budget (e.g. 10M instructions) so infinite loops don't hang the game.
- Memory limit per Lua state (`Lua::set_memory_limit`), configurable.

### 9.7 API stability & versioning
- The **scripting API level** is a single integer (`api = 1, 2, …`) that increments only when the Lua API changes incompatibly.
- Each engine release documents the range of API levels it supports (e.g. engine 0.5 supports `api = 1..=2`).
- When a plugin or game targets an older supported level, the engine exposes **compatibility shims** for that level (renamed functions aliased, old signatures adapted) and logs deprecation warnings once.
- Additive changes (new functions, new hooks, new optional fields) do **not** bump the level.
- `docs/lua-api/` (generated from Rust doc attributes) lists every function with "since API level" and "deprecated in" markers. `pkrs check` warns about deprecated usage it can detect statically.

### 9.8 Performance split
- **Stays in Rust:** rendering, sprite batching, collision and movement rules (the Lua terrain callback fires only on custom terrains), map loading, pathing for `npc:move`, damage pipeline arithmetic, data loading and validation, save I/O.
- **Fine in Lua:** event logic, cutscenes, menu screens, battle hooks (a few calls per action), AI overrides, plugin content.
- Guidance: avoid per-tick Lua callbacks. `events.on("tick")` is available but flagged by `pkrs check` as a performance warning.
- **Hot reload in dev:** on a script change, re-run the map's script file and replace its handlers. Plugin reload re-runs `init.lua` after clearing that plugin's registrations. Running coroutines keep the old code until they finish.

### 9.9 Native Rust plugins (post-v1)

Lua remains the default plugin surface (fast iteration, accessible to non-Rust authors). **Native Rust plugins** are a planned extension for teams that need heavy logic, custom serializers, or server-side hooks:

- **v1:** not shipped; no dynamic loading in release builds.
- **Future model (sketch):** `plugins/my-plugin/` contains `plugin.toml` + a `cdylib` built against a stable **`pkrs-plugin-sdk`** crate exporting the same hook registry as Lua (battle/world/events), plus optional `register_move`-style content from Rust.
- **Safety:** Rust plugins run out-of-process or with a narrow SDK boundary in-process — exact sandbox TBD; they must not break determinism for battles used in verification (§4.4, §7.2).
- **Distribution:** games declare native plugins in `pkrs.toml`; `pkrs build` packs signed/ hashed binaries per target OS. Lua and Rust plugins can coexist in one project.

---

## 10. Input

- **Action layer:** the game reads actions, never keys: `Up, Down, Left, Right, Confirm, Cancel, Menu, Run, Select` plus plugin-registered actions (`input.register_action("resonance:toggle", default_keys)`).
- **Devices:** keyboard via the `MacroquadPlatform` adapter, gamepad via the `GilrsGamepad` adapter (standard mapping; the first connected pad by default). Both feed the same action state in the `Input` facade (§4.5).
- **Rebinding:** bindings are stored in the per-user config file (§14.3), not in the save. A built-in "Controls" options screen and defaults come from `pkrs.toml [input]`.
- **Per action and tick:** `down`, `pressed` (edge), `released`, `held_ticks`. Pressed edges are computed per **simulation tick**, so no press is lost or double-counted when multiple ticks run in one frame.
- **Buffered input:**
  - Movement: a direction pressed during the last N ticks of a step is queued, giving continuous walking without stutter.
  - Menus: key repeat for held directions (initial delay plus repeat rate), and a press buffered through screen transitions so it applies to the new top state.
- **Text input** (naming screens): a dedicated on-screen keyboard state, so it also works with a gamepad.

---

## 11. Audio

All behaviour below lives in the `Audio` facade; the `KiraAudio` adapter implements the `AudioBackend` port (§4.5).

- **BGM:** OGG streaming, with **loop points** supported via metadata in the audio manifest (`assets/audio/bgm/index.ron`: `loop_start`, `loop_end` in samples), because many tracks have intros.
- **Crossfade on map change** when the BGM id differs (default 30 ticks). Same id: keep playing.
- **Battle transitions:** battle start pauses the overworld BGM, keeping its position, and plays the battle intro → battle loop. Battle end plays the victory jingle, then resumes the overworld track from where it stopped (or restarts, configurable).
- **SFX:** fire-and-forget sounds with a limit on simultaneous instances per sound id.
- **Jingles:** short tracks that duck or pause the BGM (item get, level up), then restore it.
- **Volume groups:** master / BGM / SFX, stored in user config.
- Audio commands are queued during update and flushed once per tick (§3.3) for deterministic ordering.

---

## 12. Save system

- **Model:** `SaveData { header: SaveHeader, world: WorldState, plugins: BTreeMap<PluginId, pkrs_save::Value> }` (`Value` is our own dynamic value type; `ron` is used only inside the codec, §4.5).
  - `SaveHeader { format_version: u32, engine_version, game_id, game_version, timestamp, play_time_ticks, summary (player name, location, badges/progress count for the slot screen) }`.
  - `WorldState`: flags, variables, party, PC boxes, bag, money, current map + position + facing, RNG state, **per-map persistent state** (§6.5), NPC persistent state (moved/hidden), visited maps, options that belong to the save.
- **Format (decided): RON**, pretty-printed. Human-readable and debuggable. Player editing is accepted for fan games.
- **Versioning & migrations:** `format_version` is the engine's save schema version. Migrations are Rust functions `fn(Value) -> Result<Value, SaveError>` registered per version step (`v1 → v2 → v3`). They run in sequence on load, before typed deserialization. **Game-level migrations:** a game may provide `scripts/migrate.lua` with `migrate(save, from_game_version)`, which runs after engine migrations for flag and variable renames.
- **Slots:** a configurable number of slots (default 3) plus an optional autosave slot. The slot screen reads only the headers.
- **Corruption safety:** write to `slot1.sav.tmp`, `fsync`, rename over `slot1.sav` (atomic on all three OSes; on Windows via `MoveFileEx` with replace, which `std::fs::rename` uses). Keep the previous good save as `slot1.sav.bak`. On a load failure, offer the backup.
- **Location:** per-OS data directory from the `UserPaths` port (`directories` adapter) keyed on `game.id` (§14.3). `pkrs run` uses `<project>/saves/` instead to keep dev saves separate.

---

## 13. Assets & resource management

- **VFS (`pkrs-assets`):** one port, `Vfs { read(path) -> Bytes, exists, list(dir) }`, with three adapters (§4.5):
  - `FolderVfs`: loose project files, used by `pkrs run`.
  - `PakVfs`: the `.pak` archive (zip adapter), used in release builds.
  - `MemoryVfs`: in-memory files for tests.
  - Plugin assets are mounted under `plugin_id:` prefixes.
  - **Paths** are normalised once in the `Assets` facade (`pkrs_assets::path`), so adapters only see clean relative `VfsPath`s. `/` is the separator (`\` is accepted and converted). `.` and empty segments are dropped. `..` may not climb above the mount root, and absolute paths, drive prefixes and `: * ? " < > |` are rejected. `FolderVfs` also rejects symlinks that resolve outside the root.
  - **Case rules:** paths are case-sensitive on every OS. `FolderVfs` checks the exact on-disk case even on case-insensitive filesystems (Windows, default macOS) and reports `CaseMismatch` with the real spelling, so a wrong-case path fails in development rather than on a Linux player's machine.
- **Handles:** typed `Handle<T>` (a `u32` index into a slot map with a generation) for textures, sounds, fonts, maps and animations. Data structs store handles, not paths, after resolution.
- **Caching:** load on first request and keep it cached (one `AssetStore<T>` per asset type; `T: Asset` supplies the decoder, owned by the subsystem that uses `T`). Each `load` adds a reference and `release` drops one. Unreferenced assets stay cached until `evict_unused()` runs (on map change), so walking back and forth between maps doesn't reload shared assets. Evicted handles become stale rather than aliasing a reused slot. Maps can declare preloads (tilesets, NPC sprites). Battle assets (creature sprites) load on battle start and are reference-counted for eviction on map change. Large games stay bounded by keeping only the current map, its neighbours and the party's assets resident.
- **Loading:** synchronous in v1 (assets are tiny, and a GBA-style map loads in milliseconds). Map transitions hide loading behind the fade. Async loading can come later behind the same handle API.
- **`.pak` format (decided: single archive):** a **zip** container (`zip` crate, store/deflate) with a `pak.ron` index (file table, atlas rects, content hash, engine/API version, game id). We use zip because it's inspectable and has battle-tested crates. *Light obfuscation (XOR) is optional, later, and explicitly not security.* Data RON is packed as-is in v1. A pre-parsed binary cache is a later optimization.

---

## 14. Cross-platform build & distribution

### 14.1 Prebuilt runtime + packed data
- CI builds `pkrs-runtime` for each target on each engine release and attaches the binaries to the GitHub Release:
  - `x86_64-unknown-linux-gnu`
  - `x86_64-pc-windows-msvc`
  - `x86_64-apple-darwin` + `aarch64-apple-darwin` → merged into a universal binary with `lipo`
- `pkrs build --target X`:
  1. `pkrs check` (fails the build on errors).
  2. Pack the project into `game.pak` (host-independent).
  3. Download (and cache in `~/.cache/pkrs/runtimes/<version>/<target>`) the runtime matching `engine.version`, verifying its SHA-256 against the release manifest.
  4. Assemble the output:
     - **Linux:** `dist/linux/<Game>/` containing the `<game>` binary, `game.pak` and a `.desktop` file + icon. A `.tar.gz`, with AppImage later.
     - **Windows:** `dist/windows/<Game>/` containing `<Game>.exe` (renamed runtime, with the icon and version info patched via `rcedit`-equivalent resource editing, or left generic in v1) and `game.pak`. A `.zip`.
     - **macOS:** `dist/macos/<Game>.app/Contents/{MacOS/<game>, Resources/game.pak, Info.plist}`, with `Info.plist` generated from `pkrs.toml` (bundle id = `game.id`). A `.zip`. **Signing and notarization are a later milestone**; v1 documents the right-click → Open workaround and `xattr -dr com.apple.quarantine`.
- **No Rust toolchain needed** to build a game for any OS, from any OS.
- The runtime finds `game.pak` next to the executable (or in `Resources/` on macOS), with a `--project <dir>` override used by `pkrs run`.

### 14.2 Platform specifics
- **Windows:** `#![windows_subsystem = "windows"]` in release runtime builds, so no console window appears. Logs go to a file instead (§15).
- **Linux:** depends on system X11/Wayland + ALSA/PulseAudio libs, which miniquad loads dynamically. The runtime is built on an older glibc baseline (e.g. Ubuntu 22.04 runners) for broad compatibility.
- **macOS:** minimum macOS 11. High-DPI is handled by the integer scaler.

### 14.3 Per-OS paths (`directories` crate, keyed on `game.id`)
- Saves: `ProjectDirs::data_dir()` (e.g. `%APPDATA%\<org>\<game>\data`, `~/Library/Application Support/<id>`, `~/.local/share/<game>`).
- Config (bindings, volume, window scale): `ProjectDirs::config_dir()`.
- Logs: `ProjectDirs::data_local_dir()/logs`.

### 14.4 CI matrix (GitHub Actions)
- `ubuntu-latest`, `windows-latest`, `macos-latest` (arm64), plus `macos-13` (x86_64) for release builds.
- Per PR: `fmt`, `clippy`, `test`, `build` on all three OSes.
- **Smoke test:** the runtime runs a sample project headless for N ticks (`--headless --ticks 600 --script tests/smoke.lua`), then exits 0. On Linux it runs under `xvfb-run`. On Windows and macOS, if a GPU context is unavailable on runners, it falls back to `--headless`, which skips the window (§15.2).
- Release workflow on a tag: build runtimes + CLI for all targets, generate checksums and upload them to the GitHub Release.

---

## 15. Tooling, testing & quality

### 15.1 Debug tooling (dev builds / `pkrs run`)
- **F3 overlay:** FPS, frame time, tick count, state stack, current map + player tile, Lua memory, active coroutines.
- **F4 collision view:** colors collision, terrain, warps, triggers and encounter rects.
- **F5:** reload scripts. **F6:** reload data.
- **`~` console:** runs a Lua expression in a dev-only sandbox (warp, set flag, give item, start battle).
- **Logging:** `tracing` + `tracing-subscriber`. stderr in dev, a rolling file in release. Lua `log.*` is routed into `tracing` with a `script` target.

### 15.2 Testing strategy
- **`pkrs-battle` unit tests:** damage formula, type chart, status effects, turn order and the hook chain (with a mock `BattleHooks`). **Seeded replay tests:** a recorded `(seed, setup, inputs)` must produce the same `BattleEvent` stream (snapshot tests with `insta`).
- **`pkrs-data`:** the template projects must pass validation, plus golden tests for error messages.
- **`pkrs-script`:** sandbox tests (no `io`/`os`), error isolation (a throwing hook doesn't crash the battle), coroutine scheduler waits, plugin dependency resolution (cycles, conflicts, version mismatch).
- **Headless state tests:** the runtime can run with the `NullRenderer`/`NullAudio`/`NullGamepad` adapters (§4.5) and **scripted input** (`tests/*.lua` with `press("confirm")`, `expect_state("Dialogue")`), to test transitions such as "talk to NPC → dialogue → choice → battle → back to overworld".
- **`pkrs-save`:** round-trip tests, migration tests from frozen fixture saves per format version, and an atomic write test (simulated crash leaves the old save intact).

### 15.3 CI gates
`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, the multi-OS build + smoke test, and `pkrs check` on both templates.

### 15.4 Error handling policy
- Library crates use **`thiserror`** enums per crate (`DataError`, `ScriptError`, `SaveError`, …).
- Binaries (`pkrs-runtime`, `pkrs-cli`) use **`anyhow`** for top-level context. User-facing diagnostics use `miette` for `pkrs check`.
- No `unwrap()`/`expect()` in library code paths reachable from content or scripts (enforced in review, with clippy `unwrap_used` in library crates).
- Release runtime: fatal errors show a minimal error screen (message + log path) rather than a silent exit.

---

## 16. Roadmap / milestones

Each milestone is shippable and becomes **one issue**:

1. **Skeleton & CI.** Workspace with the crates from §4.3 (empty where unused), `Platform`/`Renderer` facades with their ports, macroquad and Null adapters, `cargo deny` wrapper bans (§4.5), window, fixed-step loop with interpolation, pixel-perfect 240×160 render target with integer scaling, one animated sprite on screen. CI green on Linux, Windows and macOS (fmt/clippy/test/build + smoke run).
2. **Project model & overworld.** `pkrs new` (one-shot template), `pkrs run`, `pkrs.toml` parsing, folder VFS, Tiled map loading with the layer conventions, grid movement (facing, collisions, ledges), camera follow + clamp, warps.
3. **Lua runtime & events.** mlua sandbox, coroutine scheduler, NPC interaction, dialogue box + bitmap text, choice menu, cutscene API (`say`, `move`, `wait`), state stack driven from scripts, flags and variables.
4. **Battle core.** `pkrs-battle` logic (state machine, damage pipeline, types, statuses, AI, seeded RNG, hook trait) with full unit tests. Then the battle UI state that plays back `BattleEvent`s. Wild encounters.
5. **Plugin system.** Manifests, dependency resolution, per-plugin environments, priority-chain hooks into battle and world, content registration, Lua UI screens, the **Resonance** example plugin, optional **doubles** default plugin, `pkrs plugin new`.
6. **Data & saves.** Full RON schemas + 3-stage validation, `pkrs check`, localization tables, party/bag/summary menus, save slots with atomic writes + migrations, debug hot reload.
7. **Distribution & audio.** `.pak` packer, `pkrs build` for all three OSes with prebuilt runtimes from the release workflow, per-OS paths, macOS `.app`, the Windows no-console build, kira audio (BGM loop points, crossfades, jingles, battle transitions), input rebinding + gamepad, `pkrs upgrade`.
8. **Sample projects.** `examples/one-shot` (a short story with no battles) and `examples/full-game-slice` (two routes, a town, a trainer, wild encounters, the Resonance plugin), both built for all three OSes in CI.

---

## 17. Open questions & risks

| # | Question / risk | Options | Leaning / decision |
|---|---|---|---|
| 1 | **WASM** | Ship web build / drop for v1 | **Dropped for v1.** Revisit only if a concrete need appears; **Android** is the more likely post-desktop target. |
| 2 | `.tmj` (JSON) map support in the `tiled` crate | Require `.tmx` only / add a converter | **`.tmx` only** for v1 |
| 3 | Windows exe icon & metadata patching without a Windows toolchain | Pure-Rust PE resource editing / generic icon in v1 | **Generic icon in v1**, revisit in M7 |
| 4 | macOS signing & notarization | Ad-hoc only / a Developer ID for the CLI-produced games (game author's cert) | **Unsigned + documented workaround** in v1; `pkrs build --sign` later |
| 5 | Number of crates vs. compile time and maintenance | Keep 13 / merge crates | **Keep the split**, merge if a crate stays under ~500 lines after M6 |
| 6 | Double battles in core vs. as a plugin | Core from the start / default plugin | **Default plugin** (`plugins/doubles/`) on N-vs-N internal model (1v1 default config) |
| 7 | Lua API level 1 freeze point | After M5 / after M8 | **After M8**: API is `0` (unstable) until the sample projects validate it |
| 8 | macroquad global context & headless testing | Abstract the renderer behind a trait with `NullRenderer` | **Resolved by §4.5:** `NullRenderer`/`NullAudio` are adapters of the `RenderBackend`/`AudioBackend` ports, used by `--headless` in CI (§15.2). The M1 spike validates the port surfaces in code |
| 9 | Overriding built-in states (e.g. replacing the battle UI entirely from Lua) | Allow / forbid in v1 | **Forbid in v1**: plugins add screens and hook data; full override later |
| 10 | Distribution of the CLI itself | `cargo install` only / prebuilt CLI binaries + an installer script | **Both**: prebuilt binaries are the primary path, since non-Rust users are the audience |

---

## References
- Glenn Fiedler, *Fix Your Timestep!*
- Robert Nystrom, *Game Programming Patterns* (Game Loop, State, Component, Command, Event Queue, Service Locator)
- Gamma, Helm, Johnson, Vlissides, *Design Patterns* (Adapter, Facade)
- Alistair Cockburn, *Hexagonal Architecture* (Ports & Adapters)
- Jason Gregory, *Game Engine Architecture*
- [Are We Game Yet?](https://arewegameyet.rs)
- Crates: `macroquad`, `kira`, `gilrs`, `mlua`, `tiled`, `ron`, `serde`, `directories`, `notify`, `zip`, `texture_packer`, `rand_pcg`, `tracing`, `thiserror`, `anyhow`, `miette`, `insta`, `include_dir`, `cargo-deny`

---

## Review & sign-off

| Role | Name | Decision | Date |
|---|---|---|---|
| Co-author | Arcangelo Chinè (@arcangelochine) | **ACK — approved for implementation** | 2026-11-03 |
| Owner / final reviewer | Umberto Domenico Ciccia (@umbertocicciaa) | **ACK — approved for implementation** | 2026-11-03 |

Final-review changes applied before sign-off:
1. External dependencies isolated with **Ports & Adapters + Facade** (§4.5), with related updates in §0.3, §3.1, §3.3, §4.2, §4.3, §5.2, §5.6, §6.1, §10, §11, §12, §13, §15.2, §16, §17.
2. Third-party franchise names and trademarked mechanic names removed from the text for licensing reasons (§1.1, §1.2, §1.3, §4.1, §6.3, §6.4, §7.1, §7.4, §9.4, References).

Signed: *Umberto Domenico Ciccia*, 2026-11-03
