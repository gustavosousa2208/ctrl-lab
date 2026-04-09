# ctrl-lab

Small open-source environment for drawing, simulating, and eventually deploying control-system block diagrams.

## Stack

- Vite
- TypeScript
- React
- React Flow

## Getting Started

```bash
npm install
npm run dev
```

## Tauri Desktop

This repo is wired for Tauri v2 on top of the existing Vite app.

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

Install a modern Node.js release compatible with Vite 7, for example Node 22 via your preferred version manager.

Then install project dependencies and start the desktop shell:

```bash
npm install
npm run tauri dev
```

### Important WSL2 note

Running `npm run tauri dev` inside WSL2 launches the Linux desktop build.

If you want a native Windows desktop app and Windows installers, install the Windows Tauri prerequisites as documented by Tauri:

- Microsoft C++ Build Tools with Desktop development with C++
- WebView2
- Rust on Windows

Then run the same repo from a Windows terminal and use:

```bash
npm install
npm run tauri dev
```
