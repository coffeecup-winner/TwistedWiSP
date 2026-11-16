# C / Python ownership

Engine/config/processor handles are owning allocations; destroy each exactly once.
A processor retains its LLVM context and sample allocations independently of its
compiling host. All calls require valid, uniquely accessed handles. Arbitrary
stale pointers cannot be validated by this ABI. Raw metadata handles are borrowed
from their engine and must not survive mutation or destruction.

`wisp_engine_switch_to_signal_processor` consumes a nonnull processor when the
engine is nonnull, including failed runtime installation. Its former `ptr.read()`
implementation was unsafe: callers must never destroy/process a transferred handle.
A null engine leaves the processor untouched.

Prefer additive `wisp_engine_take_signal_processor(engine, &processor)`: it clears
the caller's handle when ownership transfers. A false return with a null handle
means installation failed after consumption; a false return with the original
handle means null arguments. The Python wrapper uses this contract and refuses
processing after transfer. These APIs preserve existing exported signatures.

Free the owned string from `wisp_context_load_flow_from_file` with
`wisp_string_destroy`. Null strings mean failure. Ordinary null/undersized frame
arguments to legacy process functions are ignored without unwinding across C;
Rust hosts can use `process_one_checked` / `process_all_checked` for errors.
Non-null output pointers must still describe valid writable storage of the stated
length. `output_channels` gives the required interleaved frame size.
