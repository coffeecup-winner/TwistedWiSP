# Linux build and packaging

Build with the repository's Rust toolchain, LLVM 18, C/C++ tools, pkg-config,
ALSA and Linux windowing/udev development libraries. Set `LLVM_SYS_180_PREFIX`
for a custom LLVM installation. The GUI uses native WGPU, X11/Wayland and XDG
portal dialogs; manual path entry is also available.

From the repository root:

```sh
cargo build --release -p wisp_gui
python3 scripts/package-linux.py /tmp/twistedwisp-linux
```

The packaging script uses Python's standard library and cached Cargo dependencies
(`--locked --offline`). Its destination must be new; `--debug` selects a debug build.
The installation includes:

- `bin/wisp_gui`
- `share/twistedwisp/core`, examples and documentation
- Project/dependency license notices and `shared-libraries.txt` from `ldd`

The package is dynamically linked. Check its shared-library requirements on the
target machine and review `THIRD-PARTY.md` and bundled notices before distribution.
Sample WAV assets are not packaged.

Core lookup uses `--core-lib-path`, then `WISP_CORE_PATH`, then the executable's
`../share/twistedwisp/core`, followed by development checkout locations. Verify a
moved installation from another working directory with:

```sh
/path/to/installation/bin/wisp_gui --check-resources
```

See the [GUI README](../README.md) for usage and test commands. Physical audio/MIDI,
portal behavior and display scaling need testing on the target desktop;
resource checks and headless tests do not cover them.
