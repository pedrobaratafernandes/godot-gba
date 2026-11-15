# Godot GBA (Rust GDExtension)

A Game Boy Advance emulator for Godot 4.7.2, built with Rust and GDExtension on top of the [rustboyadvance-ng](https://github.com/michelhe/rustboyadvance-ng) core. Builds for **Web, Windows and Linux**.

## Layout

```
godot-gba/
├── Cargo.toml / rust-toolchain.toml / .cargo/   # Rust crate (pinned versions)
├── Dockerfile.web                               # Emscripten 4.0.20 + Rust 1.97.1
├── bios/cult-of-gba-bios.bin                    # free BIOS (MIT), embedded in the library
├── src/
│   ├── lib.rs            # GDExtension entry point
│   ├── host.rs           # glue to rustboyadvance-core (screen, sound, buttons)
│   ├── emulator_node.rs  # GBAEmulator node used in the scenes
│   └── tests.rs          # tests (cargo test --release)
├── patches/rustboyadvance-utils/  # copy with 1 Emscripten fix (see [patch] in Cargo.toml)
├── build/                # build scripts (Web, Windows, Linux)
└── GodotProject/         # Godot project (Launcher + MainGBA)
```

## Building

Every library goes to `GodotProject/bin/`:

| Platform | Command | Produces |
|---|---|---|
| **Windows** | `build\build_windows.bat` (close Godot first) | `godot_gba.dll` |
| **Linux** (via Docker) | `bash build/build_linux_docker.sh` | `libgodot_gba.so` |
| **Linux** (on a Linux machine) | `bash build/build_linux.sh` | `libgodot_gba.so` |
| **Web** (via Docker) | `bash build/build_web_docker.sh` | `godot_gba.wasm` + `godot_gba.threads.wasm` |
| **Everything** | `bash build/build_all.sh` | all of the above |

The Docker scripts need **Docker Desktop** running and are run from Git Bash on Windows.
The Windows DLL is also what the Godot **editor** and **F5** use.

Web variants:
- `godot_gba.wasm` → Web export with **Thread Support off** (the project's preset).
- `godot_gba.threads.wasm` → **Thread Support on** (the server must send COOP/COEP headers).

Then, in Godot: **Project > Export** and pick a preset (Windows Desktop, Linux or Web).
Exports go to `build/windows/`, `build/linux/` and `build/web/`.
To test the Web build use Godot's "Run in Browser" button (`index.html` does not
work when opened straight from disk).

### Things to know

- **LTO is disabled on purpose** (`Cargo.toml`): with LTO, rustboyadvance-ng's unsafe
  code is miscompiled and the game crashes after 2-3 frames.
- **Use `;` for comments in `godot_gba.gdextension`**: Godot 4.7 ignores the whole
  `[libraries]` section when it contains `#` comments ("No GDExtension library found").

## Tests

They run the emulator core without Godot. Use `--release` to test with the same
optimisations as the real builds:

```bash
cargo test --release
```

- The embedded BIOS is 16 KB; invalid BIOS or ROM files are rejected.
- A test ROM (ARM code that paints the screen red) runs with and without `skip_bios`
  (i.e. the Cult-of-GBA BIOS boots the cartridge).
- Sound comes out at 44100 Hz, buttons reach the KEYINPUT register and save states work.
- **Every game in `GodotProject/games/`** (`.gba`/`.zip`) must boot and show an image.

## Controls

| GBA | Keyboard | Gamepad |
|---|---|---|
| D-pad | Arrow keys | D-pad / left stick |
| A | X | B (right) / Y |
| B | Z | A (bottom) / X |
| L / R | A / S | LB / RB |
| Start | Enter | Start |
| Select | Backspace / Right Shift | Back |
| Fast forward (hold) | Tab | Right trigger |
| Menu | Escape | Guide |

On touch screens a D-pad, A/B, L/R, Start/Select and a menu button appear.
Keys can be changed in **Project Settings > Input Map** (`gba_*` actions).

## BIOS

The emulator ships with the free [Cult-of-GBA](https://github.com/Cult-of-GBA/BIOS) BIOS
(MIT license, see `bios/LICENSE`). To use the official BIOS, put it at
`GodotProject/roms/gba_bios.bin` — **do not publish it** in a public export
(the presets include `*.bin`).

## Saves

- **Save State / Load State** in the menu: saves the full state (including the in-game
  save) to `user://saves/` — on the Web it is stored in the browser (IndexedDB).
- In-game saves (SRAM/Flash/EEPROM) are **not** stored separately yet: they are lost
  when the game is closed, unless you use a Save State.

## Credits

- Engine: [Godot Engine](https://godotengine.org) 4.7 — MIT license
- Rust bindings: [godot-rust](https://github.com/godot-rust/gdext) (`godot` crate 0.5.5) — MPL-2.0.
- GBA emulator core: [RustBoyAdvance-NG](https://github.com/michelhe/rustboyadvance-ng)
  by Michel Heily — MIT license. Used as `rustboyadvance-core` / `rustboyadvance-utils`;
  a slightly modified copy of `rustboyadvance-utils` lives in `patches/` (see its `LICENSE`).
- On-screen controls icons: [Input Prompts](https://kenney.nl/assets/input-prompts)
  by [Kenney](https://kenney.nl) — CC0 1.0 (public domain). Files in `GodotProject/assets/`.
- GBA BIOS: [Cult-of-GBA BIOS](https://github.com/Cult-of-GBA/BIOS) — MIT license.

## License

MIT © Pedro Fernandes — see `LICENSE`.
