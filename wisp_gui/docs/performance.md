# UI benchmark

From the repository root:

```sh
cargo run --release --locked --offline -p wisp_gui --example benchmark -- wisp/core
```

The harness generates deterministic valid graphs before timing: 100 nodes / 200
wires, 1,000 nodes / 2,000 wires, and 16 animated scopes plus a control. It measures
idle and scripted wheel input at 1280×800 / 1×, with 50 warmup frames and 300
measured frames. Scope updates include UI-side snapshot copying.

Output reports p50/p95 UI CPU time and Linux process RSS. It excludes the window
server, GPU presentation and audio processing; it does not measure audible
latency or establish a supported graph-size limit.

For comparisons, record the commit, toolchain, CPU, OS and build profile. Desktop
measurements should also record GPU/driver, display scale and presentation mode,
and include selection/dragging and sustained playback. Keep captured results with
the investigation they support rather than treating old timings as current guarantees.
