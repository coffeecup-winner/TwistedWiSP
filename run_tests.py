#!/usr/bin/env python

import os
import sys

BASE_DIR = os.path.dirname(os.path.abspath(__file__))

if __name__ == '__main__':
    release_mode = False
    pytest_args = sys.argv[1:]
    if len(sys.argv) > 1:
        if sys.argv[1] == '--release':
            release_mode = True
            pytest_args = sys.argv[2:]

    if release_mode:
        cargo_arg = '--release'
        target = 'release'
    else:
        cargo_arg = ''
        target = 'debug'

    core_dir = os.path.join(BASE_DIR, 'wisp', 'core')
    target_dir = os.path.join(BASE_DIR, 'target', target)
    tests_dir = os.path.join(BASE_DIR, 'tests')

    os.chdir(BASE_DIR)
    os.system(f'cargo build {cargo_arg}')

    os.chdir(tests_dir)
    os.system(f'LD_LIBRARY_PATH={target_dir} '
              f'WISP_CORE_PATH={core_dir} '
              f'python -m pytest {" ".join(pytest_args)}')
