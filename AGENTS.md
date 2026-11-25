# AGENTS.md

Rakeinen is a desktop app (Rust, eframe/egui 0.37 with the glow backend) for dithering images with a live preview. `README.md` describes the features and file formats. This file is what you need to work on the code.

## Environment

- The project runs in a Nix flake dev shell (`nix develop`, or direnv with `use flake .`). Use the tools from that shell.
- Flakes only see files tracked by git. `git add` a new file before running `nix build` or `nix flake check`, or the build will not find it.
- Tell the user whenever you change `flake.nix`, because they may need to reactivate the dev shell.
- Prefer `/tmp/opencode` over `/tmp`. Run tests with `TMPDIR=/tmp/opencode cargo test`, because a few tests write temporary files.

## Commands

```sh
cargo build
cargo test                 # the engine and most UI logic are tested headless
cargo clippy --all-targets # keep it warning-free
cargo fmt
nix build .#default        # packaged build, also checks the Nix side
nix flake check            # validates the desktop entry
nix fmt                    # formats flake.nix (nixfmt-rfc-style)
```

The dev profile is built with `opt-level = 1` so the live preview stays usable in debug builds.

`cargo run` opens a window on the user's screen. Do not launch the GUI without telling the user first. Check changes with build, clippy and tests, and say clearly what you could not verify visually.

## Git

- Commit as you go, with [Conventional Commits](https://www.conventionalcommits.org/) messages (`feat:`, `fix:`, `refactor:`, `build:`, `docs:`, `feat(ui):`, and so on).
- Keep commits focused, and put the reason in the body when it is not obvious.
- Do not leave a commit that does not build or that is missing part of its change.

## Layout

```
src/main.rs        entry point; first argument is an image to open; sets the app id
src/app.rs         application state, settings panel, toolbar, view layouts
src/worker.rs      background thread; one job can deliver several stages
src/export.rs      full-image render, integer upscaling, PNG saving
src/preset.rs      all settings as JSON (format marker + version)
src/engine/        pure image code with no UI dependency
  mod.rs           Settings, Region, process_region, quantized, preprocessed
  preprocess.rs    block-average downscale and tone curve
  palette.rs       Palette, import/export, k-means extraction, Quantizer
  diffusion.rs  kernels.rs  ordered.rs  noise.rs
src/ui/            viewer (panes and dividers), editors, wheel_select
packaging/         desktop entry and icon, installed by flake.nix
```

## How the pipeline fits together

- Pixel values in the engine are `[f32; 3]` in a working space of 0..=255 per channel. With "linear light" on, that is linear light scaled to 255, otherwise sRGB. Matching colors goes through `Quantizer` (RGB or Oklab distance).
- Everything is addressed in processed pixels: the source shrunk by `pixel_size`. The viewer draws results at original-image coordinates, so each result layer carries its `pixel_size`.
- `process_region` can dither just a rectangle. Ordered and noise dithering are exactly equal to a full run. Error diffusion starts with a warm-up margin and matches in tone but not pixel for pixel. A region that starts at the top and spans the full width is exact. Preprocessing is croppable because it is a plain block average plus a per-pixel curve.
- The worker always runs the newest job. A new job cancels the previous one, so each job must request every stage that is still wanted. The app tracks what each stage was last computed from (`submitted_quantized`, `submitted_dithered`) and what a job still owes (`awaiting`).
- While a pointer button is held on a large image, only the visible region is dithered, and one full pass follows on release.
- Stages: `Quantized` (preprocessed and snapped to the palette, no dithering) and `Dithered`. The Original view needs no work.

## egui 0.37 notes

- `eframe::App` has `fn ui(&mut self, ui, frame)`, not `update`. Panels and `CentralPanel` use `.show(ui, ...)`, and `Window` uses `.show(ctx, ...)`. `Ui::id` was renamed `scope_id`.
- The renderer is glow, not wgpu. wgpu ignores texture `mipmap_mode`, which is why the backend was switched. `winit` is a direct dependency only to keep its default features (Wayland dlopen and window decorations), because `eframe` is built with `default-features = false`.
- Scroll input is `smooth_scroll_delta`. A widget that handles the wheel should zero it so an enclosing scroll area does not also move (see `ui/wheel.rs`).
- `rfd` (file dialogs) loads `libdbus-1.so.3` at runtime to reach the xdg portal, so `dbus` must stay in `runtimeLibs` in `flake.nix`. Without it dialogs silently never open (no error, `None` is returned as if cancelled).
- Check an unfamiliar API in `~/.cargo/registry/src/*/egui-0.37.0/` before guessing.

## Conventions

- Keep the engine free of UI types. Put logic that can be tested headless in functions that do not need a window, and test it. A `egui::Context::default()` is enough to create textures in tests.
- Match the surrounding style. Comments are sparse and explain why, not what.
- New settings need: a serde derive, a default, validation or clamping in `Settings::sanitized`, and a round-trip case in `preset.rs`. Settings missing from a preset must keep their defaults.
- Do not add dependencies lightly. This repo uses `rayon` for parallel pixel work and `serde_json` for files.

## Pitfalls

- Edits made with regex substitutions can silently not match, especially after `cargo fmt` reflows code. After scripted edits, check that every change landed (grep for it), or use exact-match edits that fail loudly. A previous change shipped incomplete this way.
- Do not describe code behaviour from memory. Read the code before explaining or diagnosing it.
- `cargo fmt` rewrites files. Run it before committing so formatting changes do not mix into later diffs.
