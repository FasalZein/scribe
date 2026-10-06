# Support tiers and a glibc 2.28 floor

scribe runs local models on other people's machines, but only macOS on Apple Silicon and Linux on the CPU (arm64 and emulated x86_64, both in OrbStack containers) have ever run it.

Decision:

- **Supported**, tested before each release: macOS on Apple Silicon (Metal), and Linux x86_64 and arm64 on the CPU.
- **Best effort**: Windows, Intel Macs (cargo build), and the Vulkan and CUDA builds.
- Linux release binaries are built against glibc 2.28, so Debian 10+, Ubuntu 20.04+ and RHEL 8+ run them. The ubuntu-22.04 build needed glibc 2.35 and excluded RHEL 9.
- `scribe doctor` reports the backend, GPU, memory, tool versions and cache state, and runs a short self-test. `install.sh` runs it, so a broken backend shows before the first real source.
- The README carries a support table with memory and CPU-speed guidance, including 8 GB machines.
