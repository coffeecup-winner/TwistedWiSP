import struct
from .twisted_wisp import *

def test_init():
    engine = TwistedWispEngine()
    # Explicitly delete the engine
    del engine

def test_basic():
    engine = TwistedWispEngine()
    sp = engine.engine_compile_signal_processor(b'phasor')
    assert sp is not None
    floatlist = [0.0, 0.0]
    sp.process_one(struct.pack('%sf' % len(floatlist), *floatlist)                   
)
