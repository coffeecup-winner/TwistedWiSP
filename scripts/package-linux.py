#!/usr/bin/env python3
"""Build/package native Linux; include locked dependency license sources."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('destination', type=Path)
parser.add_argument('--debug', action='store_true', help='development smoke artifact, not release')
args = parser.parse_args()
out = args.destination.resolve()
if out.exists():
    raise SystemExit('Destination must not exist')
subprocess.run(['cargo', 'build', '--locked', '--offline', '-p', 'wisp_gui'] + ([] if args.debug else ['--release']), cwd=root, check=True)
host = next(line.split(': ', 1)[1] for line in subprocess.check_output(['rustc', '-vV'], text=True).splitlines() if line.startswith('host: '))
metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--offline', '--format-version=1', '--filter-platform', host], cwd=root))
packages = {p['id']: p for p in metadata['packages']}
nodes = {n['id']: n for n in metadata['resolve']['nodes']}
start = next(p['id'] for p in packages.values() if p['name'] == 'wisp_gui')
visited = set()
def visit(id):
    if id in visited:
        return
    visited.add(id)
    for dep in nodes[id]['dependencies']:
        visit(dep)
visit(start)
(out / 'bin').mkdir(parents=True)
share = out / 'share' / 'twistedwisp'
shutil.copytree(root / 'wisp/core', share / 'core')
(share / 'examples').mkdir()
for name in ['control.twf', 'phasor_test.twf']:
    shutil.copy2(root / 'wisp_gui' / 'examples' / name, share / 'examples' / name)
for name in ['LICENSE-MIT', 'LICENSE-APACHE']:
    shutil.copy2(root / name, out / name)
shutil.copytree(root / 'wisp_gui/docs', share / 'docs')
binary = Path(metadata['target_directory']) / ('debug' if args.debug else 'release') / 'wisp_gui'
shutil.copy2(binary, out / 'bin/wisp_gui')
licenses = out / 'licenses'
licenses.mkdir()
notices = ['# Locked native Linux dependency notices', '', 'Generated from the enabled GUI dependency graph (including build dependencies).', 'License expressions and upstream license files follow; inspect before distribution.', '']
for id in sorted(visited, key=lambda id: (packages[id]['name'], packages[id]['version'])):
    p = packages[id]
    name = p['name'] + '-' + p['version']
    folder = licenses / name
    folder.mkdir()
    source = Path(p['manifest_path']).parent
    files = [f for f in source.iterdir() if f.is_file() and (f.name.upper().startswith(('LICENSE', 'COPYING', 'NOTICE')) or f.name == p.get('license_file'))]
    if source.is_relative_to(root):
        files += [root / 'LICENSE-MIT', root / 'LICENSE-APACHE']
    for f in files:
        shutil.copy2(f, folder / f.name)
    notices.append(f"- {name}: {p.get('license') or p.get('license_file') or 'MISSING LICENSE'}; {p.get('repository') or ''}")
    if not files:
        (folder / 'SOURCE.txt').write_text(f"{p.get('repository')}\nSPDX: {p.get('license')}\nRegistry source: {p.get('source')}\n")
(out / 'THIRD-PARTY.md').write_text('\n'.join(notices) + '\n')
inspection = subprocess.run(['ldd', str(out / 'bin/wisp_gui')], text=True, capture_output=True, check=True)
(out / 'shared-libraries.txt').write_text(inspection.stdout)
subprocess.run([str(out / 'bin/wisp_gui'), '--check-resources'], cwd='/tmp', check=True)
print(out)
