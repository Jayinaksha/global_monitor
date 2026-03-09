# Global Monitor

A real-time global aircraft tracking visualizer built with Rust, [egui](https://github.com/emilk/egui)/[eframe](https://github.com/emilk/egui/tree/master/crates/eframe), and GPU-accelerated rendering via [wgpu](https://wgpu.rs/).

Aircraft positions are fetched live from the [OpenSky Network](https://opensky-network.org/) public API and rendered on a world map with flight-trail history.

## Features

- **Live aircraft data** – polls the OpenSky Network every 5 seconds
- **GPU-accelerated rendering** – aircraft dots and trail segments are drawn with a custom wgpu render pipeline for smooth performance even with thousands of simultaneous flights
- **CPU fallback** – automatically falls back to egui's CPU painter on systems where wgpu is unavailable
- **Pan & zoom** – drag to pan the map; scroll to zoom in/out (clamped to 0.2×–8×)
- **Flight trails** – each aircraft retains its last 6 positions, drawn as line segments
- **Frustum culling** – off-screen aircraft and trail segments are skipped before being sent to the GPU

## Prerequisites

| Requirement | Version |
|---|---|
| Rust (stable) | 1.85+ (`edition = "2024"`) |
| A GPU with Vulkan, Metal, DX12, or WebGPU support | — |

On Linux the app requires either an **X11** or **Wayland** display server (both are compiled in by default via the `x11` and `wayland` eframe features).

## Building & Running

```bash
# Clone the repository
git clone https://github.com/Jayinaksha/global_monitor.git
cd global_monitor

# Build and run (release mode recommended for performance)
cargo run --release
```

The world map image is loaded at runtime from `src/assets/world_map.png`, so the binary must be run from the repository root.

## Project Structure

```
global_monitor/
├── Cargo.toml
└── src/
    ├── main.rs          # Application entry point, egui app loop, pan/zoom logic
    ├── data.rs          # OpenSky Network API client (async, serde)
    ├── gpu_renderer.rs  # wgpu pipeline: aircraft dots & trail lines
    ├── map_renderer.rs  # Map texture helpers
    ├── camera.rs        # Camera / projection utilities
    ├── app.rs           # Additional app state helpers
    ├── aircraft.wgsl    # WGSL shader for aircraft & trail rendering
    └── assets/
        └── world_map.png
```

## Dependencies

| Crate | Purpose |
|---|---|
| `eframe` / `egui` | Immediate-mode GUI framework |
| `egui-wgpu` | wgpu integration for egui paint callbacks |
| `wgpu` | Low-level GPU API |
| `bytemuck` | Safe byte-casting for GPU vertex buffers |
| `reqwest` | Async HTTP client |
| `serde` / `serde_json` | JSON deserialization of OpenSky responses |
| `tokio` | Async runtime |
| `image` | PNG decoding for the world map texture |
| `log` | Logging façade |

## License

This project is provided as-is. See repository for license details.
