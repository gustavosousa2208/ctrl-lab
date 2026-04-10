# ctrl-lab

Small open-source environment for drawing, simulating, and eventually deploying control-system block diagrams.

## Stack

- Vite
- TypeScript
- React
- React Flow
- Tauri v2
- Bun

## Getting Started

```bash
bun install
bun run dev
```

## Tauri Desktop

This folder contains both the Vite frontend and the Tauri shell in [`src-tauri/`](./src-tauri).

### Linux or WSL2 prerequisites

Install the Linux packages Tauri documents for Ubuntu:

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

Install Rust:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Install Bun:

```bash
curl -fsSL https://bun.sh/install | bash
```

Then install project dependencies and start the desktop shell:

```bash
bun install
bun run tauri dev
```

### Important WSL2 note

Running `bun run tauri dev` inside WSL2 launches the Linux desktop build.

If you want a native Windows desktop app and Windows installers, install the Windows Tauri prerequisites as documented by Tauri:

- Microsoft C++ Build Tools with Desktop development with C++
- WebView2
- Rust on Windows

Then run the same repo from a Windows terminal and use:

```bash
bun install
bun run tauri dev
```
