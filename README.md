# Twisted WiSP

## Overview

Twisted WiSP is (aiming to be) an signal processing exploration tool, similar to Pure Data or Max/MSP.

## Architecture overview

NOTE: This overview reflects the current architecture rework goal, not the actual architecture state.

Twisted WiSP provides a native Rust library for offline and live signal processing, used directly by the GUI and CLI. In the future a C FFI layer might be added (when/if necessary).

The library is designed to be a backend to a GUI (or CLI) frontend application, so it supports high-level concepts (e.g. undo/redo for working with flow graphs processing) that would be useful when building such applications.

### Core design concepts

There is a custom intermediate language focused on signal processing. This language makes it easier to work with input/output channels and is higher level than LLVM IR.

There are several ways to generate the Twisted WiSP IR:
  - Assembly-like IR syntax, directly compiling into IR
  - Flow graphs, compiled into IR by a flow graph compiler
  - Math expressions, compiled into IR by a math expression compiler

The signal processing chain is never interpreted, always JIT-compiled on the target machine using LLVM.

## Tests

Run the device-free Rust suites from the repository root:

```sh
cargo test --workspace
```

Public backend integration tests live in `wisp/tests/offline.rs` and
`wisp/tests/runtime.rs`. They use inline flow definitions, independent of GUI
examples. Compiler and runtime implementation tests remain beside their modules.
The live-runtime test runs in a subprocess and is ignored by default because it
requires an audio output device:

```sh
cargo test -p twisted-wisp --test runtime -- --ignored
```

On Linux, set `ALSA_CONFIG_PATH` to an ALSA null PCM configuration to run the
live-runtime test silently; see [the GUI README](wisp_gui/README.md).

There is no public C API, generated header, or Python test dependency. Internal
JIT calls still use the C calling convention to invoke Rust builtins.
