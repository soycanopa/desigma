//! The transport the session code talks to. Small traits so tests can wire two editors through
//! channels; `NetHost` / `NetGuest` carry the real thing (desigma-net over iroh).

use desigma_net::{DisconnectReason, Guest, GuestEvent, GuestOptions, Host, HostEvent, HostOptions, JoinError, Network, Ticket};

/// What a host transport reports. Peer numbers are the transport's own.
#[derive(Debug)]
pub enum HostEv {
    /// Listening; guests can join with this link.
    Ready {
        link: String,
    },
    Failed(String),
    /// A guest waits for [`HostLink::respond`] (approval is on).
    Request {
        id: u64,
        name: String,
    },
    Joined {
        peer: u32,
        name: String,
    },
    Left {
        peer: u32,
    },
    Msg {
        peer: u32,
        bytes: Vec<u8>,
    },
    Presence {
        peer: u32,
        bytes: Vec<u8>,
    },
    Refused {
        name: String,
        error: JoinError,
    },
}

pub trait HostLink {
    fn drain(&self) -> Vec<HostEv>;
    fn respond(&self, id: u64, accept: bool);
    fn send(&self, peer: u32, bytes: Vec<u8>);
    fn send_presence(&self, peer: u32, bytes: &[u8]);
    fn kick(&self, peer: u32);
    /// The link to share right now (addresses can change while sharing).
    fn link(&self) -> Option<String> {
        None
    }
    /// The resume secret `peer` showed when it connected, if any.
    fn join_token(&self, _peer: u32) -> Option<String> {
        None
    }
    /// A guest holding this secret comes back without being asked about again.
    fn allow_resume(&self, _token: &str) {}
    fn revoke_resume(&self, _token: &str) {}
}

impl desigma_collab::wire::HostWire for dyn HostLink {
    fn send(&self, net: u32, bytes: Vec<u8>) {
        HostLink::send(self, net, bytes);
    }
    fn send_presence(&self, net: u32, bytes: &[u8]) {
        HostLink::send_presence(self, net, bytes);
    }
    fn kick(&self, net: u32) {
        HostLink::kick(self, net);
    }
    fn join_token(&self, net: u32) -> Option<String> {
        HostLink::join_token(self, net)
    }
    fn allow_resume(&self, token: &str) {
        HostLink::allow_resume(self, token);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Closed {
    Kicked,
    HostEnded,
    Left,
    Lost(String),
}

#[derive(Debug)]
pub enum GuestEv {
    Joined,
    Failed(JoinError),
    Msg(Vec<u8>),
    Presence(Vec<u8>),
    Closed(Closed),
}

/// Dials the host again for a guest whose connection dropped, showing the resume secret. Each
/// call is one attempt.
pub type Redial = Box<dyn Fn(Option<&str>) -> Result<Box<dyn GuestLink>, String>>;

pub trait GuestLink {
    fn drain(&self) -> Vec<GuestEv>;
    fn send(&self, bytes: Vec<u8>);
    fn send_presence(&self, bytes: &[u8]);
    fn leave(&self);
}

/// `DESIGMA_COLLAB_NETWORK=direct` skips relays and discovery (LAN); `loopback` also binds
/// 127.0.0.1 only (tests, same-machine sessions).
pub fn network() -> Network {
    match std::env::var("DESIGMA_COLLAB_NETWORK") {
        Ok(v) if v.eq_ignore_ascii_case("direct") => Network::Direct,
        Ok(v) if v.eq_ignore_ascii_case("loopback") => Network::Loopback,
        _ => Network::Internet,
    }
}

pub struct NetHost(Option<Host>);

impl NetHost {
    pub fn start(password: Option<String>, require_approval: bool, wake: Box<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        let host = Host::start(HostOptions { password, require_approval, network: network(), ..Default::default() }).map_err(|e| e.to_string())?;
        host.set_wake(wake);
        Ok(Self(Some(host)))
    }
    fn host(&self) -> &Host {
        self.0.as_ref().expect("host lives until dropped")
    }
    /// Cuts a guest's connection like a network failure would (tests).
    #[cfg(test)]
    pub fn sever(&self, peer: u32) {
        self.host().sever(peer);
    }
}

impl HostLink for NetHost {
    fn drain(&self) -> Vec<HostEv> {
        self.host()
            .drain()
            .into_iter()
            .map(|e| match e {
                HostEvent::Ready(t) => HostEv::Ready { link: t.to_link() },
                HostEvent::Failed(s) => HostEv::Failed(s),
                HostEvent::JoinRequest { request, name, .. } => HostEv::Request { id: request, name },
                HostEvent::PeerJoined { peer, name } => HostEv::Joined { peer, name },
                HostEvent::PeerLeft { peer } => HostEv::Left { peer },
                HostEvent::Message { peer, bytes } => HostEv::Msg { peer, bytes },
                HostEvent::Presence { peer, bytes } => HostEv::Presence { peer, bytes },
                HostEvent::JoinRefused { name, error, .. } => HostEv::Refused { name, error },
            })
            .collect()
    }
    fn respond(&self, id: u64, accept: bool) {
        self.host().respond(id, accept);
    }
    fn send(&self, peer: u32, bytes: Vec<u8>) {
        let _ = self.host().send(peer, bytes);
    }
    fn send_presence(&self, peer: u32, bytes: &[u8]) {
        let _ = self.host().send_presence(peer, bytes);
    }
    fn kick(&self, peer: u32) {
        self.host().kick(peer);
    }
    fn link(&self) -> Option<String> {
        self.host().ticket().map(|t| t.to_link())
    }
    fn join_token(&self, peer: u32) -> Option<String> {
        self.host().join_token(peer)
    }
    fn allow_resume(&self, token: &str) {
        self.host().allow_resume(token);
    }
    fn revoke_resume(&self, token: &str) {
        self.host().revoke_resume(token);
    }
}

impl Drop for NetHost {
    fn drop(&mut self) {
        // Dropping a Host waits (bounded) for the close frames; do it off the UI thread.
        if let Some(h) = self.0.take() {
            h.shutdown();
            std::thread::spawn(move || drop(h));
        }
    }
}

pub struct NetGuest(Option<Guest>);

impl NetGuest {
    pub fn join(ticket: &Ticket, name: &str, password: Option<&str>, wake: Box<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        Self::join_resume(ticket, name, password, wake, None)
    }

    /// [`Self::join`] showing the secret of an earlier visit.
    pub fn join_resume(ticket: &Ticket, name: &str, password: Option<&str>, wake: Box<dyn Fn() + Send + Sync>, resume: Option<String>) -> Result<Self, String> {
        let g = Guest::join_resume(ticket, name, password, GuestOptions { network: network(), ..Default::default() }, resume).map_err(|e| e.to_string())?;
        g.set_wake(wake);
        Ok(Self(Some(g)))
    }
    fn guest(&self) -> &Guest {
        self.0.as_ref().expect("guest lives until dropped")
    }
}

impl GuestLink for NetGuest {
    fn drain(&self) -> Vec<GuestEv> {
        self.guest()
            .drain()
            .into_iter()
            .map(|e| match e {
                GuestEvent::Joined { .. } => GuestEv::Joined,
                GuestEvent::JoinFailed(e) => GuestEv::Failed(e),
                GuestEvent::Message(b) => GuestEv::Msg(b),
                GuestEvent::Presence(b) => GuestEv::Presence(b),
                GuestEvent::Disconnected(r) => GuestEv::Closed(match r {
                    DisconnectReason::Kicked => Closed::Kicked,
                    DisconnectReason::HostEnded => Closed::HostEnded,
                    DisconnectReason::Left => Closed::Left,
                    DisconnectReason::Lost(s) => Closed::Lost(s),
                }),
            })
            .collect()
    }
    fn send(&self, bytes: Vec<u8>) {
        let _ = self.guest().send(bytes);
    }
    fn send_presence(&self, bytes: &[u8]) {
        let _ = self.guest().send_presence(bytes);
    }
    fn leave(&self) {
        self.guest().leave();
    }
}

impl Drop for NetGuest {
    fn drop(&mut self) {
        if let Some(g) = self.0.take() {
            g.leave();
            std::thread::spawn(move || drop(g));
        }
    }
}

/// Clear, plain-language text for every way joining can fail.
pub fn join_error_text(e: &JoinError) -> String {
    match e {
        JoinError::VersionMismatch { host, guest } if host < guest => format!(
            "The host runs an older desigma that cannot share with yours (sharing protocol {host}, yours {guest}). Ask them to update to the latest release and share the file again."
        ),
        JoinError::VersionMismatch { host, guest } => format!(
            "The host runs a newer desigma than yours (sharing protocol {host}, yours {guest}). Update desigma to the latest release and try again."
        ),
        JoinError::WrongPassword => "Wrong password. Check it with the person who shared the link.".into(),
        JoinError::Locked { retry_secs } => format!("Too many wrong passwords. Try again in {retry_secs} seconds."),
        JoinError::Denied => "The host declined your request to join.".into(),
        JoinError::Full => "The session is full. Ask the host to make room.".into(),
        JoinError::Connect(s) => {
            format!("Cannot reach the host. Check the link and your internet connection, and that the host still has the file shared. ({s})")
        }
        JoinError::Timeout => "The host did not answer in time. Try again.".into(),
        JoinError::Protocol(s) => format!("The host sent something unexpected ({s}). Make sure you both run the same version of desigma."),
    }
}
