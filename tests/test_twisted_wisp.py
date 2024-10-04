from .twisted_wisp import *

def test_init():
    engine = TwistedWispEngine()
    # Explicitly delete the engine
    del engine

def test_basic():
    engine = TwistedWispEngine()
    engine.context_set_main_function(b'phasor')
    engine.context_update()
    del engine
