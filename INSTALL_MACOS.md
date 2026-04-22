# macOS Installation Guide

## Prerequisites

- macOS 12.0+ (Monterey or later)
- [Homebrew](https://brew.sh) installed
- Terminal with Unicode support (iTerm2, Terminal.app, or Alacritty)

## Install BlackHole (Required for Spectrum)

Spectrum visualization requires a virtual audio device to capture system audio:

```bash
brew install blackhole-2ch
```

After installation:
1. Open **Audio MIDI Setup** (search in Spotlight)
2. Click the **+** button and select **Create Multi-Output Device**
3. Check both **MacBook Pro Speakers** (or your main output) and **BlackHole 2ch**
4. Right-click the multi-output device and select **Use This Device for Sound Output**

Alternatively, use the command line:

```bash
# Create a multi-output device with BlackHole
# (Requires manual setup in Audio MIDI Setup for now)
```

## Build from Source

```bash
# Clone the repository
git clone https://github.com/yourusername/audiorep.git
cd audiorep

# Build release binary
cargo build --release

# Run
./target/release/audiorep
```

## Install to PATH

```bash
# Copy binary to a location in your PATH
cp target/release/audiorep /usr/local/bin/

# Or use cargo install
cargo install --path .
```

## Usage

```bash
# Run with default settings
audiorep

# Show help
audiorep --help

# Show version
audiorep --version
```

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `q` | Quit |
| `↑/↓` | Navigate devices |
| `h` | Toggle hidden/stopped devices |
| `+/-` | Adjust spectrum sensitivity |
| `[` / `]` | Adjust spectrum decay |
| `j` / `k` | Change output sample rate |

## Troubleshooting

### No Spectrum Displayed

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

macOS rate control requires the app to have permission to modify audio devices. If rate switching fails:
- Check that the output device supports the target rate
- Try switching rates manually in Audio MIDI Setup

## Uninstall

```bash
# Remove binary
rm /usr/local/bin/audiorep

# Remove BlackHole (optional)
brew uninstall blackhole-2ch
```

## Building from Source (Development)

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