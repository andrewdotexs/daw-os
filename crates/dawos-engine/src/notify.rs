//! Telling systemd that the engine is alive (ADR 0023, third line).
//!
//! When the engine runs as a unit with a watchdog time, systemd expects a
//! message at intervals and restarts the process when they stop. The
//! protocol is one datagram of text to the socket named in NOTIFY_SOCKET:
//! small enough to write here instead of taking a dependency for it.

use std::os::unix::net::UnixDatagram;

/// The connection to systemd, if the engine was started by it with
/// notifications on.
pub struct Notifier {
    socket: Option<(UnixDatagram, String)>,
}

impl Notifier {
    /// Read NOTIFY_SOCKET from the environment.
    pub fn from_environment() -> Self {
        let socket = std::env::var("NOTIFY_SOCKET").ok().and_then(|path| {
            // An address starting with '@' is an abstract socket, which
            // the standard library reaches through another interface:
            // systemd gives a path for units of the system manager.
            if path.starts_with('@') || path.is_empty() {
                return None;
            }
            UnixDatagram::unbound().ok().map(|socket| (socket, path))
        });
        Self { socket }
    }

    /// Whether systemd is listening.
    pub fn active(&self) -> bool {
        self.socket.is_some()
    }

    /// The engine is up.
    pub fn ready(&self) {
        self.send("READY=1");
    }

    /// The engine, and its watchdog, are alive.
    pub fn alive(&self) {
        self.send("WATCHDOG=1");
    }

    fn send(&self, message: &str) {
        if let Some((socket, path)) = &self.socket {
            // Nothing to do if it fails: systemd will notice the silence.
            let _ = socket.send_to(message.as_bytes(), path);
        }
    }
}
