# Rakeinen

A desktop app for dithering images, with a live preview that updates as you tweak the settings.

*Rakeinen* is Finnish for "grainy".

## Features

- **Live preview.** Dithering runs on a background thread and the result updates as you change a setting. On large images a reduced-quality preview is shown while you drag a slider, followed by one full-quality pass when you let go.
- **Error diffusion** with Floyd–Steinberg, Jarvis–Judice–Ninke, Stucki, Atkinson, Burkes, Sierra, Sierra Two-Row and Sierra Lite. Options for serpentine scanning and error strength.
- **Ordered dithering** with Bayer 2/4/8/16, clustered-dot and line matrices.
- **Noise dithering** with white noise (seeded) and blue noise (generated with void-and-cluster).
- **Custom kernels and matrices.** Edit an error diffusion kernel or an ordered threshold matrix (up to 16×16, any shape) in the app, and save or load it as JSON.
- **Palettes.** Edit colors by hand, pick a preset (black & white, grayscale, Game Boy, CGA, EGA, PICO-8), import or export `.hex` and `.gpl` files, or extract a palette from the loaded image.
- **Color handling.** Choose RGB or Oklab for finding the closest palette color, and optionally dither in linear light.
- **Preprocessing.** Pixel size (downscaling), brightness, contrast, gamma, grayscale and invert.
- **Viewer.** Pan and zoom, a Fit button (or double-click), a 1:1 button for one image pixel per screen pixel, and a "Show original" toggle.

## Running

The project uses a Nix flake. With flakes enabled:

```sh
nix run .               # build and run
nix develop             # enter a shell with the Rust toolchain and window libraries
cargo run --release     # inside the dev shell
```

If you use direnv, `.envrc` already contains `use flake .`.

## Using it

1. Open an image with **Open…** or drop a file on the window. PNG, JPEG, WebP and BMP are supported.
2. Pick a method and tweak its settings in the left panel.
3. Scroll to zoom, drag to pan, and use **Fit** or **1:1** to reset the view.

### File formats

| What | Format |
|---|---|
| Diffusion kernel | JSON: `{ "cols", "rows", "weights", "divisor" }`. Row 0 is the current scanline and the current pixel is in column `cols / 2`. |
| Threshold matrix | JSON: `{ "width", "height", "values" }` |
| Palette | `.hex` (one `RRGGBB` color per line) or GIMP `.gpl` |

## Project layout

```
src/
  main.rs            entry point
  app.rs             application state and the settings panel
  worker.rs          background dithering thread, with cancellation of stale jobs
  engine/
    mod.rs           settings and the processing pipeline
    preprocess.rs    resize and tone adjustments
    palette.rs       palettes, import/export, extraction, color matching
    diffusion.rs     error diffusion
    kernels.rs       diffusion kernel data and built-in kernels
    ordered.rs       threshold matrices and ordered dithering
    noise.rs         white and blue noise
  ui/
    viewer.rs        pan/zoom image viewer
    kernel_editor.rs
    matrix_editor.rs
    palette_editor.rs
```

The engine does not depend on the UI, so it can be tested on its own:

```sh
cargo test
```

## Not done yet

- Saving the result as PNG, with optional integer upscaling.
- Saving and loading all settings as a preset.
