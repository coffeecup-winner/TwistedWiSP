# Native GUI

An egui graph editor for Twisted WiSP. This is pre-MVP software: APIs and file
formats may change without backward compatibility.

From the repository root:

```sh
cargo run -p wisp_gui -- --flow wisp_gui/examples/control.twf
cargo run -p wisp_gui -- --flow wisp_gui/examples/beat3.twf --data-path data
```

Use `--offline` to edit without opening audio or MIDI devices. **Start DSP** opens
the devices and starts playback; **Stop** outputs silence. MIDI is optional.
Structural edits rebuild the processor; sliders update it without recompilation.
If compilation fails, playback stops and the document remains editable.

Configuration uses command-line options (`--help`). `--core-lib-path` or
`WISP_CORE_PATH` selects the core library; repeatable `--data-path` adds WAV search
directories. Audio host, device, rate, channels, buffer size and MIDI input are
configurable. The default audio buffer is 512 frames; actual latency depends on
the backend and device.

## Editing

- Press **N** over the graph or right-click empty space to add a function or
  expression. Expressions start with `=`; **Apply** or Ctrl/Cmd+Enter commits,
  Enter inserts a newline, and Escape cancels the draft.
- Drag between pins to connect. Float inputs sum multiple sources. Right-click a
  wire or pin to disconnect. Feedback requires a lag.
- Shift-drag selects a region; Shift-click adds nodes and Ctrl/Cmd-click removes
  them from the selection. Drag node headers to move; Delete/Backspace deletes.
- Ctrl/Cmd+Z undoes; Ctrl/Cmd+Shift+Z redoes. The **Inspector** edits geometry and
  control values. Node width and height are size hints, not resize handles.
- Use **Open**, **Save** and **Save As** for `.twf` flows. Unsaved edits prompt on
  replacement or close. Path entry is available when native dialogs are unavailable.
- Buffer nodes offer WAV selection, reload and a builtin sine source. Relative
  paths resolve against the flow directory, then `--data-path` directories.
- Select a control/toggle and choose **Learn CC** (or press **L**), then move a
  MIDI controller. Mappings last for the session.

Multichannel WAVs are mixed to mono; sample-rate conversion is not implemented.
Nested graph editing and nested array bindings are not supported. **Canvas lab**
is a synthetic rendering experiment, separate from the document and playback.

## Development

See [Linux build and packaging](docs/linux.md) and [UI benchmarking](docs/performance.md).

```sh
cargo check --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy -p wisp_gui --all-targets --no-deps -- -D warnings
```

Native callback tests require an audio device and are ignored by default:

```sh
cargo test -p twisted-wisp --test runtime -- --ignored
cargo test -p wisp_gui native_callback -- --ignored
```

On Linux, set `ALSA_CONFIG_PATH` to a file containing this configuration for silent
callback tests. It does not verify audible playback or physical device behavior.

```text
pcm.!default {
    type null
    hint { show on description "TwistedWiSP null output" }
}
```
