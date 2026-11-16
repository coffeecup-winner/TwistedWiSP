from .twisted_wisp import *

def test_init():
    engine = TwistedWispEngine()
    # Explicitly delete the engine
    del engine
