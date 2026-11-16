# Native egui editor — milestone 1

From the repository root:

```sh
cargo run -p wisp_gui_egui -- --flow wisp_gui/control.twf
cargo run -p wisp_gui_egui -- --flow wisp_gui/control.twf --offline
```

Startup is stopped and does not open audio/MIDI. **Start DSP** lazily opens audio, compiles the authoritative flow, installs it, restores saved scalar values, then starts playback. Sliders update DSP without recompilation. Stop outputs silence while keeping the callback available for commands. Audio failures are shown in the editor; offline edits remain available. MIDI is optional.

`--offline` forbids device initialization. `--help` lists core, audio host/device/channel/buffer/rate and MIDI options. `--core-lib-path PATH` selects another core directory. Repeatable `--data-path` is reserved for milestone 4; WAV binding is not implemented. Flows needing arrays/buffers remain viewable but cannot start DSP.

The graph displays actual `.twf` ports/wires and saved geometry. Pan, zoom, node movement and control sliders are supported. Edits remain in memory; saving, topology editing and undo are later milestones. Replacing or closing an edited document offers discard/cancel. A failed load retains the previous document and playback.

**Canvas lab** is separate synthetic state. It contains waveform painting from a bounded 2,048-sample ring, a highlighted multiline editor, mixed controls, and a one-level child snarl. Scale buttons exercise 0.25×–2×. The child is explicitly read-only: the public-API nesting experiment routes wheel input over the child to the parent. See the reproducible headless test and completion report in [milestone1.md](../gui_plan/milestone1.md). No snarl fork or vendoring is used.

Dependencies: egui/eframe 0.36.2, egui-snarl 0.12.0; Rust stable (tested 1.100.0), LLVM 18, and native Linux audio/windowing development libraries. Set `LLVM_SYS_180_PREFIX` for a nonstandard LLVM installation. The existing Qwen package name and edition 2024 are retained.

```sh
cargo check --workspace
cargo test -p twisted-wisp --lib
cargo test -p wisp_gui_egui
cargo fmt --all -- --check
cargo clippy -p wisp_gui_egui --all-targets --no-deps -- -D warnings
```

The device-free suite includes real LLVM/JIT output tests. A separate ignored test exercises the native callback:

```sh
cargo test -p wisp_gui_egui native_callback_start_scalar_stop_restart -- --ignored
```

It needs an output device. On Linux it can also run silently with `ALSA_CONFIG_PATH` pointing to this file:

```text
pcm.!default {
    type null
    hint { show on description "TwistedWiSP null output" }
}
```

Null-output testing verifies callback requests and lifetime behavior, not audible playback. The completion report records the remaining physical-device and windowed visual/interaction checks.
