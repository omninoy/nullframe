//! WireGuard tunnel implementation using boringtun.

use super::config::{VpnError, VpnStatus, WireGuardConfig};
use super::tunnel::VpnTunnel;
use async_trait::async_trait;
use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use chrono::Utc;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// A WireGuard UDP socket dialed through a SOCKS5 UDP ASSOCIATE relay: the
/// detour a WG→WG chain needs, where the second tunnel's handshake and data
/// ride the first tunnel's worker instead of leaving the machine directly.
///
/// The TCP control connection is held open for the lifetime of the tunnel —
/// the relay tears the association down the moment it closes.
pub(crate) struct SocksUdpTunnel {
  _tcp: std::net::TcpStream,
  pub(crate) udp: UdpSocket,
}

impl SocksUdpTunnel {
  /// Negotiate a UDP ASSOCIATE with `relay` and connect a datagram socket to
  /// the relay endpoint the server hands back.
  pub(crate) fn connect(relay: SocketAddr) -> std::io::Result<Self> {
    let tcp = std::net::TcpStream::connect_timeout(&relay, Duration::from_secs(10))?;
    tcp.set_nodelay(true)?;

    // Greeting: one method, no auth.
    std::io::Write::write_all(&mut &tcp, &[0x05, 0x01, 0x00])?;
    let mut reply = [0u8; 2];
    std::io::Read::read_exact(&mut &tcp, &mut reply)?;
    if reply[0] != 0x05 || reply[1] != 0x00 {
      return Err(std::io::Error::other(format!(
        "SOCKS detour method refused: {reply:02x?}"
      )));
    }

    // UDP ASSOCIATE (CMD=3) with a wildcard destination.
    std::io::Write::write_all(&mut &tcp, &[0x05, 0x03, 0x00, 0x01, 0, 0, 0, 0, 0, 0])?;
    let mut assoc = [0u8; 10];
    std::io::Read::read_exact(&mut &tcp, &mut assoc)?;
    if assoc[0] != 0x05 || assoc[1] != 0x00 || assoc[3] != 0x01 {
      return Err(std::io::Error::other(format!(
        "SOCKS detour associate refused: {assoc:02x?}"
      )));
    }
    let bound = SocketAddr::from(([127, 0, 0, 1], u16::from_be_bytes([assoc[8], assoc[9]])));

    let udp = UdpSocket::bind("0.0.0.0:0")?;
    udp.connect(bound)?;
    udp.set_nonblocking(false).ok();

    Ok(Self { _tcp: tcp, udp })
  }

  /// RFC 1928 §7 datagram: RSV(2) FRAG(1) ATYP(1) ADDR PORT payload.
  pub(crate) fn send_to(&self, data: &[u8], dst: SocketAddr) -> std::io::Result<()> {
    let mut pkt = Vec::with_capacity(data.len() + 26);
    pkt.extend_from_slice(&[0x00, 0x00, 0x00]);
    match dst.ip() {
      std::net::IpAddr::V4(v4) => {
        pkt.push(0x01);
        pkt.extend_from_slice(&v4.octets());
      }
      std::net::IpAddr::V6(v6) => {
        pkt.push(0x04);
        pkt.extend_from_slice(&v6.octets());
      }
    }
    pkt.extend_from_slice(&dst.port().to_be_bytes());
    pkt.extend_from_slice(data);
    // A WG datagram past the relay MTU would silently vanish; fail loudly.
    if pkt.len() > 65507 {
      return Err(std::io::Error::other("Datagram too large for SOCKS relay"));
    }
    self.udp.send(&pkt).map(|_| ())
  }

  pub(crate) fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
    let mut pkt = vec![0u8; 65535];
    let len = self.udp.recv(&mut pkt)?;
    if len < 5 {
      return Err(std::io::Error::other("SOCKS detour datagram truncated"));
    }
    if pkt[2] != 0x00 {
      return Err(std::io::Error::other(
        "SOCKS detour fragmented datagram dropped",
      ));
    }
    let (addr, header_len): (SocketAddr, usize) = match pkt[3] {
      0x01 => {
        if len < 10 {
          return Err(std::io::Error::other("SOCKS detour header truncated"));
        }
        (
          SocketAddr::from((
            [pkt[4], pkt[5], pkt[6], pkt[7]],
            u16::from_be_bytes([pkt[8], pkt[9]]),
          )),
          10,
        )
      }
      0x04 => {
        if len < 22 {
          return Err(std::io::Error::other("SOCKS detour header truncated"));
        }
        let mut octets = [0u8; 16];
        octets.copy_from_slice(&pkt[4..20]);
        (
          SocketAddr::from((
            std::net::Ipv6Addr::from(octets),
            u16::from_be_bytes([pkt[20], pkt[21]]),
          )),
          22,
        )
      }
      other => {
        return Err(std::io::Error::other(format!(
          "SOCKS detour unsupported address type {other}"
        )));
      }
    };
    if header_len > len {
      return Err(std::io::Error::other("SOCKS detour header truncated"));
    }
    let payload = &pkt[header_len..len];
    let n = payload.len().min(buf.len());
    buf[..n].copy_from_slice(&payload[..n]);
    Ok((n, addr))
  }
}

/// The UDP uplink a WireGuard tunnel sends its encrypted packets through:
/// straight out of the machine, or wrapped in a SOCKS5 UDP detour.
enum WgUplink {
  Direct(Arc<UdpSocket>),
  Socks(Arc<SocksUdpTunnel>),
}

impl WgUplink {
  fn send_to(&self, data: &[u8], peer: SocketAddr) -> Result<(), VpnError> {
    match self {
      Self::Direct(socket) => socket
        .send_to(data, peer)
        .map(|_| ())
        .map_err(|e| VpnError::Connection(format!("Failed to send data: {e}"))),
      Self::Socks(tunnel) => tunnel
        .send_to(data, peer)
        .map_err(|e| VpnError::Connection(format!("SOCKS detour send failed: {e}"))),
    }
  }

  fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr), VpnError> {
    match self {
      Self::Direct(socket) => socket
        .recv_from(buf)
        .map_err(|e| VpnError::Connection(format!("Handshake timeout or error: {e}"))),
      Self::Socks(tunnel) => tunnel
        .recv_from(buf)
        .map_err(|e| VpnError::Connection(format!("SOCKS detour recv failed: {e}"))),
    }
  }

  fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), VpnError> {
    match self {
      Self::Direct(socket) => socket
        .set_read_timeout(timeout)
        .map_err(|e| VpnError::Connection(format!("Failed to set timeout: {e}"))),
      Self::Socks(tunnel) => tunnel
        .udp
        .set_read_timeout(timeout)
        .map_err(|e| VpnError::Connection(format!("Failed to set timeout: {e}"))),
    }
  }
}

/// WireGuard tunnel implementation
pub struct WireGuardTunnel {
  vpn_id: String,
  config: WireGuardConfig,
  tunnel: Option<Arc<Mutex<Box<Tunn>>>>,
  uplink: Option<WgUplink>,
  connected: AtomicBool,
  connected_at: Option<i64>,
  bytes_sent: AtomicU64,
  bytes_received: AtomicU64,
  last_handshake: Option<i64>,
  peer_addr: Option<SocketAddr>,
}

impl WireGuardTunnel {
  /// Create a new WireGuard tunnel
  pub fn new(vpn_id: String, config: WireGuardConfig) -> Self {
    Self {
      vpn_id,
      config,
      tunnel: None,
      uplink: None,
      connected: AtomicBool::new(false),
      connected_at: None,
      bytes_sent: AtomicU64::new(0),
      bytes_received: AtomicU64::new(0),
      last_handshake: None,
      peer_addr: None,
    }
  }

  /// Parse base64 key to bytes
  fn parse_key(key: &str) -> Result<[u8; 32], VpnError> {
    let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, key)
      .map_err(|e| VpnError::InvalidWireGuard(format!("Invalid key encoding: {e}")))?;

    if decoded.len() != 32 {
      return Err(VpnError::InvalidWireGuard(format!(
        "Invalid key length: {} (expected 32)",
        decoded.len()
      )));
    }

    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&decoded);
    Ok(key_bytes)
  }

  /// Initialize the WireGuard tunnel
  fn init_tunnel(&mut self) -> Result<(), VpnError> {
    // Parse private key
    let private_key_bytes = Self::parse_key(&self.config.private_key)?;
    let static_private = StaticSecret::from(private_key_bytes);

    // Parse peer public key
    let peer_public_bytes = Self::parse_key(&self.config.peer_public_key)?;
    let peer_public = PublicKey::from(peer_public_bytes);

    // Parse optional preshared key
    let preshared_key = if let Some(ref psk) = self.config.preshared_key {
      Some(Self::parse_key(psk)?)
    } else {
      None
    };

    // Create the boringtun tunnel
    let tunn = Tunn::new(
      static_private,
      peer_public,
      preshared_key,
      self.config.persistent_keepalive,
      0, // index
      None,
    );

    self.tunnel = Some(Arc::new(Mutex::new(Box::new(tunn))));
    Ok(())
  }

  /// Resolve peer endpoint to socket address
  fn resolve_endpoint(&mut self) -> Result<SocketAddr, VpnError> {
    let endpoint = &self.config.peer_endpoint;

    // Try to resolve the endpoint
    let addrs: Vec<SocketAddr> = endpoint
      .to_socket_addrs()
      .map_err(|e| VpnError::Connection(format!("Failed to resolve endpoint '{endpoint}': {e}")))?
      .collect();

    addrs
      .into_iter()
      .next()
      .ok_or_else(|| VpnError::Connection(format!("No addresses found for endpoint: {endpoint}")))
  }

  /// Perform WireGuard handshake
  async fn handshake(&mut self) -> Result<(), VpnError> {
    let tunnel = self
      .tunnel
      .as_ref()
      .ok_or_else(|| VpnError::Tunnel("Tunnel not initialized".to_string()))?;

    let uplink = self
      .uplink
      .as_ref()
      .ok_or_else(|| VpnError::Tunnel("Uplink not initialized".to_string()))?;

    let peer_addr = self
      .peer_addr
      .ok_or_else(|| VpnError::Tunnel("Peer address not resolved".to_string()))?;

    let mut tunnel_guard = tunnel.lock().await;
    let awg = &self.config.awg;

    // An AmneziaWG peer expects its junk datagrams before the initiation.
    for junk in super::awg::junk_packets(awg) {
      uplink.send_to(&junk, peer_addr)?;
    }

    // Generate handshake initiation
    let mut dst = vec![0u8; 2048];
    let result = tunnel_guard.format_handshake_initiation(&mut dst, false);

    match result {
      TunnResult::WriteToNetwork(packet) => {
        let mut wrapped = packet.to_vec();
        super::awg::wrap_outgoing(awg, super::awg::AwgMessage::Initiation, &mut wrapped);
        let sent = wrapped.len() as u64;
        uplink.send_to(&wrapped, peer_addr)?;

        self.bytes_sent.fetch_add(sent, Ordering::Relaxed);
      }
      TunnResult::Err(e) => {
        return Err(VpnError::Tunnel(format!(
          "Handshake initiation failed: {e:?}"
        )));
      }
      _ => {}
    }

    // Wait for handshake response, dropping the peer's junk datagrams
    // (an AmneziaWG peer prefixes its response with `Jc` of them).
    uplink.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;

    let mut recv_buf = vec![0u8; 2048];
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);

    loop {
      let now = std::time::Instant::now();
      if now >= deadline {
        return Err(VpnError::Connection(
          "Handshake timeout: response deadline passed".to_string(),
        ));
      }
      uplink.set_read_timeout(Some(deadline - now))?;

      match uplink.recv_from(&mut recv_buf) {
        Ok((len, _from)) => {
          let kind = match super::awg::unwrap_incoming(awg, &mut recv_buf[..len]) {
            super::awg::AwgIncoming::Ignore => continue,
            super::awg::AwgIncoming::Message { kind, len: msg_len } => (kind, msg_len),
          };
          if kind.0 != super::awg::AwgMessage::Response {
            continue;
          }
          self
            .bytes_received
            .fetch_add(kind.1 as u64, Ordering::Relaxed);

          let result = tunnel_guard.decapsulate(None, &recv_buf[..kind.1], &mut dst);

          match result {
            TunnResult::WriteToNetwork(response) => {
              let mut wrapped = response.to_vec();
              super::awg::wrap_outgoing(awg, super::awg::AwgMessage::Response, &mut wrapped);
              for junk in super::awg::junk_packets(awg) {
                let _ = uplink.send_to(&junk, peer_addr);
              }
              let sent = wrapped.len() as u64;
              uplink.send_to(&wrapped, peer_addr)?;

              self.bytes_sent.fetch_add(sent, Ordering::Relaxed);
              self.last_handshake = Some(Utc::now().timestamp());
            }
            TunnResult::Done => {
              self.last_handshake = Some(Utc::now().timestamp());
            }
            TunnResult::Err(e) => {
              return Err(VpnError::Tunnel(format!(
                "Handshake response failed: {e:?}"
              )));
            }
            _ => {}
          }

          return Ok(());
        }
        Err(e) => {
          return Err(VpnError::Connection(format!(
            "Handshake timeout or error: {e}"
          )));
        }
      }
    }
  }

  /// Encrypt and send data through the tunnel
  pub async fn send(&self, data: &[u8]) -> Result<(), VpnError> {
    let tunnel = self
      .tunnel
      .as_ref()
      .ok_or_else(|| VpnError::Tunnel("Tunnel not initialized".to_string()))?;

    let uplink = self
      .uplink
      .as_ref()
      .ok_or_else(|| VpnError::Tunnel("Uplink not initialized".to_string()))?;

    let peer_addr = self
      .peer_addr
      .ok_or_else(|| VpnError::Tunnel("Peer address not resolved".to_string()))?;

    let mut tunnel_guard = tunnel.lock().await;
    let mut dst = vec![0u8; data.len() + 256]; // Extra space for WireGuard overhead

    let result = tunnel_guard.encapsulate(data, &mut dst);

    match result {
      TunnResult::WriteToNetwork(packet) => {
        let mut wrapped = packet.to_vec();
        super::awg::wrap_outgoing(
          &self.config.awg,
          super::awg::AwgMessage::Transport,
          &mut wrapped,
        );
        let sent = wrapped.len() as u64;
        uplink.send_to(&wrapped, peer_addr)?;

        self.bytes_sent.fetch_add(sent, Ordering::Relaxed);
      }
      TunnResult::Err(e) => {
        return Err(VpnError::Tunnel(format!("Encryption failed: {e:?}")));
      }
      _ => {}
    }

    Ok(())
  }

  /// Receive and decrypt data from the tunnel
  pub async fn receive(&self, buf: &mut [u8]) -> Result<usize, VpnError> {
    let tunnel = self
      .tunnel
      .as_ref()
      .ok_or_else(|| VpnError::Tunnel("Tunnel not initialized".to_string()))?;

    let uplink = self
      .uplink
      .as_ref()
      .ok_or_else(|| VpnError::Tunnel("Uplink not initialized".to_string()))?;

    let mut recv_buf = vec![0u8; 2048];

    // Skip the peer's junk datagrams (AmneziaWG prefixes handshake messages
    // with them); transport data itself never carries a junk prefix.
    let msg_len = loop {
      let (len, _from) = uplink.recv_from(&mut recv_buf)?;
      match super::awg::unwrap_incoming(&self.config.awg, &mut recv_buf[..len]) {
        super::awg::AwgIncoming::Ignore => continue,
        super::awg::AwgIncoming::Message { len: msg_len, .. } => break msg_len,
      }
    };

    self
      .bytes_received
      .fetch_add(msg_len as u64, Ordering::Relaxed);

    let mut tunnel_guard = tunnel.lock().await;
    // decapsulate writes decrypted data directly to buf and returns a slice pointing to it
    let result = tunnel_guard.decapsulate(None, &recv_buf[..msg_len], buf);

    match result {
      // Data is already written to buf by decapsulate, just return the length
      TunnResult::WriteToTunnelV4(decrypted, _) => Ok(decrypted.len()),
      TunnResult::WriteToTunnelV6(decrypted, _) => Ok(decrypted.len()),
      TunnResult::Done => Ok(0),
      TunnResult::Err(e) => Err(VpnError::Tunnel(format!("Decryption failed: {e:?}"))),
      _ => Ok(0),
    }
  }
}

#[async_trait]
impl VpnTunnel for WireGuardTunnel {
  async fn connect(&mut self) -> Result<(), VpnError> {
    if self.connected.load(Ordering::Relaxed) {
      return Ok(());
    }

    // Initialize the tunnel
    self.init_tunnel()?;

    // Resolve endpoint
    self.peer_addr = Some(self.resolve_endpoint()?);

    // The UDP uplink: straight out of the machine, or through a SOCKS5 UDP
    // detour when this tunnel is the second hop of a WG→WG chain.
    let uplink = if let Some(detour) = self.config.dial_via_socks.clone() {
      let relay = detour
        .to_socket_addrs()
        .map_err(|e| VpnError::Connection(format!("Bad SOCKS detour '{detour}': {e}")))?
        .next()
        .ok_or_else(|| VpnError::Connection(format!("SOCKS detour '{detour}' did not resolve")))?;
      log::info!(
        "WireGuard tunnel dialing endpoint via SOCKS detour vpn={} detour={relay} endpoint={}",
        self.vpn_id,
        self.peer_addr.as_ref().expect("resolved above")
      );
      let tunnel = SocksUdpTunnel::connect(relay)
        .map_err(|e| VpnError::Connection(format!("SOCKS detour associate failed: {e}")))?;
      WgUplink::Socks(Arc::new(tunnel))
    } else {
      let socket = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| VpnError::Connection(format!("Failed to create socket: {e}")))?;
      socket
        .set_nonblocking(false)
        .map_err(|e| VpnError::Connection(format!("Failed to set socket options: {e}")))?;
      WgUplink::Direct(Arc::new(socket))
    };

    uplink.set_read_timeout(None)?;

    self.uplink = Some(uplink);

    // Perform handshake
    self.handshake().await?;

    self.connected.store(true, Ordering::Release);
    self.connected_at = Some(Utc::now().timestamp());

    log::info!("WireGuard tunnel connected vpn={}", self.vpn_id);

    Ok(())
  }

  async fn disconnect(&mut self) -> Result<(), VpnError> {
    if !self.connected.load(Ordering::Relaxed) {
      return Ok(());
    }

    self.connected.store(false, Ordering::Release);
    self.tunnel = None;
    self.uplink = None;
    self.connected_at = None;

    log::info!("WireGuard tunnel disconnected vpn={}", self.vpn_id);

    Ok(())
  }

  fn is_connected(&self) -> bool {
    self.connected.load(Ordering::Acquire)
  }

  fn vpn_id(&self) -> &str {
    &self.vpn_id
  }

  fn get_status(&self) -> VpnStatus {
    VpnStatus {
      connected: self.is_connected(),
      vpn_id: self.vpn_id.clone(),
      connected_at: self.connected_at,
      bytes_sent: Some(self.bytes_sent.load(Ordering::Relaxed)),
      bytes_received: Some(self.bytes_received.load(Ordering::Relaxed)),
      last_handshake: self.last_handshake,
    }
  }

  fn bytes_sent(&self) -> u64 {
    self.bytes_sent.load(Ordering::Relaxed)
  }

  fn bytes_received(&self) -> u64 {
    self.bytes_received.load(Ordering::Relaxed)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  use super::super::awg::AwgParams;

  fn create_test_config() -> WireGuardConfig {
    WireGuardConfig {
      private_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
      address: "10.0.0.2/24".to_string(),
      dns: Some("1.1.1.1".to_string()),
      mtu: Some(1420),
      peer_public_key: "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=".to_string(),
      peer_endpoint: "127.0.0.1:51820".to_string(),
      allowed_ips: vec!["0.0.0.0/0".to_string()],
      persistent_keepalive: Some(25),
      preshared_key: None,
      awg: AwgParams::default(),
      cps_i1: None,
      dial_via_socks: None,
    }
  }

  #[test]
  fn test_wireguard_tunnel_creation() {
    let config = create_test_config();
    let tunnel = WireGuardTunnel::new("test-wg-1".to_string(), config);

    assert_eq!(tunnel.vpn_id(), "test-wg-1");
    assert!(!tunnel.is_connected());
    assert_eq!(tunnel.bytes_sent(), 0);
    assert_eq!(tunnel.bytes_received(), 0);
  }

  #[test]
  fn test_parse_key_valid() {
    let key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    let result = WireGuardTunnel::parse_key(key);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 32);
  }

  #[test]
  fn test_parse_key_invalid_base64() {
    let key = "not-valid-base64!!!";
    let result = WireGuardTunnel::parse_key(key);
    assert!(result.is_err());
  }

  #[test]
  fn test_parse_key_wrong_length() {
    // Valid base64 but wrong length
    let key = "YWJjZA=="; // "abcd" in base64
    let result = WireGuardTunnel::parse_key(key);
    assert!(result.is_err());
  }

  #[test]
  fn test_wireguard_status() {
    let config = create_test_config();
    let tunnel = WireGuardTunnel::new("test-wg-2".to_string(), config);

    let status = tunnel.get_status();
    assert!(!status.connected);
    assert_eq!(status.vpn_id, "test-wg-2");
    assert!(status.connected_at.is_none());
    assert_eq!(status.bytes_sent, Some(0));
    assert_eq!(status.bytes_received, Some(0));
  }
}
