# Reproducible UI CPU measurements

Command: `cargo run --release --locked --offline -p wisp_gui --example benchmark -- wisp/core`.
Measured in a Debian 13 x86_64 KVM container on an AMD EPYC virtual CPU
(32 online vCPUs), Rust 1.100 / LLVM 18, egui 0.36.2 / published snarl 0.12.0.
No GPU, compositor or presentation backend participates. These are UI CPU times;
GPU/driver, vsync, audio rate and audio buffer size are not applicable to this
headless workload. They do not establish end-to-end frame or audible latency.

Each valid deterministic fixture is generated before timing. 50 warmup frames,
300 measured frames, 1280×800 logical pixels at display scale 1×; scripted time
advances at 60 Hz. Wheel mode injects alternating horizontal wheel input. Scope
mode has 16 graphs plus one control, bounded 4096-sample histories, and appends
32 samples every two frames, including snapshot copying in measured UI cost.
This characterizes the renderer; actual audio polling/transport is tested separately.

| Nodes / wires | Input | p50 ms | p95 ms |
| --- | --- | ---: | ---: |
| 100 / 200 | idle | 0.860 | 2.876 |
| 100 / 200 | wheel | 0.897 | 0.929 |
| 1000 / 2000 | idle | 11.009 | 17.001 |
| 1000 / 2000 | wheel | 12.247 | 15.286 |
| 16 scopes + control / 16 | idle, animated data | 0.184 | 0.207 |
| 16 scopes + control / 16 | wheel, animated data | 0.187 | 0.211 |

Process high-water RSS: 29,032 KiB; final RSS: 16,256 KiB. This shared virtualized
host is subject to scheduling noise; one short run is not a performance guarantee.
The normal graph's measured CPU p95 is below 16.7 ms; presentation acceptance
remains pending. Large graphs show significant scaling cost. This change adds
measurement and does not claim a measured optimization or maximum supported size.

Pending: interactive multi-selection drag, GPU/presentation timing, 2× DPI,
sustained playback and a long-duration resource-growth test. Run the same fixtures
on the target desktop before setting a supported scale or release frame-time claim.
