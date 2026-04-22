# AGENTS.md - Audio Pipeline Monitor

## Project Overview
Terminal-based audio pipeline visualizer for Omarchy Arch Linux. Reads `/proc` filesystem to display current audio output, bitrate, and volume levels.

## Build Commands

```bash
# Build the project
cargo build

# Build for release
cargo build --release

# Run the application
cargo run

# Run with specific arguments
cargo run -- --help
```

## Test Commands

```bash
# Run all tests
cargo test

# Run a single test
cargo test <test_name>

# Run tests with output
cargo test -- --nocapture

# Run tests for a specific module
cargo test <module_name>

# Run tests in release mode
cargo test --release
```

## Lint Commands

```bash
# Check code without building
cargo check

# Run Clippy lints
cargo clippy

# Run Clippy with all features
cargo clippy --all-features

# Format code
cargo fmt

# Check formatting without modifying
cargo fmt -- --check
```

## Code Style Guidelines

### Imports
- Group imports: std, external crates, internal modules
- Use `use` statements alphabetically within groups
- Prefer explicit imports over glob imports (`use crate::*`)

### Formatting
- Use `rustfmt` with default configuration
- Maximum line length: 100 characters
- 4 spaces for indentation (tabs converted to spaces)
- Trailing commas in multi-line structs and arrays

### Types
- Use strong typing; avoid `unwrap()` and `expect()` in production code
- Return `Result<T, E>` for fallible operations
- Use `Option<T>` for nullable values
- Define custom error types using `thiserror` or `anyhow`

### Naming Conventions
- `snake_case` for functions, variables, and modules
- `PascalCase` for types, traits, and enums
- `SCREAMING_SNAKE_CASE` for constants and statics
- `CamelCase` for type parameters
- Use descriptive names; avoid abbreviations unless widely known

### Error Handling
- Use `?` operator for error propagation
- Log errors with context using `tracing` or `log`
- Never silently ignore errors
- Provide meaningful error messages for users

### Documentation
- Document all public APIs with `///`
- Include examples in doc comments where applicable
- Use `//` for internal comments explaining "why", not "what"

### Testing
- Write unit tests in the same file as the code (`#[cfg(test)]` module)
- Use `tempfile` for filesystem tests
- Mock `/proc` filesystem reads for testing
- Name tests descriptively: `test_<function_name>_<scenario>`

### Architecture
- Keep modules focused and single-purpose
- Separate concerns: parsing, visualization, UI
- Use channels for communication between threads
- Minimize global state

### Performance
- Avoid unnecessary allocations in hot paths
- Use `&str` over `String` when possible
- Buffer file reads appropriately
- Profile before optimizing

### Dependencies
- Keep dependencies minimal
- Pin versions in `Cargo.toml`
- Document why each dependency is needed

## Project Structure

```
src/
  main.rs          # Entry point
  proc_parser.rs   # /proc filesystem parsing
  audio_info.rs    # Audio data structures
  visualizer.rs    # Terminal rendering
  ui.rs            # User interface logic
  error.rs         # Error types
```

## Audio Pipeline Monitoring

The application monitors:
- `/proc/asound/` - ALSA sound card information
- `/proc/asound/card*/pcm*/sub*/status` - PCM stream status
- `/proc/asound/card*/pcm*/sub*/hw_params` - Hardware parameters

Display information:
- Active audio output device
- Current sample rate (bitrate)
- Volume levels per channel
- Stream state (running, paused, stopped)
