# Contributing

The original author is not planning further updates. Community contributions and
independent forks are welcome under the MIT license. Review, merging and future
releases depend on maintainer availability; no response time is promised. This
repository has not been transferred to a new maintainer.

Start with a focused issue describing the problem, Windows build, GPU driver,
recording settings, and steps to reproduce. Remove personal paths and screen
content from any reports. Do not upload a recording containing private material.

Use stable Rust pinned in `rust-toolchain.toml` and the MSVC x64 target. See the
README for Windows SDK and C++ build prerequisites.

Before submitting changes:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
```

Changes to capture, encoding, timestamps, or resource lifetime need real desktop
validation. A hosted CI runner does not prove that capture works on physical GPUs.
Use `tests/validate-recording.ps1` with a locally built CLI and a moving, nonprivate
screen. Include the test duration, dropped frames, actual encoder and decode result.

Keep the capture callback free of disk I/O and CPU readbacks. Preserve the bounded
texture pool. Do not reuse a texture before the encoder releases its tracked sample.
Explain any unsafe changes, COM ownership, thread model, and cleanup path.

Performance claims need the reproducible procedure in `benchmarks/README.md`.
Do not add telemetry, advertisements, accounts, or cloud dependencies.

Contributions are licensed under the MIT license.
