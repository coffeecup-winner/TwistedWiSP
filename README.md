# Twisted WiSP

## Overview

Twisted WiSP is (aiming to be) an signal processing exploration tool, similar to Pure Data or Max/MSP.

## Architecture overview

NOTE: This overview reflects the current architecture rework goal, not the actual architecture state.

Twisted WiSP is compiled as a single native library that has a C API interface for working with the signal processing engine. The library can perform offline or live signal processing duties.

The library is designed to be a backend to a GUI (or CLI) frontend application, so it supports high-level concepts (e.g. undo/redo for working with flow graphs processing) that would be useful when building such applications.

### Core design concepts

There is a custom intermediate language focused on signal processing. This language makes it easier to work with input/output channels and is higher level than LLVM IR.

There are several ways to generate the Twisted WiSP IR:
  - Assembly-like IR syntax, directly compiling into IR
  - Flow graphs, compiled into IR by a flow graph compiler
  - Math expressions, compiled into IR by a math expression compiler

The signal processing chain is never interpreted, always JIT-compiled on the target machine using LLVM.
