//! Session transport for live collaboration: a host and guests over iroh QUIC.
//!
//! Star topology: every guest holds one connection to the host. The crate is document-agnostic and
//! carries opaque byte messages. Two lanes per connection:
//! * reliable, ordered frames (one bidirectional stream, see [`frame`]) for edits and snapshots;
//! * unreliable QUIC datagrams for presence (cursors), which never queue behind a snapshot.
//!
//! The API is synchronous for the egui UI thread: a tokio runtime lives inside [`Host`] / [`Guest`],
//! commands are non-blocking, and events come out of `try_recv` / `drain`. Set a wake callback
//! (`ctx.request_repaint()`) to be nudged when events arrive.
mod auth;
pub mod frame;
mod guest;
mod host;
mod proto;
mod rt;
mod ticket;

pub use guest::{DisconnectReason, Guest, GuestEvent, GuestOptions};
pub use host::{generate_secret_key, Host, HostEvent, HostOptions};
pub use proto::PROTOCOL_VERSION;
pub use ticket::Ticket;

/// ALPN of the collaboration protocol.
pub const ALPN: &[u8] = b"desigma/collab/1";

/// How an endpoint reaches the network.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Network {
    /// Default n0 relays and address discovery (hole punching with relay fallback).
    #[default]
    Internet,
    /// No relay, no discovery: direct addresses only (LAN).
    Direct,
    /// Like [`Network::Direct`], bound to 127.0.0.1 only: tests and same-machine sessions,
    /// which then never trigger a firewall prompt.
    Loopback,
}

/// Peer index: 0 is the host, guests get 1, 2, ... and are never reused within a session.
pub type PeerId = u32;

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("cannot start networking: {0}")]
    Bind(String),
    #[error("cannot reach the host: {0}")]
    Connect(String),
    #[error("connection error: {0}")]
    Io(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("frame of {0} bytes is too large")]
    TooLarge(usize),
    #[error("no such peer")]
    UnknownPeer,
    #[error("not connected")]
    Closed,
    #[error("peer is too slow, message dropped")]
    Backlog,
    #[error("invalid ticket: {0}")]
    Ticket(String),
}

/// Why joining failed. Both sides surface it: the guest as the result, the host as
/// [`HostEvent::JoinRefused`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, serde::Serialize, serde::Deserialize)]
pub enum JoinError {
    #[error("protocol version mismatch (host {host}, you {guest}): update desigma")]
    VersionMismatch { host: u32, guest: u32 },
    #[error("wrong password")]
    WrongPassword,
    #[error("too many wrong passwords, try again in {retry_secs} s")]
    Locked { retry_secs: u64 },
    #[error("the host declined your request")]
    Denied,
    #[error("the session is full")]
    Full,
    #[error("cannot reach the host: {0}")]
    Connect(String),
    #[error("the host did not answer in time")]
    Timeout,
    #[error("protocol error: {0}")]
    Protocol(String),
}
