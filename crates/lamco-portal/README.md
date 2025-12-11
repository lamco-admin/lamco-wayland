# lamco-portal

[![Crates.io](https://img.shields.io/crates/v/lamco-portal.svg)](https://crates.io/crates/lamco-portal)
[![Documentation](https://docs.rs/lamco-portal/badge.svg)](https://docs.rs/lamco-portal)
[![License](https://img.shields.io/crates/l/lamco-portal.svg)](https://github.com/lamco-admin/lamco-wayland/tree/main/lamco-portal)

High-level Rust interface to XDG Desktop Portal for Wayland screen capture and input control.

## Features

- **Screen Capture**: Capture monitor or window content through PipeWire streams
- **Input Injection**: Send keyboard and mouse events to the desktop
- **Clipboard Integration**: Portal-based clipboard for remote desktop scenarios
- **Multi-Monitor**: Handle multiple displays simultaneously
- **Flexible Configuration**: Builder pattern and struct literals
- **Typed Errors**: Match and handle specific error conditions

## Requirements

- Wayland compositor (GNOME, KDE Plasma, Sway, etc.)
- `xdg-desktop-portal` installed and running
- Portal backend for your compositor:
  - GNOME: `xdg-desktop-portal-gnome`
  - KDE: `xdg-desktop-portal-kde`
  - wlroots: `xdg-desktop-portal-wlr`
- PipeWire for video streaming

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
lamco-portal = "0.1"
tokio = { version = "1", features = ["full"] }
```

Basic usage:

```rust
use lamco_portal::{PortalManager, PortalConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create portal manager with default config
    let manager = PortalManager::with_default().await?;

    // Create session (triggers permission dialog)
    let session = manager.create_session("my-session".to_string(), None).await?;

    // Access PipeWire FD for video capture
    let fd = session.pipewire_fd();
    let streams = session.streams();

    println!("Capturing {} streams on PipeWire FD {}", streams.len(), fd);

    // Inject mouse movement
    manager.remote_desktop()
        .notify_pointer_motion_absolute(
            session.ashpd_session(),
            0,      // stream index
            100.0,  // x position
            200.0,  // y position
        )
        .await?;

    Ok(())
}
```

## Configuration

Customize Portal behavior:

```rust
use lamco_portal::{PortalManager, PortalConfig};
use ashpd::desktop::screencast::CursorMode;
use ashpd::desktop::PersistMode;

let config = PortalConfig::builder()
    .cursor_mode(CursorMode::Embedded)  // Embed cursor in video
    .persist_mode(PersistMode::Application)  // Remember permission
    .build();

let manager = PortalManager::new(config).await?;
```

## Error Handling

Handle specific error conditions:

```rust
use lamco_portal::PortalError;

match manager.create_session("session-1".to_string(), None).await {
    Ok(session) => {
        println!("Session created successfully");
    }
    Err(PortalError::PermissionDenied) => {
        eprintln!("User denied permission");
    }
    Err(PortalError::PortalNotAvailable) => {
        eprintln!("Portal not installed - install xdg-desktop-portal");
    }
    Err(e) => {
        eprintln!("Other error: {}", e);
    }
}
```

## Examples

See the `examples/` directory for complete examples:

- `basic.rs` - Simple screen capture setup
- `input.rs` - Input injection (keyboard/mouse)
- `clipboard.rs` - Clipboard integration

Run examples with:

```bash
cargo run --example basic
```

## Platform Support

| Platform | Status | Backend Package |
|----------|--------|----------------|
| GNOME (Wayland) | ✅ Supported | xdg-desktop-portal-gnome |
| KDE Plasma (Wayland) | ✅ Supported | xdg-desktop-portal-kde |
| Sway / wlroots | ✅ Supported | xdg-desktop-portal-wlr |
| X11 | ❌ Not Supported | Wayland only |

## Security

This library triggers system permission dialogs. Users must explicitly grant:

- **Screen capture access** (which monitors/windows to share)
- **Input injection access** (keyboard/mouse control)
- **Clipboard access** (if using clipboard features)

Permissions can be remembered per-application using `PersistMode::Application` to skip the dialog on subsequent runs.

## Architecture

```
┌─────────────────┐
│ Your Application│
└────────┬────────┘
         │
         v
┌─────────────────┐
│  lamco-portal   │
│  (this crate)   │
└────────┬────────┘
         │
         v
┌─────────────────┐
│     ashpd       │ ← Low-level Portal bindings
└────────┬────────┘
         │
         v
┌─────────────────┐
│ xdg-desktop-    │
│    portal       │ ← System Portal service
└────────┬────────┘
         │
    ┌────┴────┬─────────────┐
    v         v             v
┌────────┐┌────────┐  ┌──────────┐
│PipeWire││D-Bus   │  │Compositor│
└────────┘└────────┘  └──────────┘
```

## Troubleshooting

### "Portal not available" error

**Solution**: Install xdg-desktop-portal and the appropriate backend:

```bash
# Arch Linux
sudo pacman -S xdg-desktop-portal xdg-desktop-portal-gnome

# Ubuntu/Debian
sudo apt install xdg-desktop-portal xdg-desktop-portal-gnome

# Fedora
sudo dnf install xdg-desktop-portal xdg-desktop-portal-gnome
```

### "User denied permission" error

**Solution**: This is expected behavior when the user clicks "Cancel" in the permission dialog. Handle it gracefully in your application.

### No streams available

**Causes**:
- User denied screen access
- No monitors/windows available to share
- Portal backend not running

**Solution**: Check that your portal backend is running and try again.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Related Projects

- [ashpd](https://github.com/bilelmoussaoui/ashpd) - Low-level Portal bindings
- [pipewire-rs](https://gitlab.freedesktop.org/pipewire/pipewire-rs) - PipeWire bindings
- [xdg-desktop-portal](https://github.com/flatpak/xdg-desktop-portal) - Portal specification
