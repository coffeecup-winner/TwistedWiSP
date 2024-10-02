#!/usr/bin/env python

import os
import sys

if __name__ == '__main__':
    release_mode = False
    pytest_args = sys.argv[1:]
    if len(sys.argv) > 1:
        if sys.argv[1] == '--release':
            release_mode = True
            pytest_args = sys.argv[2:]

    if release_mode:
        cargo_arg = '--release'
        target_dir = 'target/release'
    else:
        cargo_arg = ''
        target_dir = 'target/debug'

    os.system(f'cargo build {cargo_arg}')
    os.chdir('../tests')
    os.system(f'LD_LIBRARY_PATH=../{target_dir} python -m pytest {" ".join(pytest_args)}')
