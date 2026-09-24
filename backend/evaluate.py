"""Sculpt's real-photo evaluation CLI. Run --help for the four stages."""
import argparse
from pathlib import Path

from sculpt_eval.dataset import DEFAULT_MANIFEST, fetch_photos


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    fetch = commands.add_parser('fetch', help='Download and hash-check the CC0 photos (network required)')
    fetch.add_argument('--manifest', type=Path, default=DEFAULT_MANIFEST)
    run = commands.add_parser('run', help='Reconstruct the whole set sequentially on the local MPS runtime')
    run.add_argument('--manifest', type=Path, default=DEFAULT_MANIFEST)
    run.add_argument('--output', type=Path, required=True)
    run.add_argument('--quality', choices=['draft', 'balanced', 'high'], default='balanced')
    run.add_argument('--masks', type=Path, help='Use saved source-coordinate grayscale masks: DIRECTORY/case-id/mask.png')
    reference = commands.add_parser('reference', help='Freeze baseline foreground masks and estimated photo cameras')
    reference.add_argument('--run', type=Path, required=True)
    reference.add_argument('--output', type=Path, required=True)
    compare = commands.add_parser('compare', help='Render both runs at four angles and create an offline HTML comparison')
    compare.add_argument('--left', type=Path, required=True)
    compare.add_argument('--right', type=Path, required=True)
    compare.add_argument('--reference', type=Path, required=True)
    compare.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.command == 'fetch':
        fetch_photos(args.manifest)
    elif args.command == 'run':
        from sculpt_eval.runner import run
        return run(args.manifest, args.output, args.quality, args.masks)
    elif args.command == 'reference':
        from sculpt_eval.report import make_reference
        make_reference(args.run, args.output)
    else:
        from sculpt_eval.report import compare
        compare(args.left, args.right, args.reference, args.output)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
