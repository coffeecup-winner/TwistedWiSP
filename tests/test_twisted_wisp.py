import struct
from .twisted_wisp import *
import os

CORE_PATH = os.environ.get('WISP_CORE_PATH')


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
    sp = engine.engine_compile_signal_processor('phasor')
    assert sp is not None
    floatlist = [0.0, 0.0]
    sp.process_one(struct.pack('%sf' % len(floatlist), *floatlist))
