# AudioRep

Audio pipeline monitor and live spectrum visualizer for Linux terminals.

## Features

- Playback stream discovery from `/proc/asound`
- Live output spectrum using the default PipeWire/PulseAudio monitor source
- Hide stopped streams by default, toggle with `h`
- Spectrum sensitivity and peak decay controls
- Desktop launcher support for terminal-based launch

## Requirements

- Linux with ALSA `/proc/asound`
- PipeWire or PulseAudio with `pactl` and `parec` available
- A terminal emulator for the TUI launcher
- Rust and Cargo for building from source

## Build

```bash
cargo build --release
```

## Install

### User install

```bash
install -Dm755 "target/release/audiorep" "$HOME/.local/bin/audiorep"
install -Dm644 "audiorep.svg" "$HOME/.local/share/icons/hicolor/scalable/apps/audiorep.svg"
```

### System-wide install

```bash
sudo install -Dm755 "target/release/audiorep" "/usr/local/bin/audiorep"
sudo install -Dm644 "audiorep.desktop" "/usr/share/applications/audiorep.desktop"
sudo install -Dm644 "audiorep.svg" "/usr/share/icons/hicolor/scalable/apps/audiorep.svg"
update-desktop-database "/usr/share/applications"
```

## Usage

Run the TUI directly:

```bash
audiorep
```

Or run from source:

```bash
cargo run
```

## Controls

- `q`: quit
- `h`: show or hide stopped playback streams
- `r`: refresh device list
- `+` / `-`: increase or decrease spectrum sensitivity
- `[` / `]`: decrease or increase peak decay speed
- `Up` / `Down`: move selection

## Desktop Launcher

The included `audiorep.desktop` file launches the app with Kitty:

```ini
Exec=kitty -e /usr/local/bin/audiorep
Icon=audiorep
```

If you prefer another terminal, edit that line before installing the desktop file.
