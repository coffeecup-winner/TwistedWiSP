# Native Linux acceptance ledger

Baseline: `8d4b7db` (M4 plus waveform continuity), Rust 1.100, LLVM 18,
egui/eframe 0.36.2, egui-snarl 0.12.0. No dependency upgrade is planned.

`verified` below means automated evidence unless a manual check is named.
Physical audio/MIDI, X11/Wayland desktop behavior, GPU timings and portal behavior
require separate manual acceptance; a headless run cannot establish them.

| Behavior / Godot reference | egui entry / evidence | Result / reproduction |
| --- | --- | --- |
| Load/save, geometry, unknown metadata (`flow_editor.gd`) | document/persistence/service tests | verified; native portal manual check pending |
| Selection, wiring, move, history (`flow_editor.gd`) | canvas adapter pointer/gesture tests | verified; inspector replaces resize handles intentionally |
| Control/button/toggle (`flow_node*.gd`) | node_content/service tests | verified including release/focus loss; physical feedback pending |
| Watch/graph | service telemetry and bounded painter tests | verified; 4,096 sample history, ~30 Hz poll |
| WAV references/binding | resources, decoded WAV and LLVM ownership tests | verified at root; `--data-path ./data` for beat3 example |
| MIDI learn/cancel | atomic mailbox and fake service tests | verified without device; real controller/disconnect pending |
| Runtime loss/shutdown | runtime transport and service recovery tests | verified in fake/null-device tests; owned workers join; physical device loss pending |
| Conditional code outputs | compiler | fixed; actual JIT tests cover independent branches, merged values and nested conditionals |
| Processor ownership | Rust offline/runtime integration tests | verified; public C/Python interface removed, consuming Rust installation and lifetime tests retained |
| Nested flow instances | root-only setters | deferred by user request; no new instance addressing API |
| Inline graph editing | existing lab wheel reproduction | deferred by user request; published snarl retained, lab remains read-only |

Unsupported baseline paths: production inline nesting, resampling, persisted
per-instance overrides, verified Windows/macOS distributions. LLVM debug logging
is diagnostic and is not a real-time feature. Fresh processor installs intentionally
reset DSP state; no untyped migration is used.

## Callback and shutdown audit

Commands are bounded (64 queued, at most 64 consumed per callback). An owned
request packet is retired through a preallocated 128-entry queue, including the
previous processor, input payloads and abandoned reply payloads. A full retirement
queue leaves commands pending. Service-side collection destroys them. Callback
shutdown returns the processor and queued receiver to the runtime owner; Linux
CPAL stream destruction precedes control-thread reclamation. No detached reaper
is used. Backend stream teardown must actually finish; there is no universal
shutdown deadline across drivers/platforms.

Arrays are copied/prepared on the control thread and retained in a preallocated
128-source processor store. Watch registration and bounded query storage are
prepared before submission (128 watches, up to 4096 samples each). The warmed
allocation probe verifies zero Rust allocator alloc/dealloc calls for prepared
install/scalar/array/watch/query/remove/stop operations and noise processing.
Noise uses processor-owned xorshift state seeded during compilation.

This is not a hard-real-time guarantee. Crossbeam/OS wakeups, backend stream
implementation, page faults, diagnostic logging and generated debug instructions
remain outside that claim. Compile/decode/file I/O/dialogs execute off the audio
and UI threads. Owned I/O/dialog/decode workers join on shutdown; cancellation
stops our dialog wait, but an OS filesystem read/atomic write/portal backend can
still delay shutdown. LLVM compilation is synchronous on the editor service and
is not forcibly interrupted. A timeout abandons a reply; it does not imply a
queued runtime operation was canceled.

## Remaining acceptance

X11/Wayland and portal behavior, 1×/2× physical displays, IME/clipboard,
GPU/presentation timing, sustained audible playback and physical MIDI/device
removal still require desktop/hardware checks. Numerical JIT, real ALSA-null
callback tests and headless input tests provide complementary automated evidence.
Nested graphs are outside the current scope. Existing nested-array playback
restrictions remain explicit. There is no new project/override format.
