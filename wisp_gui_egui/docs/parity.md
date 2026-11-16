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
| Runtime loss/shutdown | runtime transport and service recovery tests | partial; callback resource retirement under audit |
| Conditional code outputs | compiler | blocked at baseline; both branches assigning `#out` can return wrong result |
| C/Python handoff | c_api / tests | blocked at baseline; raw processor read leaves duplicate owning pointer |
| Nested flow instances | root-only setters | M6; repeated instance addressing not available at baseline |
| Inline graph editing | lab wheel reproduction | blocked on public snarl routing experiment; lab remains read-only |

Unsupported baseline paths: production inline nesting, resampling, persisted
per-instance overrides, verified Windows/macOS distributions. LLVM debug logging
is diagnostic and is not a real-time feature. Fresh processor installs intentionally
reset DSP state; no untyped migration is used.
