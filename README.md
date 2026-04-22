# AudioRep

Audio pipeline monitor and live spectrum visualizer for Linux and macOS terminals.

## Features

- **Playback stream discovery** - Shows active audio devices and their sources (apps playing audio)
- **Live spectrum visualization** - Real-time FFT-based frequency display
- **Per-app source detection** - See which applications are producing audio (Music, Spotify, browsers, etc.)
- **Output rate control** - Switch sample rates on the fly (Linux: PipeWire, macOS: CoreAudio)
- **Hide stopped streams** by default, toggle with `h`
- **Spectrum sensitivity** and **peak decay** controls
- **Desktop launcher** support for terminal-based launch (Linux)

## Platform Support

| Feature | Linux | macOS |
|---------|-------|-------|
| Device discovery | ✅ ALSA `/proc/asound` | ✅ `system_profiler` |
| Per-app sources | ✅ PipeWire sink inputs | ✅ CoreAudio process list |
| Spectrum capture | ✅ PipeWire monitor | ✅ BlackHole virtual device |
| Rate switching | ✅ `pw-metadata` | ✅ CoreAudio property |
| Volume display | ✅ Per-channel | ❌ Not available |

## Requirements

### Linux

- Linux with ALSA `/proc/asound`
- PipeWire or PulseAudio with `pactl` and `parec` available
- A terminal emulator for the TUI launcher
- Rust and Cargo for building from source

### macOS

- macOS 12.0+ (Monterey or later)
- [Homebrew](https://brew.sh) installed
- Terminal with Unicode support (iTerm2, Terminal.app, or Alacritty)
- **BlackHole** virtual audio driver (for spectrum)

## Installation

### macOS - Install BlackHole (Required for Spectrum)

Spectrum visualization requires a virtual audio device to capture system audio:

```bash
brew install blackhole-2ch
```

After installation:
1. Open **Audio MIDI Setup** (search in Spotlight)
2. Click the **+** button and select **Create Multi-Output Device**
3. Check both **MacBook Pro Speakers** (or your main output) and **BlackHole 2ch**
4. Right-click the multi-output device and select **Use This Device for Sound Output**

### Build from Source

```bash
# Clone the repository
git clone https://github.com/yourusername/audiorep.git
cd audiorep

# Build release binary
cargo build --release

# Run
./target/release/audiorep
```

### Install to PATH

```bash
# User install (Linux)
install -Dm755 "target/release/audiorep" "$HOME/.local/bin/audiorep"

# System-wide install (Linux)
sudo install -Dm755 "target/release/audiorep" "/usr/local/bin/audiorep"
sudo install -Dm644 "audiorep.desktop" "/usr/share/applications/audiorep.desktop"
sudo install -Dm644 "audiorep.svg" "/usr/share/icons/hicolor/scalable/apps/audiorep.svg"
sudo update-desktop-database "/usr/share/applications"

# macOS
cp target/release/audiorep /usr/local/bin/
# Or use cargo install
cargo install --path .
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

### Command Line Options

```bash
audiorep --help     # Show help information
audiorep --version  # Show version information
```

### Controls

| Key | Action |
|-----|--------|
| `q` | Quit |
| `h` | Toggle hidden/stopped devices |
| `↑/↓` | Navigate devices |
| `+`/`-` | Adjust spectrum sensitivity |
| `[`/`]` | Adjust spectrum decay |
| `j`/`k` | Change output sample rate |

## Desktop Launcher (Linux)

The included `audiorep.desktop` file launches the app with Kitty:

```ini
Exec=kitty -e /usr/local/bin/audiorep
Icon=/usr/share/icons/hicolor/scalable/apps/audiorep.svg
```

If you prefer another terminal, edit that line before installing the desktop file.

## Troubleshooting

### No Spectrum Displayed (macOS)

1. Verify BlackHole is installed:
   ```bash
   ls /Library/Audio/Plug-Ins/HAL/BlackHole2ch.driver
   ```

2. Restart CoreAudio:
   ```bash
   sudo killall coreaudiod
   ```

3. Check that BlackHole appears in audio devices:
   ```bash
   system_profiler SPAudioDataType | grep -i blackhole
   ```

4. Ensure audio is playing and output is set to the multi-output device

### Terminal Title Keeps Changing

The app sets the terminal title to "audiorep" on startup. If your terminal emulator overrides this, check your terminal settings.

### High CPU Usage

The spectrum capture thread uses CPU for FFT calculations. If CPU usage is too high:
- Reduce terminal size (fewer bars to render)
- Lower the refresh rate (modify `refresh_interval` in `src/ui.rs`)

### Rate Control Not Working

**macOS**: Rate control requires the app to have permission to modify audio devices. If rate switching fails:
- Check that the output device supports the target rate
- Try switching rates manually in Audio MIDI Setup

**Linux**: Ensure PipeWire is running and `pw-metadata` is available.

## Uninstall

```bash
# Remove binary
rm /usr/local/bin/audiorep

# Remove BlackHole (macOS, optional)
brew uninstall blackhole-2ch
```

## Development

```bash
# Run tests
cargo test

# Run with debug output
RUST_LOG=debug cargo run

# Format code
cargo fmt

# Run lints
cargo clippy --all-features
```

## License

[Your License Here]