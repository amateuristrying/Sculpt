"""Generate local stress fixtures from the installed, pinned TripoSR examples.

These are synthetic input variations, not a representative quality dataset.
No user files are read or sent anywhere. Outputs remain in the ignored folder.
"""
from pathlib import Path
import json
from PIL import Image, ImageDraw

PROJECT = Path(__file__).resolve().parents[2]
OUTPUT = PROJECT / 'backend/outputs/edge-fixtures'
SOURCE = PROJECT / '.sculpt-runtime/TripoSR/examples'


def main():
    OUTPUT.mkdir(parents=True, exist_ok=True)
    chair = Image.open(SOURCE / 'chair.png').convert('RGBA')
    chair.rotate(90, expand=True).save(OUTPUT / 'sideways-chair.png')
    chair.save(OUTPUT / 'transparent-chair.png')
    occluded = chair.copy()
    ImageDraw.Draw(occluded).rectangle((0, chair.height // 2, chair.width // 2, chair.height), fill=(90, 100, 110, 255))
    occluded.save(OUTPUT / 'occluded-chair.png')
    clutter = Image.new('RGBA', (768, 512), (170, 160, 145, 255))
    chair.thumbnail((420, 420))
    clutter.alpha_composite(chair, (20, 50))
    teapot = Image.open(SOURCE / 'teapot.png').convert('RGBA')
    teapot.thumbnail((380, 380))
    clutter.alpha_composite(teapot, (360, 100))
    clutter.convert('RGB').save(OUTPUT / 'two-objects.jpg')
    Image.new('RGBA', (512, 512), (0, 0, 0, 0)).save(OUTPUT / 'empty.png')
    cases = [{'id': name, 'source': file, 'category': 'synthetic stress input',
              'credit': 'Derived locally from the pinned TripoSR repository examples',
              **options} for name, file, options in [
        ('transparent-chair', 'transparent-chair.png', {}),
        ('sideways-chair', 'sideways-chair.png', {}),
        ('occluded-chair', 'occluded-chair.png', {}),
        ('two-objects', 'two-objects.jpg', {}),
        ('keep-background', 'two-objects.jpg', {'background': 'keep'}),
        ('empty-alpha', 'empty.png', {'expected': 'error'}),
    ]]
    (OUTPUT / 'manifest.json').write_text(json.dumps({'cases': cases}, indent=2))
    print(OUTPUT / 'manifest.json')


if __name__ == '__main__':
    main()
