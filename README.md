# Flipper Zero PC Monitor & Soundboard Backend (USB)
Cross-platform PC backend written in Rust for **PC Monitor & Soundboard (USB Version)** on Flipper Zero.

This backend continuously streams system telemetry (CPU, RAM, GPU, and Temperatures) to the Flipper Zero while listening for D-pad/button events to trigger audio playback (soundboard).

## Features
- **Real-Time Telemetry**: Collects CPU %, RAM usage, GPU %, VRAM usage, and real-time **CPU & GPU temperatures** in °C.
- **USB Soundboard**: Receives button presses from the Flipper Zero and plays the corresponding sound files located in the `sounds/` folder.
- **Dual-Audio Output (Virtual Cable Support)**:
  - Plays audio to your default headphones/speakers.
  - Automatically detects virtual audio cables (e.g. *VB-Audio Cable* on Windows or *PipeWire/PulseAudio* virtual sinks on Linux) so sounds can be heard by friends on **Discord**, in-game voice chat, or **OBS**.
- **Auto-Detection**: Scans and connects automatically to Flipper Zero USB CDC (`0483:5740`).
- **Graceful Port Handling**: Detects busy serial ports with clear instructions.

## Soundboard Directory Structure & Setup
Place your `.mp3` files inside a folder named `sounds/` located in the same directory as the backend executable:

```text
your-folder/
├── flipper-pc-monitor-backend-usb       # (or Flipper-Zero-PC-Monitor-USB-Windows-x86_64.exe)
└── sounds/
    ├── up.mp3                           # Triggered by D-Pad Up
    ├── down.mp3                         # Triggered by D-Pad Down
    ├── left.mp3                         # Triggered by D-Pad Left
    ├── right.mp3                        # Triggered by D-Pad Right
    ├── ok.mp3                           # Triggered by OK button
    └── back.mp3                         # Triggered by Back button (short press)
```

> **Note:** If any sound file is missing, the backend will simply print a warning and continue running without interrupting the PC monitor.

| Button on Flipper | Sound File Path           | Description                |
|-------------------|---------------------------|----------------------------|
| **Up**            | `sounds/up.mp3`           | Plays on D-Pad Up          |
| **Down**          | `sounds/down.mp3`         | Plays on D-Pad Down        |
| **Left**          | `sounds/left.mp3`         | Plays on D-Pad Left        |
| **Right**         | `sounds/right.mp3`        | Plays on D-Pad Right       |
| **OK**            | `sounds/ok.mp3`           | Plays on OK button         |
| **Back** *(Short)*| `sounds/back.mp3`         | Plays on Back short press  |
| **Back** *(Long)* | *None (Exits App)*        | Hold to safely exit on Flipper |

### Streaming sounds to Discord / Games:
- **Windows**: Install [VB-Audio Virtual Cable](https://vb-audio.com/Cable/). The backend will automatically detect it and route the audio. In Discord, select *CABLE Output* as your input microphone (or mix it using VoiceMeeter).
- **Linux**: Create a virtual sink with PipeWire / PulseAudio:
  ```bash
  pactl load-module module-null-sink sink_name=CABLE sink_properties=device.description=Virtual_CABLE
  ```

## Hardware Monitoring Notes (Windows vs Linux)

- **Linux**: Temperatures are read natively out-of-the-box from standard kernel hwmon interfaces (`/sys/class/hwmon`).
- **Windows (CPU Temperature)**:
  - Due to Windows architecture, accessing direct CPU core/package registers (MSR) requires Ring 0 kernel privileges with signed drivers.
  - To view accurate real-time CPU & GPU temperatures, simply have [Libre Hardware Monitor](https://librehardwaremonitor.net/) (or [Open Hardware Monitor](https://openhardwaremonitor.org/)) running in the background. It is open-source, portable (no installation needed), and can be minimized to the system tray.
  - The backend automatically connects to the Libre Hardware Monitor WMI service to retrieve dynamic temperatures.
  - *If Libre Hardware Monitor is not open, all other telemetry (CPU %, RAM %, GPU %, VRAM %) and the soundboard will continue to work normally.*

## How to Run

### Using Pre-built Binaries
Download the binary for your platform from the [Releases](https://github.com/DonJulve/Flipper-Zero-PC-Monitor-USB-Backend/releases) tab. Ensure the `sounds/` folder with your audio files is in the same directory as the executable.

### Building from Source

#### Linux Prerequisites:
```bash
sudo apt install libdbus-1-dev libudev-dev pkg-config libasound2-dev
```

#### Run or Build:
```bash
# Run in development mode:
cargo run --release

# Or build the standalone binary:
cargo build --release
```
The resulting executable will be placed in `target/release/flipper-pc-monitor-backend-usb`.

## Credits & Acknowledgments
- **Original Backend**: [TheSainEyereg](https://github.com/TheSainEyereg/flipper-pc-monitor-backend)
- **USB Adaptation, Soundboard & Enhancements**: [DonJulve](https://github.com/DonJulve)
