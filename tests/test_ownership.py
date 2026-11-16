"""Run ABI ownership cases out of process so a native crash fails the test."""
import os
from pathlib import Path
import subprocess
import sys

import pytest


@pytest.mark.parametrize('case', ['transfer', 'invalid_engine', 'processor_keeps_engine'])
def test_processor_ownership(case):
    code = '''
import ctypes
import gc
import os
import numpy as np
from tests.twisted_wisp import TwistedWispConfig, TwistedWispEngine, lib
config = TwistedWispConfig()
config.set_core_path(os.environ['WISP_CORE_PATH'])
engine = TwistedWispEngine(config)
name = engine.context_load_flow_from_file('wisp_gui/examples/phasor_test.twf')
processor = engine.engine_compile_signal_processor(name)
assert processor is not None
output = np.zeros(2, dtype=np.float32)
'''
    if case == 'transfer':
        code += '''
engine.switch_to_signal_processor(processor)
try:
    processor.process_one(output)
except RuntimeError as error:
    assert 'transferred' in str(error)
else:
    raise AssertionError('Transferred processor remained usable')
del processor
gc.collect()
del engine
gc.collect()
'''
    elif case == 'invalid_engine':
        code += '''
original = processor._handle()
handle = ctypes.c_void_p(original)
assert not lib.wisp_engine_take_signal_processor(None, ctypes.byref(handle))
assert handle.value == original
processor.process_one(output)
assert np.allclose(output, 0.009977324)
'''
    else:
        code += '''
del engine
del config
gc.collect()
processor.process_one(output)
assert np.allclose(output, 0.009977324)
del processor
gc.collect()
'''
    result = subprocess.run(
        [sys.executable, '-c', code],
        cwd=Path(__file__).resolve().parents[1],
        env=os.environ.copy(), capture_output=True, text=True, timeout=30,
    )
    assert result.returncode == 0, result.stdout + result.stderr
