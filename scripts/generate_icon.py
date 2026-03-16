#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = ["pillow>=10"]
# ///
from __future__ import annotations

import argparse
from pathlib import Path

from PIL import Image, ImageOps


SIZES = (16, 32, 64)
NAME_BY_SIZE = {16: "small", 32: "medium", 64: "big"}


def parse_args() -> argparse.Namespace:
    repo_root = Path(__file__).resolve().parents[1]
    default_input = repo_root / "assets" / "icon.png"
    default_output_dir = repo_root / "phira" / "icon"

    parser = argparse.ArgumentParser(
        description=(
            "Generate packed RGBA bytes for 16/32/64 icons. "
            "Output layout matches Icon { small, medium, big }."
        )
    )
    parser.add_argument(
        "input",
        nargs="?",
        type=Path,
        default=default_input,
        help=f"Input image path (default: {default_input})",
    )
    parser.add_argument(
        "output_dir",
        nargs="?",
        type=Path,
        default=default_output_dir,
        help=f"Output directory (default: {default_output_dir})",
    )
    parser.add_argument(
        "--ext",
        default="",
        help="Optional file extension (e.g. .rgba). Default: no extension.",
    )
    return parser.parse_args()


def generate_icon_bytes(input_path: Path, output_dir: Path, ext: str) -> None:
    if not input_path.exists():
        raise FileNotFoundError(f"Input not found: {input_path}")

    resample = getattr(Image, "Resampling", Image).LANCZOS
    img = Image.open(input_path).convert("RGBA")

    output_dir.mkdir(parents=True, exist_ok=True)
    for size in SIZES:
        resized = ImageOps.fit(img, (size, size), method=resample)
        buf = resized.tobytes()
        expected = size * size * 4
        if len(buf) != expected:
            raise ValueError(f"Unexpected output size for {size}: {len(buf)} (expected {expected})")
        name = NAME_BY_SIZE[size]
        output_path = output_dir / f"{name}{ext}"
        output_path.write_bytes(buf)
        print(f"Wrote {len(buf)} bytes to {output_path}")


def main() -> None:
    args = parse_args()
    generate_icon_bytes(args.input, args.output_dir, args.ext)


if __name__ == "__main__":
    main()
