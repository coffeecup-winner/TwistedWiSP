import ctypes

lib = ctypes.cdll.LoadLibrary('libtwisted_wisp.so')

# Define all the function prototypes
lib.wisp_engine_create.argtypes = ()
lib.wisp_engine_create.restype = ctypes.c_void_p
lib.wisp_engine_destroy.argtypes = (ctypes.c_void_p,)
lib.wisp_engine_destroy.restype = None


class TwistedWispEngine:
    def __init__(self):
        self.__wisp = lib.wisp_engine_create()

    def __del__(self):
        lib.wisp_engine_destroy(self.__wisp)
