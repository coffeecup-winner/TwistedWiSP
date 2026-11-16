# Linux build and distribution

Build with stable Rust, LLVM 18 (`LLVM_SYS_180_PREFIX` for a custom installation),
C/C++ tools, pkg-config, ALSA and Linux windowing/udev development libraries.
Use the repository Dockerfile as the versioned development environment reference.
The application uses WGPU with native X11/Wayland support and XDG portal dialogs;
portal-free path entry remains available. No web runtime is required.

Run `python3 scripts/package-linux.py /tmp/twistedwisp-linux` to build a release
installation containing `bin/wisp_gui_egui`, `share/twistedwisp/core`, examples,
maintained docs, dependency notices/license sources and `ldd` inspection results.
`--debug` produces a development smoke artifact, not a release distribution.
The destination must be new. Review `THIRD-PARTY.md` and license files before
redistribution; build-time and transitive licenses are included. No sample audio
with unreviewed provenance is bundled. The package is dynamically linked, not a
universal/static binary: inspect `shared-libraries.txt` on the target machine.

Core lookup: explicit `--core-lib-path`, then `WISP_CORE_PATH`, then the executable's
`../share/twistedwisp/core`, then development checkout locations. Launch from any
working directory, including after moving the whole installation. Verify with
`bin/wisp_gui_egui --check-resources` (no display/devices). Examples live in
`share/twistedwisp/examples`. To run the repository beat example, add
`--flow wisp_gui/beat3.twf --data-path data` from the checkout root.

The generated C header is Cargo output at
`<target>/<profile>/build/twisted-wisp-*/out/twisted_wisp.h`, never a hardcoded
checkout target directory. Cargo metadata supplies the configured target path.
`run_tests.py` builds first and exits on failure, then loads the exact resulting
shared library. Install pytest/NumPy in your chosen Python environment; Linux
headless ABI tests can use the ALSA null config described in the GUI README.

Benchmark: `cargo run --release -p wisp_gui_egui --example benchmark -- wisp/core`.
Fixed valid fixtures are generated before timing; each scenario warms 50 frames
and measures 300 at 1280×800 / 1×. Output is UI CPU time, not presentation/GPU
latency. It covers 100/200, 1000/2000 nodes/wires, 16 scopes, idle and scripted
wheel input. Interactive multi-selection drag/GPU/vsync, 2× DPI and sustained
physical playback still require desktop acceptance. Do not compare this headless
number directly to the 16.7 ms presentation target.

License review of the locked Linux GUI graph: native dependencies are generally
MIT/Apache/BSD/ISC/Zlib/Unicode licensed. `self_cell` offers Apache-2.0 OR GPL;
choose Apache-2.0. `cbindgen` is MPL-2.0 and is a build tool, not linked application
code. Preserve its notices if redistributing that tool/source. Unicode data/code
notices are included. System libraries such as ALSA/LLVM have separate distribution
terms; this package records dynamically required libraries rather than bundling
them. MIT/Apache project notices are included for both workspace crates.
