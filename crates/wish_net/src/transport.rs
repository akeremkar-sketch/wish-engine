//! What an endpoint sends and receives datagrams through: a real UDP socket
//! or a node of the network simulator. Both are non-blocking: an endpoint
//! reads what has arrived and returns.

use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};

use wish_core::{Diagnostic, Result};

/// The outcome of one read from a transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recv {
    /// Nothing is waiting.
    Empty,
    /// A datagram of `len` bytes from `from` is at the start of the buffer.
    /// When `len` is larger than the buffer, the datagram was cut and must
    /// be dropped.
    Datagram {
        /// The datagram's size in bytes (may be larger than the buffer).
        len: usize,
        /// The address it came from.
        from: SocketAddr,
    },
    /// A datagram arrived but could not be read (too large for the buffer on
    /// some systems, or a notice that an earlier send was refused). It counts
    /// as one read.
    Dropped,
}

/// A datagram transport. Delivery is not guaranteed: datagrams may be lost,
/// duplicated or reordered, and the endpoint above copes with all three.
pub trait Transport {
    /// The address this transport receives on.
    fn local_addr(&self) -> SocketAddr;

    /// Sends one datagram. An error means the transport could not send at
    /// all (the endpoint counts it and goes on); a datagram lost on the way
    /// is not an error.
    fn send(&mut self, to: SocketAddr, bytes: &[u8]) -> Result<()>;

    /// Reads the next waiting datagram into `buf`, without waiting.
    fn recv(&mut self, buf: &mut [u8]) -> Recv;
}

/// A real, non-blocking UDP socket.
#[derive(Debug)]
pub struct UdpTransport {
    socket: UdpSocket,
    local: SocketAddr,
}

impl UdpTransport {
    /// Binds a socket to `addr` (port 0 picks a free port) and makes it
    /// non-blocking.
    pub fn bind(addr: SocketAddr) -> Result<Self> {
        let socket = UdpSocket::bind(addr).map_err(|error| {
            Diagnostic::error(
                "net_bind_failed",
                format!("could not open UDP port {addr}: {error}"),
                "Another program may be using the port: pick another port, or 0 to let the system choose one.",
            )
        })?;
        socket.set_nonblocking(true).map_err(socket_failed)?;
        let local = socket.local_addr().map_err(socket_failed)?;
        Ok(Self { socket, local })
    }

    /// Allows sending to a broadcast address (local discovery).
    pub fn enable_broadcast(&mut self) -> Result<()> {
        self.socket.set_broadcast(true).map_err(socket_failed)
    }
}

fn socket_failed(error: std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "net_socket_failed",
        format!("the UDP socket failed: {error}"),
        "Check that the network is up and that a firewall allows the program to use UDP.",
    )
}

impl Transport for UdpTransport {
    fn local_addr(&self) -> SocketAddr {
        self.local
    }

    fn send(&mut self, to: SocketAddr, bytes: &[u8]) -> Result<()> {
        match self.socket.send_to(bytes, to) {
            Ok(_) => Ok(()),
            // A full send buffer loses the datagram, as the network may.
            Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(()),
            Err(error) => Err(socket_failed(error)),
        }
    }

    fn recv(&mut self, buf: &mut [u8]) -> Recv {
        match self.socket.recv_from(buf) {
            Ok((len, from)) => Recv::Datagram { len, from },
            Err(error) if error.kind() == ErrorKind::WouldBlock => Recv::Empty,
            // Windows reports a datagram too large for the buffer, and an
            // ICMP "port unreachable" for an earlier send, as errors of the
            // read; both consume one datagram (or notice) and are skipped.
            Err(_) => Recv::Dropped,
        }
    }
}
