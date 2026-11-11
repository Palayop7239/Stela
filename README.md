> [!NOTE]
> **This is a fork of [tylertms/Stela](https://github.com/tylertms/Stela) that works on AMD Radeon support.** For releases, installs and the project itself, use upstream. Nothing here is an official Stela build.

## AMD Radeon work in this fork

The goal is to run Stela's Metal driver on AMD Radeon GPUs, starting with RDNA2 (Radeon RX 6000), the way the NVIDIA path works today. AMD's Linux kernel driver (amdgpu) runs in Stela's Linux environment, and Mesa's RADV Vulkan driver sits under Stela's Metal layer.

The test machine has a Ryzen 7 5700X, an ASUS TUF GAMING B450-PLUS II and a Radeon RX 6650 XT. On it, macOS 27 installs and starts up to Stela's GPU check.

| Step | Status |
|---|---|
| Metal layer accepts a RADV device (RX 6000 and newer), compiled at wave32 | Done, not yet run on hardware |
| amdgpu compiles in Stela's Linux environment, headless | 374 of 376 sources build |
| Kernel services amdgpu needs (PCI, DMA, IRQ, time, …) | In progress |
| amdgpu starts on the card: firmware, PSP, first ring test | Not started |
| RADV built next to NVK | Not started |
| Display (kernel FPU support, display core) | Not started |
| macOS starts on a Radeon | Not started |

The work is on the [`amd-gpu-0.2`](https://github.com/Palayop7239/Stela/tree/amd-gpu-0.2) branch, which is based on v0.2.0.

These fixes came out of this machine and were merged upstream:
- [#5](https://github.com/tylertms/Stela/pull/5): the Realtek RTL8168H stopped receiving partway through a download
- [#6](https://github.com/tylertms/Stela/pull/6): an RTL8111H was refused as a fiber (TBI) adapter
- [#7](https://github.com/tylertms/Stela/pull/7): SPTM panicked after a warm restart

---

<h1 align="center">
  <img src="assets/stela-icon.png" alt="" width="96"><br>
  Stela
</h1>

A general purpose Apple Silicon layer for x86-64 PCs. It is a UEFI boot
program with an arm64-to-x86-64 translator, a Metal driver for NVIDIA GPUs and
a model of Apple silicon hardware. macOS and most apps start, but it is slow and needs
much more testing, as of v0.1.0

> [!WARNING]
Stela is very much still in development. Your current chances of it booting are low, major hardware support/fixes are on the way.

## Translator

An ahead-of-time and just-in-time compiler translates arm64e code to x86-64.
It covers more than 3,000 AArch64 instruction encodings, maps guest registers
to host registers and caches translated code. It runs on any x86-64-v2 CPU,
Intel or AMD.

## NVIDIA driver

The Metal driver supports GTX 16 and RTX 20 series GPUs and newer. It compiles
AIR, Metal's shader IR, directly to NIR, Mesa's IR, and supports the Metal3
and Apple7 GPU families.

## Requirements

- An x86-64-v2 CPU (about 2011 or newer), UEFI with Secure Boot off, 8 GB of
  RAM.
- An NVIDIA GTX 16 or RTX 20 series GPU or newer.
- A USB stick of 8 GB or more.
- An empty GPT partition of at least 60 GB.
- Network access during the install, which downloads macOS.

Stela currently models the Mac mini (M2) and runs macOS 27.

## Install

1. Download `stela` from the
   [releases page](https://github.com/tylertms/Stela/releases).
2. On the PC that will run macOS, run `sudo ./stela setup` (Linux, macOS) or
   double-click `stela.exe` (Windows).
3. Choose the USB stick, the macOS version and the Mac model, then confirm.
   Setup erases the USB stick.
4. Boot the PC from the USB stick.

## Build

You need [Rust](https://rustup.rs) and LLVM with Clang 21 or newer.

```sh
git clone https://github.com/tylertms/Stela
cd Stela
cargo xtask dist
```

This writes `target/release/stela`. The first build fetches and compiles
Linux, Mesa and LLVM sources and takes a long time.

## Legal

Stela contains no Apple software. You are responsible for following Apple's
software license agreement and terms of use.

Stela's code is GPL-2.0-or-later. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

## Community

[Discord Server](https://discord.gg/aSkwrJEMYc)
