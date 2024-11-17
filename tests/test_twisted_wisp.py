from .twisted_wisp import *
import numpy as np
import os

CORE_PATH = os.environ.get('WISP_CORE_PATH')

THIS_DIR = os.path.dirname(os.path.abspath(__file__))
EXAMPLES_PATH = os.path.abspath(os.path.dirname(THIS_DIR) + os.sep + 'wisp_gui')

def test_init():
    config = TwistedWispConfig()
    config.set_core_path(CORE_PATH)
    engine = TwistedWispEngine(config)
    # Explicitly delete the engine
    del engine


def test_basic():
    config = TwistedWispConfig()
    config.set_core_path(CORE_PATH)
    engine = TwistedWispEngine(config)
    name = engine.context_load_flow_from_file(EXAMPLES_PATH + os.sep + 'phasor_test.twf')
    sp = engine.engine_compile_signal_processor(name)
    assert sp is not None
    floats = np.array([0.0, 0.0], dtype=np.float32)
    sp.process_one(floats)
    assert (floats == np.array([0.009977324, 0.009977324], dtype=np.float32)).all()
