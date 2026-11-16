import ctypes

lib = ctypes.cdll.LoadLibrary('libtwisted_wisp.so')

# Define all the function prototypes
lib.wisp_engine_create.argtypes = ()
lib.wisp_engine_create.restype = ctypes.c_void_p
lib.wisp_engine_destroy.argtypes = (ctypes.c_void_p,)
lib.wisp_engine_destroy.restype = None
lib.wisp_context_set_main_function.argtypes = (ctypes.c_void_p, ctypes.c_char_p)
lib.wisp_context_set_main_function.restype = None
lib.wisp_context_update.argtypes = (ctypes.c_void_p,)
lib.wisp_context_update.restype = None


class TwistedWispEngine:
    def __init__(self):
        self.__wisp = lib.wisp_engine_create()

    def __del__(self):
        lib.wisp_engine_destroy(self.__wisp)
    
    def context_set_main_function(self, function_name):
        lib.wisp_context_set_main_function(self.__wisp, function_name)

    def context_update(self):
        lib.wisp_context_update(self.__wisp)
