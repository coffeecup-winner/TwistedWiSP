import ctypes

lib = ctypes.cdll.LoadLibrary('libtwisted_wisp.so')

# Define all the function prototypes
lib.wisp_engine_create.argtypes = ()
lib.wisp_engine_create.restype = ctypes.c_void_p
lib.wisp_engine_destroy.argtypes = (ctypes.c_void_p,)
lib.wisp_engine_destroy.restype = None
lib.wisp_engine_compile_signal_processor.argtypes = (ctypes.c_void_p, ctypes.c_char_p)
lib.wisp_engine_compile_signal_processor.restype = ctypes.c_void_p
lib.wisp_context_set_main_function.argtypes = (ctypes.c_void_p, ctypes.c_char_p)
lib.wisp_context_set_main_function.restype = None
lib.wisp_context_update.argtypes = (ctypes.c_void_p,)
lib.wisp_context_update.restype = None

lib.wisp_processor_destroy.argtypes = (ctypes.c_void_p,)
lib.wisp_processor_destroy.restype = None
lib.wisp_processor_process_one.argtypes = (ctypes.c_void_p, ctypes.POINTER(ctypes.c_float), ctypes.c_size_t)
lib.wisp_processor_process_one.restype = None
lib.wisp_processor_process_all.argtypes = (ctypes.c_void_p, ctypes.POINTER(ctypes.c_float), ctypes.c_size_t)
lib.wisp_processor_process_all.restype = None

class TwistedWispEngine:
    def __init__(self):
        self.__wisp = lib.wisp_engine_create()

    def __del__(self):
        lib.wisp_engine_destroy(self.__wisp)

    def engine_compile_signal_processor(self, function):
        sp = lib.wisp_engine_compile_signal_processor(self.__wisp, function)
        if sp == 0:
            return None
        return TwistedWispProcessor(sp)

    def context_set_main_function(self, function_name):
        lib.wisp_context_set_main_function(self.__wisp, function_name)

    def context_update(self):
        lib.wisp_context_update(self.__wisp)

class TwistedWispProcessor:
    def __init__(self, signal_processor):
        self.__processor = signal_processor

    def __del__(self):
        lib.wisp_processor_destroy(self.__processor)

    def process_one(self, buffer):
        lib.wisp_processor_process_one(self.__processor, buffer, len(buffer) / 4)

    def process_all(self, buffer):
        lib.wisp_processor_process_all(self.__processor, buffer, len(buffer) / 4)
