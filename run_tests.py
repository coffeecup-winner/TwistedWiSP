#!/usr/bin/env python3
"""Build first, then test the exact Cargo target; propagate either failure."""
import json
import os
from pathlib import Path
import subprocess
import sys

BASE_DIR = Path(__file__).resolve().parent


def main(args=None):
    args = list(sys.argv[1:] if args is None else args)
    release = bool(args and args[0] == '--release')
    if release:
        args.pop(0)
    build = subprocess.run(['cargo', 'build', '--locked'] + (['--release'] if release else []), cwd=BASE_DIR)
    if build.returncode:
        return build.returncode
    metadata = subprocess.run(['cargo', 'metadata', '--no-deps', '--format-version=1'], cwd=BASE_DIR, capture_output=True, text=True)
    if metadata.returncode:
        sys.stderr.write(metadata.stderr)
        return metadata.returncode
    target = Path(json.loads(metadata.stdout)['target_directory']) / ('release' if release else 'debug')
    env = os.environ.copy()
    env['WISP_LIBRARY_PATH'] = str(target / 'libtwisted_wisp.so')
    env['WISP_CORE_PATH'] = str(BASE_DIR / 'wisp' / 'core')
    env['LD_LIBRARY_PATH'] = str(target) + (':' + env['LD_LIBRARY_PATH'] if env.get('LD_LIBRARY_PATH') else '')
    return subprocess.run([sys.executable, '-m', 'pytest', str(BASE_DIR / 'tests'), *args], cwd=BASE_DIR, env=env).returncode


if __name__ == '__main__':
    sys.exit(main())
