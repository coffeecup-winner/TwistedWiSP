# Native egui editor — milestones 1–2

From the repository root:

```sh
cargo run -p wisp_gui_egui -- --flow wisp_gui/control.twf
cargo run -p wisp_gui_egui -- --flow wisp_gui/control.twf --offline
```

Startup is stopped and does not open audio/MIDI. **Start DSP** lazily opens audio, compiles the authoritative flow, installs it, restores saved scalar values, then starts playback. Sliders update DSP without recompilation. During a drag, outdated queued values collapse to the newest value instead of trailing behind the pointer. The GUI requests a 256-frame audio buffer by default (about 5.8 ms per block at 44.1 kHz), clamped to device support; `--audio-buffer-size` overrides it. Actual response also depends on the device/driver output buffering. Stop outputs silence while keeping the callback available for commands. Audio failures are shown in the editor; offline edits remain available. MIDI is optional.

`--offline` forbids device initialization. `--help` lists core, audio host/device/channel/buffer/rate and MIDI options. `--core-lib-path PATH` selects another core directory. Repeatable `--data-path` is reserved for milestone 4; WAV binding is not implemented. Flows needing arrays/buffers remain viewable but cannot start DSP.

The graph displays actual `.twf` ports/wires and saved geometry. Use **New** with a unique alphanumeric/underscore name, or **Open** an existing flow. Edits stay in memory; saving is milestone 3. Replacing or closing an edited document offers discard/cancel. A failed load retains the previous document and playback.

- **N** over the graph, or right-click empty space → **Add node…**, opens the searchable function/expression picker at the cursor. Expressions start with `=`. Apply commits; rejected drafts remain editable. Function entries show their kind/port counts, with port details on hover.
- Drag between output/input pins to connect. Float inputs sum multiple sources; incompatible types, array fan-in and feedback without a lag are rejected. Right-click a wire to disconnect that exact edge, or right-click a pin to disconnect all attached edges. The node menu also offers **Disconnect all**. Command/Ctrl-drag previews/copies attached connections; accepted wires remain until explicitly disconnected, including after a canceled drag.
- Use snarl's native selection (Shift-drag a selection rectangle; Shift-click node headers adds to selection; Command/Ctrl-click removes from selection). Drag selected node headers to move the group. **Snap (20)** snaps the group's anchor to a 20-unit graph grid on release, preserving relative offsets regardless of zoom.
- **Delete/Backspace** over the graph deletes the selection and incident wires together. Right-click a node for **Replace…** or **Delete**. Replacement preserves identity/properties; disconnect affected wires first if the new ports are incompatible.
- **Undo/Redo**, Command/Ctrl-Z, Command/Ctrl-Shift-Z or Command/Ctrl-Y restore accepted edits. Graph shortcuts leave text fields/picker drafts alone. Slider drags and group movement form single history entries. Each history direction retains at most 64 states / 8 MiB; pan/zoom/selection are view state.

Structural edits while DSP is enabled stop, compile, install, initialize controls and resume on the existing device. Compile failure keeps the edited document visible and the last good processor stopped; correct the graph or Undo to recover. Device/install/initialization failure leaves editing/history available; retry Start. Geometry and scalar changes do not compile. Expressions are bounded to 8,192 bytes, at most 64 opening parentheses and argument indices `$0`–`$1023`; malformed/nonfinite literals are rejected.

Use **Open…**, **Save**, **Save As…**, or Command/Ctrl-O, S, Shift-S and N for file actions. The existing Flow path field with **Open** and **Save to path…** work without a desktop portal. New/Open/window close on a dirty document offers Save/Discard/Cancel. A save captures accepted edits; edits made while it writes remain unsaved. External changes require confirmation before overwriting. Files are replaced through a synced sibling temporary file; symlink destinations must be selected by their target path explicitly.

The **Inspector** edits the single selected node's x/y/width/height and control value. Enter or focus loss commits numeric typing; Escape cancels typing. Drags form one Undo entry. Coordinates use legacy integer geometry, minimum sizes are 80×60, and controls use 0–1 in 0.001 steps. Width/height are renderer size hints: snarl's header/pins can require more space, and interactive resize handles are not provided.

Buffer references are preserved and diagnosed without enabling playback. Relative paths resolve against the owning flow directory, then `--data-path` entries in order. Save As rebases resolved references to preserve the target; unresolved strings and absent-path builtin entries remain unchanged. Flattened node metadata supports the backend's i32/f32/string types; unsupported types fail loading with a property-specific error rather than being silently discarded.

**Canvas lab** is separate synthetic state. It contains waveform painting from a bounded 2,048-sample ring, a highlighted multiline editor, mixed controls, and a one-level child snarl. Scale buttons exercise 0.25×–2×. The child is explicitly read-only: the public-API nesting experiment routes wheel input over the child to the parent. See the reproducible headless test and completion report in [milestone1.md](../gui_plan/milestone1.md). No snarl fork or vendoring is used.

Dependencies: egui/eframe 0.36.2, egui-snarl 0.12.0, rfd 0.15.4 (MIT, GTK-free XDG portal/async-std); Rust stable (tested 1.100.0), LLVM 18, and native Linux audio/windowing development libraries. Set `LLVM_SYS_180_PREFIX` for a nonstandard LLVM installation. The existing Qwen package name and edition 2024 are retained.

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
