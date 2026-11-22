use super::config::{VpnError, WireGuardConfig};
use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use smoltcp::iface::{Config as IfaceConfig, Interface, PollResult, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::dns;
use smoltcp::socket::tcp::{Socket as TcpSocket, SocketBuffer};
use smoltcp::time::Instant as SmolInstant;
use smoltcp::wire::{HardwareAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Address};
use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::{Arc, Mutex};
use tokio::net::{TcpListener, TcpStream};

const SMOLTCP_TCP_RX_BUF: usize = 65536;
const SMOLTCP_TCP_TX_BUF: usize = 65536;
const SMOLTCP_UDP_BUF: usize = 65536;

/// How often boringtun's timers run: handshake renewal, keepalives.
const WG_TIMER_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// How long the relay loop waits before its next pass. A pass that moved
/// nothing doubles the wait, from 1 ms up to 8 ms, so an idle tunnel wakes
/// about 125 times a second instead of about 1000; any activity drops it back
/// to 1 ms. A new SOCKS5 connection ends the wait at once. Packets from the
/// peer and from the browser do not end it, so the cap is also the most an
/// idle tunnel adds to the first packet after a pause.
#[derive(Debug)]
struct IdleBackoff {
  wait_ms: u64,
}

impl IdleBackoff {
  const MIN_MS: u64 = 1;
  const MAX_MS: u64 = 8;

  fn new() -> Self {
    Self {
      wait_ms: Self::MIN_MS,
    }
  }

  /// The wait after a pass. `active` is whether the pass moved bytes,
  /// packets or connection state; `smoltcp_due` is when smoltcp next needs a
  /// poll (a retransmit, a DNS retry), which the wait never overshoots.
  fn next_wait(
    &mut self,
    active: bool,
    smoltcp_due: Option<std::time::Duration>,
  ) -> std::time::Duration {
    self.wait_ms = if active {
      Self::MIN_MS
    } else {
      (self.wait_ms * 2).min(Self::MAX_MS)
    };
    let wait = std::time::Duration::from_millis(self.wait_ms);
    smoltcp_due
      .map_or(wait, |due| wait.min(due))
      .max(std::time::Duration::from_millis(Self::MIN_MS))
  }
}

/// Parse an RFC 1928 §7 UDP request header. Returns the destination endpoint
/// and the payload offset, or None if malformed, fragmented, or domain-typed.
/// Only literal IPs are routed through the tunnel: resolving a domain on the
/// host would leak DNS, and QUIC/WebRTC datagrams always carry literal IPs.
fn parse_udp_datagram(buf: &[u8]) -> Option<(IpEndpoint, usize)> {
  if buf.len() < 4 || buf[2] != 0 {
    // too short, or FRAG != 0 (fragmentation unsupported)
    return None;
  }
  match buf[3] {
    0x01 => {
      if buf.len() < 10 {
        return None;
      }
      let ip = Ipv4Address::new(buf[4], buf[5], buf[6], buf[7]);
      let port = u16::from_be_bytes([buf[8], buf[9]]);
      Some((IpEndpoint::new(IpAddress::Ipv4(ip), port), 10))
    }
    0x04 => {
      if buf.len() < 22 {
        return None;
      }
      let mut o = [0u8; 16];
      o.copy_from_slice(&buf[4..20]);
      let ip = smoltcp::wire::Ipv6Address::from(o);
      let port = u16::from_be_bytes([buf[20], buf[21]]);
      Some((IpEndpoint::new(IpAddress::Ipv6(ip), port), 22))
    }
    _ => None,
  }
}

/// Wrap a tunnel-received datagram in an RFC 1928 §7 UDP reply header naming
/// `src` as the origin, for delivery back to the browser's relay socket.
fn build_udp_datagram(src: IpEndpoint, payload: &[u8]) -> Vec<u8> {
  let mut out = vec![0x00, 0x00, 0x00]; // RSV(2) + FRAG(0)
  match src.addr {
    IpAddress::Ipv4(v4) => {
      out.push(0x01);
      out.extend_from_slice(&v4.octets());
    }
    IpAddress::Ipv6(v6) => {
      out.push(0x04);
      out.extend_from_slice(&v6.octets());
    }
  }
  out.extend_from_slice(&src.port.to_be_bytes());
  out.extend_from_slice(payload);
  out
}

/// The WG tunnel's datagram uplink: straight UDP, or a SOCKS5 UDP detour for
/// WG→WG chains (hop 2's endpoint rides hop 1's worker).
enum WgSocket {
  Direct(Arc<UdpSocket>),
  Detour(Arc<super::wireguard::SocksUdpTunnel>),
}

impl WgSocket {
  fn send_to(&self, data: &[u8], peer: SocketAddr) -> std::io::Result<usize> {
    match self {
      Self::Direct(socket) => socket.send_to(data, peer),
      Self::Detour(tunnel) => {
        tunnel.send_to(data, peer)?;
        Ok(data.len())
      }
    }
  }

  fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
    match self {
      Self::Direct(socket) => socket.recv_from(buf),
      Self::Detour(tunnel) => tunnel.recv_from(buf),
    }
  }

  fn set_read_timeout(&self, timeout: Option<std::time::Duration>) -> std::io::Result<()> {
    match self {
      Self::Direct(socket) => socket.set_read_timeout(timeout),
      Self::Detour(tunnel) => tunnel.udp.set_read_timeout(timeout),
    }
  }

  fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()> {
    match self {
      Self::Direct(socket) => socket.set_nonblocking(nonblocking),
      Self::Detour(tunnel) => tunnel.udp.set_nonblocking(nonblocking),
    }
  }
}

/// A live SOCKS5 UDP ASSOCIATE served with the direct inner-IP bypass: the
/// loopback relay socket the client sends datagrams to, the client's learned
/// source address, and the inner (tunnel-side) UDP source port the datagrams
/// leave with. Bypassed traffic never touches the smoltcp stack, so the
/// tunnel's MTU does not cap it — a detoured exit's own WireGuard packets
/// (up to its server's MTU) must pass whole, and an inner ICMP smaller than
/// the bypass cap must reach the exit's stack too.
struct UdpAssoc {
  relay: UdpSocket,
  client_addr: Option<SocketAddr>,
}

struct WgDevice {
  tunn: Arc<Mutex<Box<Tunn>>>,
  udp_socket: Arc<WgSocket>,
  peer_addr: SocketAddr,
  awg: super::awg::AwgParams,
  cps_i1: Option<Vec<u8>>,
  /// The inner IP packet budget. The config's MTU governs it (a CDN-fronted
  /// or nested tunnel ships a smaller one than WireGuard's 1420 default); a
  /// plain config without an MTU line keeps the old 1420. Detour datagrams
  /// bypass this entirely (see `route_inner`).
  mtu: usize,
  /// SOCKS5 UDP ASSOCIATE sessions served with the inner-IP bypass, keyed by the
  /// inner source port the association's datagrams leave with.
  detour_relays: Arc<Mutex<HashMap<u16, Arc<Mutex<UdpAssoc>>>>>,
  rx_queue: VecDeque<Vec<u8>>,
  tx_queue: VecDeque<Vec<u8>>,
}

impl WgDevice {
  /// Send one boringtun-produced packet: handshake messages get their AWG
  /// junk prefix, every packet gets the AWG type/padding wrap.
  fn send_outbound(&mut self, raw: &[u8]) {
    let kind = match raw_message_type(raw) {
      Some(
        kind @ (super::awg::AwgMessage::Initiation
        | super::awg::AwgMessage::Response
        | super::awg::AwgMessage::CookieReply),
      ) => {
        if kind == super::awg::AwgMessage::Initiation {
          if let Some(ref i1) = self.cps_i1 {
            if let Err(e) = self.udp_socket.send_to(i1, self.peer_addr) {
              crate::proxy_server::warn_throttled(&format!("WireGuard I1 send failed err=\"{e}\""));
            }
          }
        }
        for junk in super::awg::junk_packets(&self.awg) {
          if let Err(e) = self.udp_socket.send_to(&junk, self.peer_addr) {
            crate::proxy_server::warn_throttled(&format!("WireGuard junk send failed err=\"{e}\""));
          }
        }
        Some(kind)
      }
      other => other,
    };
    let Some(kind) = kind else {
      return;
    };
    let mut wrapped = raw.to_vec();
    super::awg::wrap_outgoing(&self.awg, kind, &mut wrapped);
    if let Err(e) = self.udp_socket.send_to(&wrapped, self.peer_addr) {
      crate::proxy_server::warn_throttled(&format!("WireGuard UDP send failed err=\"{e}\""));
    }
  }

  /// Deliver one decrypted inner datagram: UDP aimed at a registered detour
  /// relay goes straight to it (bypassing the smoltcp stack and its MTU cap),
  /// everything else is queued for the interface.
  fn route_inner(&mut self, packet: Vec<u8>) {
    // An IPv4 UDP datagram whose destination port belongs to a detour relay.
    if packet.len() >= 28 && packet[0] >> 4 == 4 && packet[9] == 17 {
      let dst_port = u16::from_be_bytes([packet[22], packet[23]]);
      let relay = self
        .detour_relays
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&dst_port)
        .cloned();
      if let Some(relay) = relay {
        let assoc = relay
          .lock()
          .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(client) = assoc.client_addr {
          let ihl = ((packet[0] & 0x0f) as usize) * 4;
          if packet.len() >= ihl + 8 {
            let src = IpEndpoint::new(
              IpAddress::Ipv4(Ipv4Address::new(
                packet[12], packet[13], packet[14], packet[15],
              )),
              u16::from_be_bytes([packet[20], packet[21]]),
            );
            let payload = packet[ihl + 8..].to_vec();
            let resp = build_udp_datagram(src, &payload);
            let _ = assoc.relay.send_to(&resp, client);
            return;
          }
        }
        return;
      }
    }
    self.rx_queue.push_back(packet);
  }

  /// Decrypt every datagram the peer sent. Returns how many arrived. Junk
  /// datagrams (an AmneziaWG peer prefixes its handshake messages with them)
  /// are dropped here and never reach smoltcp.
  fn pump_wg_to_rx(&mut self) -> usize {
    let mut recv_buf = vec![0u8; 2048];
    let mut received = 0;
    loop {
      match self.udp_socket.recv_from(&mut recv_buf) {
        Ok((len, _)) => {
          match super::awg::unwrap_incoming(&self.awg, &mut recv_buf[..len]) {
            super::awg::AwgIncoming::Ignore => continue,
            super::awg::AwgIncoming::Message { len: msg_len, .. } => {
              received += 1;
              let mut dst = vec![0u8; 2048];
              // The guard drops before send_outbound takes its own borrows.
              let outcome = {
                let mut tunn = self
                  .tunn
                  .lock()
                  .unwrap_or_else(std::sync::PoisonError::into_inner);
                match tunn.decapsulate(None, &recv_buf[..msg_len], &mut dst) {
                  TunnResult::WriteToTunnelV4(data, _) | TunnResult::WriteToTunnelV6(data, _) => {
                    Some((data.to_vec(), None))
                  }
                  TunnResult::WriteToNetwork(response) => {
                    Some((Vec::new(), Some(response.to_vec())))
                  }
                  _ => None,
                }
              };
              match outcome {
                Some((data, None)) => self.route_inner(data),
                Some((_, Some(response))) => self.send_outbound(&response),
                _ => {}
              }
            }
          }
        }
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
        Err(_) => break,
      }
    }
    received
  }

  /// Encrypt and send what smoltcp queued. Returns how many packets.
  fn flush_tx_queue(&mut self) -> usize {
    let mut sent = 0;
    while let Some(ip_packet) = self.tx_queue.pop_front() {
      sent += 1;
      let mut dst = vec![0u8; ip_packet.len() + 256];
      // The guard drops before send_outbound takes its own borrows.
      let outcome = {
        let mut tunn = self
          .tunn
          .lock()
          .unwrap_or_else(std::sync::PoisonError::into_inner);
        match tunn.encapsulate(&ip_packet, &mut dst) {
          TunnResult::WriteToNetwork(packet) => Ok(Some(packet.to_vec())),
          TunnResult::Err(e) => Err(e),
          // boringtun has nothing to send right now (e.g. handshake not yet
          // complete); silently drop. smoltcp will retransmit.
          _ => Ok(None),
        }
      };
      match outcome {
        Ok(Some(packet)) => self.send_outbound(&packet),
        Ok(None) => {}
        Err(e) => {
          crate::proxy_server::warn_throttled(&format!(
            "WireGuard encapsulate failed err=\"{e:?}\""
          ));
        }
      }
    }
    sent
  }

  fn tick_timers(&mut self) {
    let mut dst = vec![0u8; 2048];
    let packet = {
      let mut tunn = self
        .tunn
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
      match tunn.update_timers(&mut dst) {
        TunnResult::WriteToNetwork(packet) => Some(packet.to_vec()),
        _ => None,
      }
    };
    if let Some(packet) = packet {
      self.send_outbound(&packet);
    }
  }
}

/// The standard message type a raw boringtun packet carries, if any.
fn raw_message_type(raw: &[u8]) -> Option<super::awg::AwgMessage> {
  if raw.len() < 4 {
    return None;
  }
  match u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) {
    1 => Some(super::awg::AwgMessage::Initiation),
    2 => Some(super::awg::AwgMessage::Response),
    3 => Some(super::awg::AwgMessage::CookieReply),
    4 => Some(super::awg::AwgMessage::Transport),
    _ => None,
  }
}

/// The smoltcp inner-packet budget for this worker.
///
/// The config's MTU line governs it — a CDN-fronted server or a nested
/// tunnel ships 1180-1280 where the old hardcoded 1420 let TCP segments
/// through smoltcp that the path then silently dropped (browser TLS dies,
/// small requests keep working: the "ping green, page times out" picture).
///
/// A chained exit (its endpoint rides another tunnel's SOCKS UDP detour)
/// is clamped further: its WG data packets (MTU+32) travel as the relay's
/// inner UDP/IP payload (+28), so they must fit the relay's MTU. 1188 fits
/// behind any relay with MTU ≥ 1248 — AmneziaWG configs ship 1280, plain
/// WireGuard defaults to 1420.
const CHAINED_EXIT_MTU_CAP: u16 = 1188;

fn effective_device_mtu(config: &WireGuardConfig) -> u16 {
  let base = config.mtu.unwrap_or(1420).min(1420);
  if config.dial_via_socks.is_some() {
    base.min(CHAINED_EXIT_MTU_CAP)
  } else {
    base
  }
}

struct WgRxToken {
  data: Vec<u8>,
}

impl RxToken for WgRxToken {
  fn consume<R, F>(self, f: F) -> R
  where
    F: FnOnce(&[u8]) -> R,
  {
    f(&self.data)
  }
}

struct WgTxToken<'a> {
  tx_queue: &'a mut VecDeque<Vec<u8>>,
}

impl<'a> TxToken for WgTxToken<'a> {
  fn consume<R, F>(self, len: usize, f: F) -> R
  where
    F: FnOnce(&mut [u8]) -> R,
  {
    let mut buf = vec![0u8; len];
    let result = f(&mut buf);
    self.tx_queue.push_back(buf);
    result
  }
}

impl Device for WgDevice {
  type RxToken<'a> = WgRxToken;
  type TxToken<'a> = WgTxToken<'a>;

  fn receive(&mut self, _timestamp: SmolInstant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
    if let Some(data) = self.rx_queue.pop_front() {
      Some((
        WgRxToken { data },
        WgTxToken {
          tx_queue: &mut self.tx_queue,
        },
      ))
    } else {
      None
    }
  }

  fn transmit(&mut self, _timestamp: SmolInstant) -> Option<Self::TxToken<'_>> {
    Some(WgTxToken {
      tx_queue: &mut self.tx_queue,
    })
  }

  fn capabilities(&self) -> DeviceCapabilities {
    let mut caps = DeviceCapabilities::default();
    caps.medium = Medium::Ip;
    caps.max_transmission_unit = self.mtu;
    caps
  }
}

/// How long the UDP DNS query may stay unanswered before the same question
/// retries over TCP/53. Some tunnels drop UDP-53 outright while TCP through
/// them works fine.
const UDP_DNS_DEADLINE: std::time::Duration = std::time::Duration::from_secs(3);

/// Assemble the inner IPv4/UDP datagram for one detour datagram, by hand and
/// straight into the device queue. The smoltcp stack caps packets at the
/// tunnel's MTU, and a detoured exit's WireGuard packets ride whole — its
/// server's MTU is not ours to know. UDP checksum 0 is legal for IPv4.
fn build_inner_udp(
  src_ip: std::net::Ipv4Addr,
  src_port: u16,
  dst: IpEndpoint,
  payload: &[u8],
) -> Option<Vec<u8>> {
  let dst4 = match dst.addr {
    IpAddress::Ipv4(v4) => v4.octets(),
    IpAddress::Ipv6(_) => {
      crate::proxy_server::warn_throttled(
        "Detour datagram to an IPv6 destination is not supported; dropped",
      );
      return None;
    }
  };
  let udp_len = 8 + payload.len();
  let total = 20 + udp_len;
  let mut out = Vec::with_capacity(total);
  out.push(0x45); // IPv4, IHL 5
  out.push(0x00); // DSCP
  out.extend_from_slice(&(total as u16).to_be_bytes());
  out.extend_from_slice(&rand::random::<u16>().to_be_bytes()); // id
  out.extend_from_slice(&[0x00, 0x00]); // no DF, fragment offset 0
  out.push(64); // TTL
  out.push(17); // UDP
  out.extend_from_slice(&[0, 0]); // header checksum, filled below
  out.extend_from_slice(&src_ip.octets());
  out.extend_from_slice(&dst4);
  out.extend_from_slice(&src_port.to_be_bytes());
  out.extend_from_slice(&dst.port.to_be_bytes());
  out.extend_from_slice(&(udp_len as u16).to_be_bytes());
  out.extend_from_slice(&[0, 0]); // UDP checksum 0 (legal for IPv4)
  out.extend_from_slice(payload);
  let mut sum = 0u32;
  for i in (0..20).step_by(2) {
    sum += u16::from_be_bytes([out[i], out[i + 1]]) as u32;
  }
  while sum >> 16 != 0 {
    sum = (sum >> 16) + (sum & 0xffff);
  }
  out[10..12].copy_from_slice(&(!(sum as u16)).to_be_bytes());
  Some(out)
}
/// The whole DNS-over-TCP exchange (connect + write + read) gets one window.
const TCP_DNS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

/// A DNS query in flight over a TCP/53 connection through the tunnel: the
/// fallback for tunnels whose UDP-53 is dropped (a CDN-fronted AWG hop
/// carries TCP while every UDP datagram to :53 vanishes).
struct PendingDnsTcp {
  port: u16,
  domain: String,
  want_ipv6: bool,
  /// Whether the other address family was already tried for this connect.
  fell_back: bool,
  /// The framed query (2-byte length prefix + message) left to send.
  framed: Vec<u8>,
  sent: bool,
  buf: Vec<u8>,
  started_at: std::time::Instant,
}

/// Replace a connection's socket slot with a fresh placeholder TCP socket.
/// The old socket is spent (a DNS exchange rode it, or the association
/// changed form); the convention is one smoltcp socket per connection.
fn reset_conn_socket(sockets: &mut SocketSet<'_>, old_handle: SocketHandle) -> SocketHandle {
  sockets.remove(old_handle);
  let tcp_rx = SocketBuffer::new(vec![0u8; SMOLTCP_TCP_RX_BUF]);
  let tcp_tx = SocketBuffer::new(vec![0u8; SMOLTCP_TCP_TX_BUF]);
  sockets.add(TcpSocket::new(tcp_rx, tcp_tx))
}

/// Begin a DNS-over-TCP exchange through the tunnel: a fresh socket connects
/// to the resolver's TCP/53 with the framed query queued. Returns the new
/// slot handle for `conn.smol_handle`.
#[allow(clippy::too_many_arguments)]
fn start_dns_over_tcp(
  sockets: &mut SocketSet<'_>,
  iface: &mut Interface,
  old_handle: SocketHandle,
  dns_ip: IpAddress,
  port: u16,
  domain: String,
  want_ipv6: bool,
  fell_back: bool,
) -> Result<(SocketHandle, PendingDnsTcp), ()> {
  let framed = super::dns_tcp::frame_query(&super::dns_tcp::build_query(
    &domain,
    want_ipv6,
    rand::random::<u16>(),
  ));
  let handle = reset_conn_socket(sockets, old_handle);
  let socket = sockets.get_mut::<TcpSocket>(handle);
  let local_port = 10000 + (rand::random::<u16>() % 50000);
  if socket
    .connect(iface.context(), (dns_ip, 53), local_port)
    .is_err()
  {
    sockets.remove(handle);
    return Err(());
  }
  log::debug!("DNS-over-TCP fallback started domain={domain} dns={dns_ip} port={port}");
  Ok((
    handle,
    PendingDnsTcp {
      port,
      domain,
      want_ipv6,
      fell_back,
      framed,
      sent: false,
      buf: Vec::new(),
      started_at: std::time::Instant::now(),
    },
  ))
}

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

fn parse_one_cidr(entry: &str) -> Result<(IpCidr, IpAddress), VpnError> {
  let parts: Vec<&str> = entry.split('/').collect();
  let ip_str = parts[0].trim();

  let ip: std::net::IpAddr = ip_str
    .parse()
    .map_err(|_| VpnError::InvalidWireGuard(format!("Invalid IP address: {ip_str}")))?;

  let prefix = if parts.len() > 1 {
    parts[1]
      .trim()
      .parse::<u8>()
      .map_err(|_| VpnError::InvalidWireGuard(format!("Invalid prefix length: {}", parts[1])))?
  } else if ip.is_ipv6() {
    128
  } else {
    32
  };

  match ip {
    std::net::IpAddr::V4(v4) => {
      let smol_ip = Ipv4Address::new(
        v4.octets()[0],
        v4.octets()[1],
        v4.octets()[2],
        v4.octets()[3],
      );
      Ok((
        IpCidr::new(IpAddress::Ipv4(smol_ip), prefix),
        IpAddress::Ipv4(smol_ip),
      ))
    }
    std::net::IpAddr::V6(v6) => {
      let smol_ip = smoltcp::wire::Ipv6Address::from(v6.octets());
      Ok((
        IpCidr::new(IpAddress::Ipv6(smol_ip), prefix),
        IpAddress::Ipv6(smol_ip),
      ))
    }
  }
}

/// Parse every address in a (comma-separated) WireGuard `Address` line. A
/// dual-stack config yields both the IPv4 and IPv6 entries so the tunnel
/// interface is addressed for each family it carries.
fn parse_cidr_addresses(addr: &str) -> Result<Vec<(IpCidr, IpAddress)>, VpnError> {
  let mut out = Vec::new();
  for entry in addr.split(',') {
    let entry = entry.trim();
    if entry.is_empty() {
      continue;
    }
    out.push(parse_one_cidr(entry)?);
  }
  if out.is_empty() {
    return Err(VpnError::InvalidWireGuard(format!(
      "No interface address in: {addr}"
    )));
  }
  Ok(out)
}

/// Convert a smoltcp `IpAddress` to a std `IpAddr`. Both smoltcp address types
/// are re-exports of `core::net`, so this is a straight variant map.
fn smol_to_std_ip(ip: IpAddress) -> std::net::IpAddr {
  match ip {
    IpAddress::Ipv4(v4) => std::net::IpAddr::V4(v4),
    IpAddress::Ipv6(v6) => std::net::IpAddr::V6(v6),
  }
}

pub struct WireGuardSocks5Server {
  config: WireGuardConfig,
  port: u16,
}

impl WireGuardSocks5Server {
  pub fn new(config: WireGuardConfig, port: u16) -> Self {
    Self { config, port }
  }

  fn create_tunnel(&self) -> Result<Box<Tunn>, VpnError> {
    let private_key_bytes = parse_key(&self.config.private_key)?;
    let static_private = StaticSecret::from(private_key_bytes);

    let peer_public_bytes = parse_key(&self.config.peer_public_key)?;
    let peer_public = PublicKey::from(peer_public_bytes);

    let preshared_key = if let Some(ref psk) = self.config.preshared_key {
      Some(parse_key(psk)?)
    } else {
      None
    };

    Ok(Box::new(Tunn::new(
      static_private,
      peer_public,
      preshared_key,
      self.config.persistent_keepalive,
      0,
      None,
    )))
  }

  fn resolve_endpoint(&self) -> Result<SocketAddr, VpnError> {
    self
      .config
      .peer_endpoint
      .to_socket_addrs()
      .map_err(|e| {
        VpnError::Connection(format!(
          "Failed to resolve endpoint '{}': {e}",
          self.config.peer_endpoint
        ))
      })?
      .next()
      .ok_or_else(|| {
        VpnError::Connection(format!(
          "No addresses found for endpoint: {}",
          self.config.peer_endpoint
        ))
      })
  }

  /// Returns how many attempts the handshake took.
  fn do_handshake(
    tunn: &mut Tunn,
    socket: &WgSocket,
    peer_addr: SocketAddr,
    awg: &super::awg::AwgParams,
    cps_i1: Option<&[u8]>,
  ) -> Result<u32, VpnError> {
    socket
      .set_read_timeout(Some(std::time::Duration::from_secs(5)))
      .map_err(|e| VpnError::Connection(format!("Failed to set timeout: {e}")))?;

    // WireGuard handshakes use UDP which can silently lose packets, especially
    // through Docker port-forwarding layers. Retry the handshake initiation up
    // to 5 times (25s total) before giving up — the protocol is designed for
    // retransmission and peers handle duplicate initiations correctly.
    let max_attempts = 5;
    let mut last_error = String::from("no handshake attempt completed");

    for attempt in 1..=max_attempts {
      if let Some(i1) = cps_i1 {
        socket
          .send_to(i1, peer_addr)
          .map_err(|e| VpnError::Connection(format!("Failed to send I1: {e}")))?;
      }
      // An AmneziaWG peer expects its junk datagrams before the initiation.
      for junk in super::awg::junk_packets(awg) {
        socket
          .send_to(&junk, peer_addr)
          .map_err(|e| VpnError::Connection(format!("Failed to send junk: {e}")))?;
      }
      let mut dst = vec![0u8; 2048];
      let result = tunn.format_handshake_initiation(&mut dst, false);

      match result {
        TunnResult::WriteToNetwork(packet) => {
          let mut wrapped = packet.to_vec();
          super::awg::wrap_outgoing(awg, super::awg::AwgMessage::Initiation, &mut wrapped);
          socket
            .send_to(&wrapped, peer_addr)
            .map_err(|e| VpnError::Connection(format!("Failed to send handshake: {e}")))?;
        }
        TunnResult::Err(e) => {
          return Err(VpnError::Tunnel(format!(
            "Handshake initiation failed: {e:?}"
          )));
        }
        _ => {}
      }

      // Wait for the response, dropping the peer's junk datagrams on the way
      // (an AmneziaWG peer prefixes its response with `Jc` of them).
      let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
      let mut recv_buf = vec![0u8; 2048];
      loop {
        let now = std::time::Instant::now();
        if now >= deadline {
          last_error = "timeout: handshake response deadline passed".to_string();
          break;
        }
        socket
          .set_read_timeout(Some(deadline - now))
          .map_err(|e| VpnError::Connection(format!("Failed to set timeout: {e}")))?;
        match socket.recv_from(&mut recv_buf) {
          Ok((len, _)) => {
            let kind = match super::awg::unwrap_incoming(awg, &mut recv_buf[..len]) {
              super::awg::AwgIncoming::Ignore => continue,
              super::awg::AwgIncoming::Message { kind, len: msg_len } => (kind, msg_len),
            };
            // Anything but a handshake response (e.g. transport from a stale
            // session) is not what this attempt waits for; keep reading.
            if kind.0 != super::awg::AwgMessage::Response {
              continue;
            }
            let result = tunn.decapsulate(None, &recv_buf[..kind.1], &mut dst);
            match result {
              TunnResult::WriteToNetwork(response) => {
                let mut wrapped = response.to_vec();
                super::awg::wrap_outgoing(awg, super::awg::AwgMessage::Response, &mut wrapped);
                for junk in super::awg::junk_packets(awg) {
                  let _ = socket.send_to(&junk, peer_addr);
                }
                socket
                  .send_to(&wrapped, peer_addr)
                  .map_err(|e| VpnError::Connection(format!("Failed to send response: {e}")))?;
              }
              TunnResult::Done => {}
              TunnResult::Err(e) => {
                last_error = format!("handshake response error: {e:?}");
                log::debug!(
                  "WireGuard handshake attempt failed attempt={attempt}/{max_attempts} err=\"{last_error}\""
                );
                break;
              }
              _ => {}
            }

            socket
              .set_read_timeout(None)
              .map_err(|e| VpnError::Connection(format!("Failed to clear timeout: {e}")))?;
            return Ok(attempt);
          }
          Err(e) => {
            last_error = format!("timeout: {e}");
            break;
          }
        }
      }

      if attempt < max_attempts {
        log::debug!(
          "WireGuard handshake attempt failed attempt={attempt}/{max_attempts} err=\"{last_error}\""
        );
      }
    }

    Err(VpnError::Connection(format!(
      "Handshake failed after {max_attempts} attempts: {last_error}"
    )))
  }

  pub async fn run(
    self,
    config_id: String,
    config_path: Option<std::path::PathBuf>,
  ) -> Result<(), VpnError> {
    let started = std::time::Instant::now();
    let peer_addr = self.resolve_endpoint()?;
    let mut tunn = self.create_tunnel()?;

    let uplink: WgSocket = if let Some(detour) = &self.config.dial_via_socks {
      let relay = detour
        .to_socket_addrs()
        .map_err(|e| VpnError::Connection(format!("Bad SOCKS detour '{detour}': {e}")))?
        .next()
        .ok_or_else(|| VpnError::Connection(format!("SOCKS detour '{detour}' did not resolve")))?;
      log::info!(
        "WireGuard worker dialing endpoint via SOCKS detour detour={relay} endpoint={peer_addr}"
      );
      let tunnel = super::wireguard::SocksUdpTunnel::connect(relay)
        .map_err(|e| VpnError::Connection(format!("SOCKS detour associate failed: {e}")))?;
      WgSocket::Detour(Arc::new(tunnel))
    } else {
      let socket = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| VpnError::Connection(format!("Failed to create UDP socket: {e}")))?;
      WgSocket::Direct(Arc::new(socket))
    };

    let handshake_attempts = Self::do_handshake(
      &mut tunn,
      &uplink,
      peer_addr,
      &self.config.awg,
      self.config.cps_i1.as_deref(),
    )?;

    uplink.set_nonblocking(true)?;

    let local_addrs = parse_cidr_addresses(&self.config.address)?;

    let tunn_arc = Arc::new(Mutex::new(tunn));
    let udp_arc = Arc::new(uplink);

    let device_mtu = effective_device_mtu(&self.config);
    log::info!(
      "WireGuard inner MTU vpn={} mtu={device_mtu}",
      self.config.peer_endpoint
    );
    // The source IP detour datagrams leave with: the tunnel's first IPv4.
    let inner_src_ip = local_addrs.iter().find_map(|(_, ip)| match ip {
      IpAddress::Ipv4(v4) => {
        let o = v4.octets();
        Some(std::net::Ipv4Addr::new(o[0], o[1], o[2], o[3]))
      }
      _ => None,
    });

    let mut device = WgDevice {
      tunn: tunn_arc.clone(),
      udp_socket: udp_arc.clone(),
      peer_addr,
      awg: self.config.awg,
      cps_i1: self.config.cps_i1.clone(),
      mtu: device_mtu as usize,
      detour_relays: Arc::new(Mutex::new(HashMap::new())),
      rx_queue: VecDeque::new(),
      tx_queue: VecDeque::new(),
    };

    let iface_config = IfaceConfig::new(HardwareAddress::Ip);
    let mut iface = Interface::new(iface_config, &mut device, SmolInstant::now());
    iface.update_ip_addrs(|addrs| {
      for (cidr, _) in &local_addrs {
        let _ = addrs.push(*cidr);
      }
    });

    // Install a default route for every address family the tunnel carries.
    // WireGuard is a routed (Medium::Ip) link, so the gateway is nominal — there
    // is no L2/neighbor resolution; smoltcp only uses it to select the default
    // route and then hands the packet straight to the WgDevice. The gateway is
    // derived from the interface address (x.x.x.1 / …::1) purely for form.
    let has_ipv4 = local_addrs
      .iter()
      .any(|(_, ip)| matches!(ip, IpAddress::Ipv4(_)));
    let has_ipv6 = local_addrs
      .iter()
      .any(|(_, ip)| matches!(ip, IpAddress::Ipv6(_)));
    for (_, ip) in &local_addrs {
      match ip {
        IpAddress::Ipv4(v4) => {
          let o = v4.octets();
          let gw = Ipv4Address::new(o[0], o[1], o[2], 1);
          iface
            .routes_mut()
            .add_default_ipv4_route(gw)
            .map_err(|e| VpnError::Tunnel(format!("Failed to add default IPv4 route: {e}")))?;
        }
        IpAddress::Ipv6(v6) => {
          let mut o = v6.octets();
          o[14] = 0;
          o[15] = 1;
          let gw = smoltcp::wire::Ipv6Address::from(o);
          iface
            .routes_mut()
            .add_default_ipv6_route(gw)
            .map_err(|e| VpnError::Tunnel(format!("Failed to add default IPv6 route: {e}")))?;
        }
      }
    }

    let listener = TcpListener::bind(format!("127.0.0.1:{}", self.port))
      .await
      .map_err(|e| VpnError::Connection(format!("Failed to bind SOCKS5 listener: {e}")))?;

    let actual_port = listener
      .local_addr()
      .map_err(|e| VpnError::Connection(format!("Failed to get local addr: {e}")))?
      .port();

    // Update config with actual port and local_url. Prefer the explicit
    // config path the worker was started with — a case where
    // get_storage_dir() in the worker process resolved to a different
    // directory than in the parent (Qubes/sandboxed Linux), causing the
    // write-back to land in the wrong place and the parent to time out.
    let updated = match &config_path {
      Some(path) => crate::vpn_worker_storage::get_vpn_worker_config_from_path(path)
        .or_else(|| crate::vpn_worker_storage::get_vpn_worker_config(&config_id)),
      None => crate::vpn_worker_storage::get_vpn_worker_config(&config_id),
    };
    let vpn_id = updated
      .as_ref()
      .map_or_else(|| "-".to_string(), |wc| wc.vpn_id.clone());
    if let Some(mut wc) = updated {
      wc.local_port = Some(actual_port);
      wc.local_url = Some(format!("socks5://127.0.0.1:{}", actual_port));
      let result = match &config_path {
        Some(path) => crate::vpn_worker_storage::save_vpn_worker_config_to_path(&wc, path)
          .map_err(|e| e.to_string()),
        None => crate::vpn_worker_storage::save_vpn_worker_config(&wc).map_err(|e| e.to_string()),
      };
      if let Err(e) = result {
        log::error!(
          "Worker config write-back failed; the app will not see this port id={config_id} path={:?} err=\"{e}\"",
          config_path
        );
      }
    } else {
      log::error!(
        "Worker config write-back skipped: config not found id={config_id} path={:?}",
        config_path
      );
    }

    log::info!(
      "Worker started kind=vpn id={config_id} pid={} listen=127.0.0.1:{actual_port} vpn={vpn_id} vpn_type=wireguard peer={peer_addr} handshake_attempts={handshake_attempts} elapsed_ms={}",
      std::process::id(),
      started.elapsed().as_millis()
    );

    // DNS resolution for domain-name CONNECT requests must go THROUGH the tunnel, never
    // the host resolver (which would leak the query to the local network — the
    // whole point of the VPN). Resolve via the WireGuard config's DNS servers
    // (default 1.1.1.1, still routed through the tunnel) — every IPv4 resolver
    // the config lists, so smoltcp's own retry can rotate between them.
    // `dns_servers` must outlive `sockets` since the socket borrows it, so
    // declare it first.
    let mut dns_servers: Vec<IpAddress> = self
      .config
      .dns
      .iter()
      .flat_map(|s| s.split(','))
      .filter_map(|entry| entry.trim().parse::<std::net::Ipv4Addr>().ok())
      .map(|v4| {
        let o = v4.octets();
        IpAddress::Ipv4(Ipv4Address::new(o[0], o[1], o[2], o[3]))
      })
      .collect();
    if dns_servers.is_empty() {
      dns_servers.push(IpAddress::Ipv4(Ipv4Address::new(1, 1, 1, 1)));
    }
    let dns_servers = dns_servers;

    let mut sockets = SocketSet::new(vec![]);

    // Single DNS socket shared by all connections; queries are driven through
    // the tunnel by iface.poll like every other socket. Build the query storage
    // without vec![None; N] since DnsQuery isn't Clone.
    let dns_queries: Vec<Option<dns::DnsQuery>> = (0..64).map(|_| None).collect();
    let dns_socket = dns::Socket::new(&dns_servers, dns_queries);
    let dns_handle = sockets.add(dns_socket);

    // An in-flight through-tunnel DNS resolution for a domain-name CONNECT.
    struct PendingDns {
      query: dns::QueryHandle,
      port: u16,
      domain: String,
      // true if this is an AAAA query (resolving to IPv6); false for A.
      want_ipv6: bool,
      // whether we've already retried with the other address family.
      fell_back: bool,
      // whether the same question was already retried over TCP/53.
      tcp_tried: bool,
      started_at: std::time::Instant,
    }

    struct Connection {
      smol_handle: SocketHandle,
      tcp_stream: TcpStream,
      socks_done: bool,
      connecting: bool,
      greeting_done: bool,
      read_buf: Vec<u8>,
      dest_addr: Option<SocketAddr>,
      udp: Option<u16>,
      pending_dns: Option<PendingDns>,
      pending_dns_tcp: Option<PendingDnsTcp>,
    }

    let mut connections: Vec<Connection> = Vec::new();
    let mut idle = IdleBackoff::new();
    let mut last_timer_tick = std::time::Instant::now();
    let mut accepted: Option<TcpStream> = None;

    loop {
      // Whether this pass moved bytes, packets or connection state. A pass
      // that did not lets the loop sleep longer.
      let mut active = false;

      // A SOCKS5 connection the wait below accepted.
      if let Some(stream) = accepted.take() {
        active = true;
        let tcp_rx = SocketBuffer::new(vec![0u8; SMOLTCP_TCP_RX_BUF]);
        let tcp_tx = SocketBuffer::new(vec![0u8; SMOLTCP_TCP_TX_BUF]);
        let tcp_socket = TcpSocket::new(tcp_rx, tcp_tx);
        let handle = sockets.add(tcp_socket);

        connections.push(Connection {
          smol_handle: handle,
          tcp_stream: stream,
          socks_done: false,
          connecting: false,
          greeting_done: false,
          read_buf: Vec::new(),
          dest_addr: None,
          udp: None,
          pending_dns: None,
          pending_dns_tcp: None,
        });
      }

      // Pump WireGuard packets into smoltcp rx queue
      if device.pump_wg_to_rx() > 0 {
        active = true;
      }

      // Poll the smoltcp interface
      let timestamp = SmolInstant::now();
      if iface.poll(timestamp, &mut device, &mut sockets) == PollResult::SocketStateChanged {
        active = true;
      }

      // Flush encrypted packets out through WireGuard
      if device.flush_tx_queue() > 0 {
        active = true;
      }

      // Process each connection
      let mut completed = Vec::new();
      for (idx, conn) in connections.iter_mut().enumerate() {
        if let Some(pending) = conn.pending_dns.take() {
          // A through-tunnel DNS resolution is in flight for this connection.
          let result = {
            let dns_sock = sockets.get_mut::<dns::Socket>(dns_handle);
            dns_sock.get_query_result(pending.query)
          };

          // Classify: a usable address of the queried family, a clean "no such
          // record", still-pending, or a hard failure.
          enum DnsOutcome {
            Resolved(IpAddress),
            NoRecord,
            Pending,
            Failed,
          }
          let outcome = match result {
            Ok(addrs) => {
              let want_ipv6 = pending.want_ipv6;
              match addrs.iter().copied().find(|a| {
                matches!(
                  (a, want_ipv6),
                  (IpAddress::Ipv4(_), false) | (IpAddress::Ipv6(_), true)
                )
              }) {
                Some(ip) => DnsOutcome::Resolved(ip),
                None => DnsOutcome::NoRecord,
              }
            }
            Err(dns::GetQueryResultError::Pending) => DnsOutcome::Pending,
            Err(_) => DnsOutcome::Failed,
          };

          match outcome {
            DnsOutcome::Pending => {
              // Still resolving — but only within the UDP window. A tunnel
              // that drops UDP-53 (a CDN-fronted AWG hop carries TCP while
              // every UDP datagram to :53 vanishes) never answers, so past
              // the deadline the same question retries over TCP/53.
              if pending.started_at.elapsed() < UDP_DNS_DEADLINE {
                conn.pending_dns = Some(pending);
              } else if pending.tcp_tried {
                let _ = conn
                  .tcp_stream
                  .try_write(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
                completed.push(idx);
                continue;
              } else {
                match start_dns_over_tcp(
                  &mut sockets,
                  &mut iface,
                  conn.smol_handle,
                  dns_servers[0],
                  pending.port,
                  pending.domain,
                  pending.want_ipv6,
                  pending.fell_back,
                ) {
                  Ok((handle, pending_tcp)) => {
                    conn.smol_handle = handle;
                    conn.pending_dns_tcp = Some(pending_tcp);
                  }
                  Err(()) => {
                    let _ = conn
                      .tcp_stream
                      .try_write(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
                    completed.push(idx);
                    continue;
                  }
                }
              }
            }
            DnsOutcome::Resolved(ip) => {
              active = true;
              conn.dest_addr = Some(SocketAddr::new(smol_to_std_ip(ip), pending.port));
              let socket = sockets.get_mut::<TcpSocket>(conn.smol_handle);
              let local_port = 10000 + (rand::random::<u16>() % 50000);
              if socket
                .connect(iface.context(), (ip, pending.port), local_port)
                .is_err()
              {
                let _ = conn
                  .tcp_stream
                  .try_write(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
                completed.push(idx);
                continue;
              }
              conn.connecting = true;
            }
            DnsOutcome::NoRecord | DnsOutcome::Failed => {
              active = true;
              // No record for this family (or the query failed). The retry
              // ladder: the same family over TCP/53, then the other family
              // over UDP (which may itself fall back to TCP).
              let other_ipv6 = !pending.want_ipv6;
              let other_supported = if other_ipv6 { has_ipv6 } else { has_ipv4 };
              if !pending.tcp_tried {
                match start_dns_over_tcp(
                  &mut sockets,
                  &mut iface,
                  conn.smol_handle,
                  dns_servers[0],
                  pending.port,
                  pending.domain,
                  pending.want_ipv6,
                  pending.fell_back,
                ) {
                  Ok((handle, pending_tcp)) => {
                    conn.smol_handle = handle;
                    conn.pending_dns_tcp = Some(pending_tcp);
                  }
                  Err(()) => {
                    let _ = conn
                      .tcp_stream
                      .try_write(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
                    completed.push(idx);
                    continue;
                  }
                }
              } else if !pending.fell_back && other_supported {
                let qtype = if other_ipv6 {
                  smoltcp::wire::DnsQueryType::Aaaa
                } else {
                  smoltcp::wire::DnsQueryType::A
                };
                let dns_sock = sockets.get_mut::<dns::Socket>(dns_handle);
                if let Ok(query) = dns_sock.start_query(iface.context(), &pending.domain, qtype) {
                  conn.pending_dns = Some(PendingDns {
                    query,
                    port: pending.port,
                    domain: pending.domain,
                    want_ipv6: other_ipv6,
                    fell_back: true,
                    tcp_tried: false,
                    started_at: std::time::Instant::now(),
                  });
                } else {
                  let _ = conn
                    .tcp_stream
                    .try_write(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
                  completed.push(idx);
                  continue;
                }
              } else {
                let _ = conn
                  .tcp_stream
                  .try_write(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
                completed.push(idx);
                continue;
              }
            }
          }
        } else if let Some(mut pending) = conn.pending_dns_tcp.take() {
          // DNS-over-TCP in flight through the tunnel (the fallback for
          // tunnels whose UDP-53 is dropped).
          if pending.started_at.elapsed() > TCP_DNS_TIMEOUT {
            active = true;
            let _ = conn
              .tcp_stream
              .try_write(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
            completed.push(idx);
            continue;
          }

          #[derive(Debug)]
          enum TcpDnsOutcome {
            Waiting,
            Resolved(IpAddress),
            NoRecord,
            Failed,
          }
          let mut progressed = false;
          let outcome = {
            let socket = sockets.get_mut::<TcpSocket>(conn.smol_handle);
            if !pending.sent {
              // Queue the framed query; a partial write resumes next pass.
              if let Ok(n) = socket.send_slice(&pending.framed) {
                if n > 0 {
                  progressed = true;
                  pending.framed.drain(..n);
                  pending.sent = pending.framed.is_empty();
                }
              }
              if pending.sent {
                TcpDnsOutcome::Waiting
              } else if !socket.is_open() {
                TcpDnsOutcome::Failed
              } else {
                TcpDnsOutcome::Waiting
              }
            } else {
              // Pull whatever arrived and parse once the full message is in.
              let mut chunk = [0u8; 4096];
              while let Ok(n) = socket.recv_slice(&mut chunk) {
                if n == 0 {
                  break;
                }
                progressed = true;
                pending.buf.extend_from_slice(&chunk[..n]);
              }
              if pending.buf.len() >= 2 {
                let msg_len = u16::from_be_bytes([pending.buf[0], pending.buf[1]]) as usize;
                if pending.buf.len() >= 2 + msg_len {
                  match super::dns_tcp::parse_response(
                    &pending.buf[2..2 + msg_len],
                    pending.want_ipv6,
                  ) {
                    super::dns_tcp::DnsAnswer::Resolved(ip) => TcpDnsOutcome::Resolved(ip),
                    super::dns_tcp::DnsAnswer::NoRecord => TcpDnsOutcome::NoRecord,
                    super::dns_tcp::DnsAnswer::Invalid => TcpDnsOutcome::Failed,
                  }
                } else if !socket.is_open() {
                  TcpDnsOutcome::Failed
                } else {
                  TcpDnsOutcome::Waiting
                }
              } else if !socket.is_open() {
                TcpDnsOutcome::Failed
              } else {
                TcpDnsOutcome::Waiting
              }
            }
          };

          match outcome {
            TcpDnsOutcome::Waiting => {
              if progressed {
                active = true;
              }
              conn.pending_dns_tcp = Some(pending);
            }
            TcpDnsOutcome::Resolved(ip) => {
              active = true;
              log::debug!("DNS-over-TCP resolved domain={} ip={ip}", pending.domain);
              conn.dest_addr = Some(SocketAddr::new(smol_to_std_ip(ip), pending.port));
              // The exchange socket is spent: a fresh one connects onward.
              let handle = reset_conn_socket(&mut sockets, conn.smol_handle);
              conn.smol_handle = handle;
              let socket = sockets.get_mut::<TcpSocket>(conn.smol_handle);
              let local_port = 10000 + (rand::random::<u16>() % 50000);
              if socket
                .connect(iface.context(), (ip, pending.port), local_port)
                .is_err()
              {
                let _ = conn
                  .tcp_stream
                  .try_write(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
                completed.push(idx);
                continue;
              }
              conn.connecting = true;
            }
            TcpDnsOutcome::NoRecord | TcpDnsOutcome::Failed => {
              active = true;
              log::debug!(
                "DNS-over-TCP gave up domain={} outcome={:?}",
                pending.domain,
                outcome
              );
              // The reliable path also came up empty: try the other address
              // family over UDP once, else refuse the connect.
              let other_ipv6 = !pending.want_ipv6;
              let other_supported = if other_ipv6 { has_ipv6 } else { has_ipv4 };
              if !pending.fell_back && other_supported {
                let qtype = if other_ipv6 {
                  smoltcp::wire::DnsQueryType::Aaaa
                } else {
                  smoltcp::wire::DnsQueryType::A
                };
                let handle = reset_conn_socket(&mut sockets, conn.smol_handle);
                conn.smol_handle = handle;
                let dns_sock = sockets.get_mut::<dns::Socket>(dns_handle);
                if let Ok(query) = dns_sock.start_query(iface.context(), &pending.domain, qtype) {
                  conn.pending_dns = Some(PendingDns {
                    query,
                    port: pending.port,
                    domain: pending.domain,
                    want_ipv6: other_ipv6,
                    fell_back: true,
                    tcp_tried: false,
                    started_at: std::time::Instant::now(),
                  });
                } else {
                  let _ = conn
                    .tcp_stream
                    .try_write(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
                  completed.push(idx);
                  continue;
                }
              } else {
                let _ = conn
                  .tcp_stream
                  .try_write(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
                completed.push(idx);
                continue;
              }
            }
          }
        } else if conn.connecting {
          let socket = sockets.get_mut::<TcpSocket>(conn.smol_handle);
          if socket.may_send() {
            active = true;
            let _ = conn.tcp_stream.try_write(&[
              0x05,
              0x00,
              0x00,
              0x01,
              127,
              0,
              0,
              1,
              (actual_port >> 8) as u8,
              (actual_port & 0xff) as u8,
            ]);
            conn.connecting = false;
            conn.socks_done = true;
          } else if !socket.is_open() {
            let _ = conn
              .tcp_stream
              .try_write(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
            completed.push(idx);
          }
        } else if !conn.socks_done {
          // Handle SOCKS5 handshake
          let mut buf = [0u8; 512];
          match conn.tcp_stream.try_read(&mut buf) {
            Ok(0) => {
              completed.push(idx);
              continue;
            }
            Ok(n) => {
              active = true;
              conn.read_buf.extend_from_slice(&buf[..n]);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => {
              completed.push(idx);
              continue;
            }
          }

          if !conn.greeting_done && conn.read_buf.len() >= 3 {
            // SOCKS5 greeting: version, nmethods, methods
            if conn.read_buf[0] != 0x05 {
              completed.push(idx);
              continue;
            }
            let nmethods = conn.read_buf[1] as usize;
            if conn.read_buf.len() < 2 + nmethods {
              continue;
            }
            // Reply: no auth required
            if conn.tcp_stream.try_write(&[0x05, 0x00]).is_err() {
              completed.push(idx);
              continue;
            }
            conn.read_buf.drain(..2 + nmethods);
            conn.greeting_done = true;
          }

          if conn.greeting_done && conn.dest_addr.is_none() && conn.read_buf.len() >= 10 {
            // SOCKS5 request: CONNECT (0x01) or UDP ASSOCIATE (0x03)
            if conn.read_buf[0] != 0x05 {
              completed.push(idx);
              continue;
            }
            let cmd = conn.read_buf[1];
            if cmd != 0x01 && cmd != 0x03 {
              // command not supported
              let _ = conn
                .tcp_stream
                .try_write(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
              completed.push(idx);
              continue;
            }

            let (addr, addr_len) = match conn.read_buf[3] {
              0x01 => {
                // IPv4
                if conn.read_buf.len() < 10 {
                  continue;
                }
                let ip = std::net::Ipv4Addr::new(
                  conn.read_buf[4],
                  conn.read_buf[5],
                  conn.read_buf[6],
                  conn.read_buf[7],
                );
                let port = u16::from_be_bytes([conn.read_buf[8], conn.read_buf[9]]);
                (SocketAddr::new(std::net::IpAddr::V4(ip), port), 10)
              }
              0x03 => {
                // Domain name
                let domain_len = conn.read_buf[4] as usize;
                let needed = 4 + 1 + domain_len + 2;
                if conn.read_buf.len() < needed {
                  continue;
                }
                let domain = String::from_utf8_lossy(&conn.read_buf[5..5 + domain_len]).to_string();
                let port_start = 5 + domain_len;
                let port =
                  u16::from_be_bytes([conn.read_buf[port_start], conn.read_buf[port_start + 1]]);
                if cmd == 0x03 {
                  // UDP ASSOCIATE ignores the DST address (the relay learns the
                  // client's source from its first datagram), so no resolution
                  // is needed — never resolve it on the host.
                  (
                    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), port),
                    needed,
                  )
                } else {
                  // TCP CONNECT: resolve the domain THROUGH THE TUNNEL via the
                  // config DNS server, never the host resolver (which would leak
                  // the query onto the local network). Prefer IPv4 when the
                  // tunnel carries it (the well-tested path) and fall back to
                  // AAAA only for names with no A record; an IPv6-only tunnel
                  // resolves AAAA directly. Start an async query and defer the
                  // connection until it resolves.
                  conn.read_buf.drain(..needed);
                  let want_ipv6 = !has_ipv4;
                  let qtype = if want_ipv6 {
                    smoltcp::wire::DnsQueryType::Aaaa
                  } else {
                    smoltcp::wire::DnsQueryType::A
                  };
                  let dns_sock = sockets.get_mut::<dns::Socket>(dns_handle);
                  match dns_sock.start_query(iface.context(), &domain, qtype) {
                    Ok(query) => {
                      conn.pending_dns = Some(PendingDns {
                        query,
                        port,
                        domain,
                        want_ipv6,
                        fell_back: false,
                        tcp_tried: false,
                        started_at: std::time::Instant::now(),
                      });
                    }
                    Err(e) => {
                      crate::proxy_server::warn_throttled(&format!(
                        "Tunnel DNS query start failed err=\"{e:?}\""
                      ));
                      let _ = conn
                        .tcp_stream
                        .try_write(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
                      completed.push(idx);
                    }
                  }
                  continue;
                }
              }
              0x04 => {
                // IPv6-literal CONNECT. Route it through the tunnel when the
                // tunnel carries IPv6; otherwise it can never be reached (no
                // IPv6 address/route), so fail fast with "host unreachable"
                // rather than letting the smoltcp socket sit in SynSent until it
                // times out (a visible hang).
                if conn.read_buf.len() < 22 {
                  continue;
                }
                if !has_ipv6 {
                  let _ = conn
                    .tcp_stream
                    .try_write(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
                  completed.push(idx);
                  continue;
                }
                let mut octets = [0u8; 16];
                octets.copy_from_slice(&conn.read_buf[4..20]);
                let ip = std::net::Ipv6Addr::from(octets);
                let port = u16::from_be_bytes([conn.read_buf[20], conn.read_buf[21]]);
                (SocketAddr::new(std::net::IpAddr::V6(ip), port), 22)
              }
              _ => {
                completed.push(idx);
                continue;
              }
            };

            conn.read_buf.drain(..addr_len);

            if cmd == 0x03 {
              // === SOCKS5 UDP ASSOCIATE ===
              // The request's DST is the client's intended source (typically
              // 0.0.0.0:0) and is ignored — the browser's relay source is
              // learned from its first datagram. Bind a loopback relay socket
              // the client sends to; its datagrams leave through the tunnel
              // with the inner-IP bypass (no smoltcp socket, no MTU cap — a
              // detoured exit's own WireGuard packets ride whole).
              let relay = match UdpSocket::bind("127.0.0.1:0") {
                Ok(s) => s,
                Err(_) => {
                  let _ = conn
                    .tcp_stream
                    .try_write(&[0x05, 0x01, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
                  completed.push(idx);
                  continue;
                }
              };
              let _ = relay.set_nonblocking(true);
              let relay_port = relay.local_addr().map(|a| a.port()).unwrap_or(0);

              // Reply with the relay endpoint (127.0.0.1:relay_port).
              if conn
                .tcp_stream
                .try_write(&[
                  0x05,
                  0x00,
                  0x00,
                  0x01,
                  127,
                  0,
                  0,
                  1,
                  (relay_port >> 8) as u8,
                  (relay_port & 0xff) as u8,
                ])
                .is_err()
              {
                completed.push(idx);
                continue;
              }

              let inner_port = 20000 + (rand::random::<u16>() % 40000);
              device
                .detour_relays
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(
                  inner_port,
                  Arc::new(Mutex::new(UdpAssoc {
                    relay,
                    client_addr: None,
                  })),
                );
              conn.udp = Some(inner_port);
              conn.socks_done = true;
              continue;
            }

            conn.dest_addr = Some(addr);

            // Open smoltcp TCP socket to the destination
            let socket = sockets.get_mut::<TcpSocket>(conn.smol_handle);
            let smol_addr = match addr.ip() {
              std::net::IpAddr::V4(v4) => {
                let o = v4.octets();
                IpAddress::Ipv4(Ipv4Address::new(o[0], o[1], o[2], o[3]))
              }
              std::net::IpAddr::V6(v6) => {
                IpAddress::Ipv6(smoltcp::wire::Ipv6Address::from(v6.octets()))
              }
            };

            let local_port = 10000 + (rand::random::<u16>() % 50000);
            if socket
              .connect(iface.context(), (smol_addr, addr.port()), local_port)
              .is_err()
            {
              let _ = conn
                .tcp_stream
                .try_write(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
              completed.push(idx);
              continue;
            }

            conn.connecting = true;
          }
        } else if let Some(inner_port) = conn.udp {
          // === UDP ASSOCIATE relay ===
          // The association lives only while the TCP control connection is
          // open (RFC 1928 §6); tear down when the browser closes it.
          let mut probe = [0u8; 1];
          match conn.tcp_stream.try_read(&mut probe) {
            Ok(0) => {
              completed.push(idx);
              continue;
            }
            Ok(_) => {} // ignore any data on the control channel
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => {
              completed.push(idx);
              continue;
            }
          }

          let Some(relay) = device
            .detour_relays
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&inner_port)
            .cloned()
          else {
            completed.push(idx);
            continue;
          };
          let mut udp = relay
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

          // Client → tunnel: strip the §7 header and forward the payload as a
          // hand-built inner IP/UDP datagram (bypasses the smoltcp stack and
          // its MTU cap).
          let mut dbuf = [0u8; SMOLTCP_UDP_BUF];
          loop {
            match udp.relay.recv_from(&mut dbuf) {
              Ok((n, src)) => {
                active = true;
                udp.client_addr = Some(src);
                if let Some((dst, off)) = parse_udp_datagram(&dbuf[..n]) {
                  if let Some(packet) = inner_src_ip
                    .and_then(|src_ip| build_inner_udp(src_ip, inner_port, dst, &dbuf[off..n]))
                  {
                    device.tx_queue.push_back(packet);
                  }
                }
              }
              Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
              Err(_) => break,
            }
          }
        } else {
          // Data relay between SOCKS5 client and smoltcp socket. Bytes move
          // only as far as the other side takes them: what smoltcp has no
          // room for stays in the client socket, and what the client cannot
          // take yet stays in smoltcp, so a full buffer never drops data.
          let socket = sockets.get_mut::<TcpSocket>(conn.smol_handle);

          // Client → smoltcp, read straight into smoltcp's free space.
          if socket.can_send() {
            let read = socket.send(|free| {
              if free.is_empty() {
                return (0, Err(std::io::ErrorKind::WouldBlock.into()));
              }
              match conn.tcp_stream.try_read(free) {
                Ok(n) => (n, Ok(n)),
                Err(e) => (0, Err(e)),
              }
            });
            match read {
              Ok(Ok(0)) => {
                socket.close();
                completed.push(idx);
                continue;
              }
              Ok(Ok(_)) => active = true,
              Ok(Err(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
              Ok(Err(_)) | Err(_) => {
                socket.close();
                completed.push(idx);
                continue;
              }
            }
          }

          // smoltcp → Client: dequeue only what the client socket took. Two
          // rounds cover a receive buffer that wraps around.
          let mut write_failed = false;
          for _ in 0..2 {
            if !socket.can_recv() {
              break;
            }
            let written = socket.recv(|data| match conn.tcp_stream.try_write(data) {
              Ok(n) => (n, Ok(n)),
              Err(e) => (0, Err(e)),
            });
            match written {
              Ok(Ok(0)) => break,
              Ok(Ok(_)) => active = true,
              Ok(Err(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => break,
              Ok(Err(_)) => {
                write_failed = true;
                break;
              }
              Err(_) => break,
            }
          }
          if write_failed {
            socket.close();
            completed.push(idx);
            continue;
          }

          // Check if smoltcp socket closed
          if !socket.is_open() && !socket.is_active() {
            completed.push(idx);
          }
        }
      }

      // Remove completed connections (in reverse order)
      if !completed.is_empty() {
        active = true;
      }
      completed.sort_unstable();
      completed.dedup();
      for idx in completed.into_iter().rev() {
        let conn = connections.remove(idx);
        if let Some(pending) = conn.pending_dns {
          sockets
            .get_mut::<dns::Socket>(dns_handle)
            .cancel_query(pending.query);
        }
        if let Some(inner_port) = conn.udp {
          device
            .detour_relays
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&inner_port);
        }
        sockets.remove(conn.smol_handle);
      }

      // Timer ticks for WireGuard keepalives
      if last_timer_tick.elapsed() >= WG_TIMER_INTERVAL {
        device.tick_timers();
        last_timer_tick = std::time::Instant::now();
      }

      // Wait for the next pass: briefly while traffic flows, longer while the
      // tunnel is idle, never past a smoltcp or WireGuard timer, and no
      // longer than it takes a new SOCKS5 connection to arrive.
      let smoltcp_due = iface
        .poll_delay(SmolInstant::now(), &sockets)
        .map(|due| std::time::Duration::from_millis(due.total_millis()));
      let timer_due = WG_TIMER_INTERVAL.saturating_sub(last_timer_tick.elapsed());
      let due = Some(smoltcp_due.map_or(timer_due, |due| due.min(timer_due)));
      let wait = idle.next_wait(active, due);
      tokio::select! {
        result = listener.accept() => match result {
          Ok((stream, _addr)) => accepted = Some(stream),
          Err(e) => {
            crate::proxy_server::warn_throttled(&format!("SOCKS5 accept failed err=\"{e}\""));
            tokio::time::sleep(wait).await;
          }
        },
        _ = tokio::time::sleep(wait) => {}
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn wg_config(mtu: Option<u16>, detour: Option<&str>) -> WireGuardConfig {
    WireGuardConfig {
      private_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
      address: "10.0.0.2/24".to_string(),
      dns: None,
      mtu,
      peer_public_key: "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=".to_string(),
      peer_endpoint: "127.0.0.1:51820".to_string(),
      allowed_ips: vec!["0.0.0.0/0".to_string()],
      persistent_keepalive: None,
      preshared_key: None,
      awg: super::super::awg::AwgParams::default(),
      cps_i1: None,
      dial_via_socks: detour.map(str::to_string),
    }
  }

  #[test]
  fn the_device_mtu_follows_the_config_and_clamps_a_chained_exit() {
    assert_eq!(effective_device_mtu(&wg_config(Some(1180), None)), 1180);
    assert_eq!(effective_device_mtu(&wg_config(None, None)), 1420);
    // A conf asking for more than WireGuard's own default stays at 1420.
    assert_eq!(effective_device_mtu(&wg_config(Some(1500), None)), 1420);
    // A chained exit must fit its WG packets inside the relay's tunnel.
    assert_eq!(
      effective_device_mtu(&wg_config(Some(1420), Some("127.0.0.1:60000"))),
      1188
    );
    assert_eq!(
      effective_device_mtu(&wg_config(Some(1180), Some("127.0.0.1:60000"))),
      1180
    );
  }

  #[test]
  fn test_parse_cidr_ipv4() {
    let (cidr, ip) = parse_one_cidr("10.0.0.2/24").unwrap();
    assert_eq!(cidr.prefix_len(), 24);
    assert_eq!(ip, IpAddress::Ipv4(Ipv4Address::new(10, 0, 0, 2)));
  }

  #[test]
  fn test_parse_cidr_no_prefix() {
    let (cidr, _) = parse_one_cidr("10.0.0.2").unwrap();
    assert_eq!(cidr.prefix_len(), 32);
  }

  #[test]
  fn test_parse_cidr_ipv6_default_prefix() {
    let (cidr, ip) = parse_one_cidr("fd00::2").unwrap();
    assert_eq!(cidr.prefix_len(), 128);
    assert!(matches!(ip, IpAddress::Ipv6(_)));
  }

  #[test]
  fn test_parse_cidr_addresses_dual_stack() {
    let addrs = parse_cidr_addresses("10.0.0.2/24, fd00::2/128").unwrap();
    assert_eq!(addrs.len(), 2);
    assert_eq!(addrs[0].1, IpAddress::Ipv4(Ipv4Address::new(10, 0, 0, 2)));
    assert!(matches!(addrs[1].1, IpAddress::Ipv6(_)));
    assert!(addrs.iter().any(|(_, ip)| matches!(ip, IpAddress::Ipv4(_))));
    assert!(addrs.iter().any(|(_, ip)| matches!(ip, IpAddress::Ipv6(_))));
  }

  #[test]
  fn test_parse_key_valid() {
    let key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    assert!(parse_key(key).is_ok());
  }

  #[test]
  fn test_parse_key_invalid() {
    assert!(parse_key("not-valid").is_err());
  }

  #[test]
  fn an_idle_loop_backs_off_and_activity_resets_it() {
    use std::time::Duration;
    let ms = Duration::from_millis;
    let mut idle = IdleBackoff::new();
    assert_eq!(idle.next_wait(true, None), ms(1), "traffic keeps 1 ms");
    let waits: Vec<_> = (0..5).map(|_| idle.next_wait(false, None)).collect();
    assert_eq!(
      waits,
      vec![ms(2), ms(4), ms(8), ms(8), ms(8)],
      "an idle tunnel doubles up to 8 ms"
    );
    assert_eq!(idle.next_wait(true, None), ms(1), "activity drops it back");
    assert_eq!(idle.next_wait(false, None), ms(2));
  }

  #[test]
  fn the_backoff_never_sleeps_past_a_timer_or_spins() {
    use std::time::Duration;
    let ms = Duration::from_millis;
    let mut idle = IdleBackoff::new();
    for _ in 0..10 {
      idle.next_wait(false, None);
    }
    assert_eq!(idle.next_wait(false, Some(ms(5))), ms(5), "a due timer");
    assert_eq!(
      idle.next_wait(false, Some(Duration::ZERO)),
      ms(1),
      "a timer due now still waits 1 ms, so the loop never spins"
    );
    assert_eq!(idle.next_wait(false, Some(ms(500))), ms(8));
  }
}
