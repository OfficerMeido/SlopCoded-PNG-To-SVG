# PNG to SVG (Rust)

Converts PNG files to true vector SVGs (color tracing with the
[vtracer](https://github.com/visioncortex/vtracer) crate), then cleans up the
edges so straight lines stay straight and curves stay smooth. The SVG is saved
next to the PNG with the same name (`logo.png` -> `logo.svg`).

The result is one small native binary: no interpreter, no virtual environment
and no packaging step.

## Files

| File             | Purpose                                                      |
|------------------|--------------------------------------------------------------|
| `Cargo.toml`     | Project definition (one dependency: `vtracer`)               |
| `src/main.rs`    | Command line, batch handling, notifications, runs vtracer    |
| `src/smooth.rs`  | Edge cleanup: lines, corners, circles, curves (no dependencies) |
| `build.sh`       | Builds `target/release/png_to_svg`                           |
| `install.sh`     | Installs it and adds "Open With" for PNGs                    |

## Build and install

Install Rust from https://rustup.rs, then:

```
bash build.sh        # cargo build --release
bash install.sh      # per-user install, no sudo
```

Optional fully static binary (runs on any Linux regardless of glibc):

```
rustup target add x86_64-unknown-linux-musl
STATIC=1 bash build.sh
```

Run the tests for the smoothing code with `cargo test`.

## Use

- **File manager:** right-click a PNG > Open With > **Convert PNG to SVG**.
  Select several PNGs to convert them all. A desktop notification appears when
  finished (needs `notify-send`).
- **Terminal:**

```
png_to_svg image.png [more.png ...]
png_to_svg --no-smooth image.png     # keep vtracer's raw output
```

An existing SVG with the same name is overwritten. The installer does not make
this the default app for PNGs, so double-clicking still opens your image viewer.

## How the edge cleanup works

vtracer follows the pixel staircase of an edge, which leaves straight diagonals
slightly wobbly. After tracing, each shape outline is resampled, corners are
detected, and then:

- straight stretches become exact `L` lines, with corners snapped to where
  neighboring lines meet;
- circles and circular arcs (like a round logo) are detected and rebuilt as
  true circles (up to four cubic Béziers of 90 degrees each), so they are
  perfectly round and symmetrical instead of lumpy;
- other curved stretches are refit as a few cubic Béziers (within about 1 px);
- shapes smaller than ~24 px (e.g. tiny text) are left untouched.

If cleanup can't process a shape, that shape is left exactly as vtracer wrote it.

## Notes

- The tracing runs on a thread with a 512 MB stack (virtual memory, only used
  if needed) and panics are caught, so one bad image doesn't stop a batch.
- Keep `panic = "unwind"` (the default); don't switch the release profile to
  `panic = "abort"`.
- If the build fails on `vtracer::Config` or `convert_image_to_svg`, the crate's
  API changed between versions. `src/main.rs` is the only file that touches it
  (function `trace`); check the crate docs for your version.
