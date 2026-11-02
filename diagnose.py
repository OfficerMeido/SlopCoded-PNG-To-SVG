#!/usr/bin/env python3
"""
diagnose.py - find out which vtracer code path crashes on your machine.

Usage:  python diagnose.py /path/to/image.png

Each test runs in its own subprocess, so a segfault only kills that test.
Paste the whole output back.
"""

import subprocess
import sys
import tempfile
from importlib import metadata
from pathlib import Path

CASES = [
    ("import vtracer only",
     "import vtracer"),

    ("convert_image_to_svg_py, default settings",
     "import sys, vtracer\n"
     "vtracer.convert_image_to_svg_py(sys.argv[1], sys.argv[2])"),

    ("convert_image_to_svg_py, png_to_svg.py settings",
     "import sys, vtracer\n"
     "vtracer.convert_image_to_svg_py(sys.argv[1], sys.argv[2], colormode='color',\n"
     "  hierarchical='stacked', mode='spline', filter_speckle=4, color_precision=6,\n"
     "  layer_difference=16, corner_threshold=60, length_threshold=4.0,\n"
     "  splice_threshold=45, path_precision=3)"),

    ("convert_raw_image_to_svg (file bytes)",
     "import sys, vtracer\n"
     "svg = vtracer.convert_raw_image_to_svg(open(sys.argv[1], 'rb').read(), img_format='png')\n"
     "open(sys.argv[2], 'w').write(svg)"),

    ("convert_pixels_to_svg via Pillow (full size)",
     "import sys, vtracer\n"
     "from PIL import Image\n"
     "im = Image.open(sys.argv[1]).convert('RGBA')\n"
     "svg = vtracer.convert_pixels_to_svg(list(im.getdata()), im.size)\n"
     "open(sys.argv[2], 'w').write(svg)"),

    ("convert_pixels_to_svg via Pillow (shrunk to 512px)",
     "import sys, vtracer\n"
     "from PIL import Image\n"
     "im = Image.open(sys.argv[1]).convert('RGBA')\n"
     "im.thumbnail((512, 512))\n"
     "svg = vtracer.convert_pixels_to_svg(list(im.getdata()), im.size)\n"
     "open(sys.argv[2], 'w').write(svg)"),
]


def describe(rc: int) -> str:
    if rc == 0:
        return "OK"
    if rc < 0:
        return f"CRASHED (signal {-rc}{' = segfault' if rc == -11 else ''})"
    return f"python error (exit {rc})"


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip())
        return 2
    img = Path(sys.argv[1])

    print("python :", sys.version.split()[0])
    try:
        print("vtracer:", metadata.version("vtracer"))
    except metadata.PackageNotFoundError:
        print("vtracer: NOT INSTALLED")
    try:
        from PIL import Image
        with Image.open(img) as im:
            print(f"image  : {im.size[0]}x{im.size[1]}, mode {im.mode}, "
                  f"{img.stat().st_size / 1024:.0f} KB")
    except Exception as e:
        print("image  : could not read with Pillow:", e)
    print()

    with tempfile.TemporaryDirectory() as tmp:
        out = str(Path(tmp) / "out.svg")
        for name, code in CASES:
            r = subprocess.run([sys.executable, "-c", code, str(img), out],
                               capture_output=True, text=True)
            print(f"{describe(r.returncode):28} {name}")
            if r.returncode > 0:
                last = (r.stderr.strip().splitlines() or [""])[-1]
                print(f"{'':28} {last}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
