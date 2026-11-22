//! AmneziaWG (AWG) wire-format framing.
//!
//! AmneziaWG is WireGuard with an obfuscated envelope: junk datagrams precede
//! every handshake message, each message carries tail padding, and the 4-byte
//! message type is rewritten. The noise layer underneath is untouched, so the
//! standard `boringtun` messages still apply — they only need wrapping on the
//! way out and unwrapping on the way in. A config without AWG parameters
//! parses to the identity transform (H = 1..4, no padding, no junk), so every
//! datagram goes through this module regardless of config kind.
//!
//! Parameter semantics (amneziawg-go, device/send.go & receive.go):
//! - `Jc` junk datagrams of a random size in `Jmin..=Jmax` precede each
//!   handshake initiation, response and cookie reply — never transport data.
//! - `S1`/`S2`/`S3`/`S4` random tail bytes are appended to initiation /
//!   response / cookie / transport packets respectively.
//! - `H1..H4` replace the standard message types 1..4 in the first four bytes
//!   (little-endian u32). The `I1..I5` "in-flight" packets are DPI camouflage
//!   only — the protocol never requires them, so they are not sent.

use serde::{Deserialize, Serialize};

/// The AWG obfuscation parameters of one tunnel. `Default` is plain
/// WireGuard: the identity transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AwgParams {
  pub jc: u32,
  pub jmin: u32,
  pub jmax: u32,
  pub s1: u32,
  pub s2: u32,
  pub s3: u32,
  pub s4: u32,
  pub h1: u32,
  pub h2: u32,
  pub h3: u32,
  pub h4: u32,
}

impl Default for AwgParams {
  fn default() -> Self {
    Self {
      jc: 0,
      jmin: 0,
      jmax: 0,
      s1: 0,
      s2: 0,
      s3: 0,
      s4: 0,
      h1: 1,
      h2: 2,
      h3: 3,
      h4: 4,
    }
  }
}

/// The four WireGuard message kinds, in their standard numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwgMessage {
  Initiation,
  Response,
  CookieReply,
  Transport,
}

impl AwgMessage {
  fn standard_type(self) -> u32 {
    match self {
      Self::Initiation => 1,
      Self::Response => 2,
      Self::CookieReply => 3,
      Self::Transport => 4,
    }
  }

  fn tail_padding(self, awg: &AwgParams) -> u32 {
    match self {
      Self::Initiation => awg.s1,
      Self::Response => awg.s2,
      Self::CookieReply => awg.s3,
      Self::Transport => awg.s4,
    }
  }
}

/// Parse an AmneziaWG 1.5 CPS signature packet (e.g. `I1 = <b 0xce00...>`).
/// Returns the raw payload bytes if the tag is a `<b 0x...>` binary literal.
pub fn parse_cps_packet(raw: &str) -> Option<Vec<u8>> {
  let inner = raw.trim().strip_prefix('<')?.strip_suffix('>')?.trim();
  let hex_str = inner.strip_prefix('b')?.trim();
  let hex_str = hex_str
    .strip_prefix("0x")
    .or_else(|| hex_str.strip_prefix("0X"))?;
  let clean: String = hex_str
    .chars()
    .filter(|c| !c.is_ascii_whitespace())
    .collect();
  if clean.is_empty() || !clean.len().is_multiple_of(2) {
    return None;
  }
  let mut out = Vec::with_capacity(clean.len() / 2);
  for i in (0..clean.len()).step_by(2) {
    out.push(u8::from_str_radix(&clean[i..i + 2], 16).ok()?);
  }
  Some(out)
}

/// One `Jc` junk datagram: random bytes, random length in `Jmin..=Jmax`.
/// Empty when the config carries no junk (plain WireGuard).
pub fn junk_packets(awg: &AwgParams) -> Vec<Vec<u8>> {
  if awg.jc == 0 {
    return Vec::new();
  }
  (0..awg.jc)
    .map(|_| {
      let spread = awg.jmax.saturating_sub(awg.jmin);
      let len = awg.jmin + rand::random::<u32>() % (spread + 1);
      let len = len.max(1) as usize;
      (0..len).map(|_| rand::random::<u8>()).collect()
    })
    .collect()
}

/// Wrap one outgoing packet for the wire: rewrite the message type to the
/// config's H value and append the kind's tail padding. `packet` holds a
/// standard boringtun message on entry and the wire datagram on return.
pub fn wrap_outgoing(awg: &AwgParams, kind: AwgMessage, packet: &mut Vec<u8>) {
  let h = match kind {
    AwgMessage::Initiation => awg.h1,
    AwgMessage::Response => awg.h2,
    AwgMessage::CookieReply => awg.h3,
    AwgMessage::Transport => awg.h4,
  };
  // The standard type sits in the first four bytes as a little-endian u32.
  packet[0..4].copy_from_slice(&h.to_le_bytes());
  let padding = kind.tail_padding(awg) as usize;
  packet.extend((0..padding).map(|_| rand::random::<u8>()));
}

/// What one incoming datagram holds, after unwrapping.
#[derive(Debug)]
pub enum AwgIncoming {
  /// A WireGuard message of `kind`, stripped of padding and rewritten to its
  /// standard type, occupying the first `len` bytes of the datagram.
  Message { kind: AwgMessage, len: usize },
  /// Junk, DPI camouflage or any datagram that is not a mapped message type:
  /// the tunnel must drop it and keep reading.
  Ignore,
}

/// Classify one incoming datagram. The peer's junk packets and the `I1..I5`
/// camouflage packets surface here as [`AwgIncoming::Ignore`]. A recognised
/// message is rewritten in place to its standard type and its length trimmed
/// of the peer's tail padding, so `boringtun` parses `datagram[..len]` directly.
pub fn unwrap_incoming(awg: &AwgParams, datagram: &mut [u8]) -> AwgIncoming {
  if datagram.len() < 4 {
    return AwgIncoming::Ignore;
  }
  let wire_type = u32::from_le_bytes([datagram[0], datagram[1], datagram[2], datagram[3]]);
  let kind = match wire_type {
    t if t == awg.h1 => AwgMessage::Initiation,
    t if t == awg.h2 => AwgMessage::Response,
    t if t == awg.h3 => AwgMessage::CookieReply,
    t if t == awg.h4 => AwgMessage::Transport,
    _ => return AwgIncoming::Ignore,
  };
  // Strip the peer's tail padding. A datagram shorter than the padding claims
  // is malformed; drop it like junk rather than underflowing.
  let padded = kind.tail_padding(awg) as usize;
  if datagram.len() < padded {
    return AwgIncoming::Ignore;
  }
  let mut len = datagram.len() - padded;
  // Rewrite back to the standard type so boringtun parses the message.
  datagram[0..4].copy_from_slice(&kind.standard_type().to_le_bytes());
  // Handshake messages are fixed size; trailing bytes past the standard length
  // that survived the padding strip would still confuse boringtun.
  if kind != AwgMessage::Transport {
    let fixed = match kind {
      AwgMessage::Initiation => 148,
      AwgMessage::Response => 92,
      AwgMessage::CookieReply => 64,
      AwgMessage::Transport => unreachable!(),
    };
    len = len.min(fixed);
  }
  AwgIncoming::Message { kind, len }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_params_are_the_identity_transform() {
    let awg = AwgParams::default();
    assert!(junk_packets(&awg).is_empty());

    // A standard initiation needs no rewrite and no padding.
    let mut packet = vec![0u8; 148];
    packet[0..4].copy_from_slice(&1u32.to_le_bytes());
    let mut out = packet.clone();
    wrap_outgoing(&awg, AwgMessage::Initiation, &mut out);
    assert_eq!(out, packet);

    let buf = &mut packet.clone()[..];
    assert!(matches!(
      unwrap_incoming(&awg, buf),
      AwgIncoming::Message {
        kind: AwgMessage::Initiation,
        len: 148
      }
    ));
  }

  #[test]
  fn junk_fills_the_configured_size_window() {
    let awg = AwgParams {
      jc: 4,
      jmin: 40,
      jmax: 70,
      ..AwgParams::default()
    };
    let junk = junk_packets(&awg);
    assert_eq!(junk.len(), 4);
    assert!(junk.iter().all(|p| (40..=70).contains(&p.len())));
  }

  #[test]
  fn wrap_rewrites_the_type_and_appends_padding() {
    let awg = AwgParams {
      s1: 6,
      h1: 618246737,
      ..AwgParams::default()
    };
    let mut packet = vec![0u8; 148];
    packet[0..4].copy_from_slice(&1u32.to_le_bytes());
    wrap_outgoing(&awg, AwgMessage::Initiation, &mut packet);
    assert_eq!(packet.len(), 154);
    assert_eq!(
      u32::from_le_bytes([packet[0], packet[1], packet[2], packet[3]]),
      618246737
    );
    // Unwrapping restores the standard type and the fixed message length.
    match unwrap_incoming(&awg, &mut packet) {
      AwgIncoming::Message { kind, len } => {
        assert_eq!(kind, AwgMessage::Initiation);
        assert_eq!(len, 148);
        assert_eq!(
          u32::from_le_bytes([packet[0], packet[1], packet[2], packet[3]]),
          1
        );
      }
      _ => panic!("expected a message"),
    }
  }

  #[test]
  fn transport_padding_is_stripped_on_receive() {
    let awg = AwgParams {
      s4: 3,
      ..AwgParams::default()
    };
    let mut packet = vec![7u8; 200];
    packet[0..4].copy_from_slice(&4u32.to_le_bytes());
    match unwrap_incoming(&awg, &mut packet) {
      AwgIncoming::Message { kind, len } => {
        assert_eq!(kind, AwgMessage::Transport);
        assert_eq!(len, 197);
      }
      _ => panic!("expected a transport message"),
    }
  }

  #[test]
  fn unknown_types_are_junk() {
    let awg = AwgParams::default();
    let mut junk = vec![9u8; 50];
    assert!(matches!(
      unwrap_incoming(&awg, &mut junk),
      AwgIncoming::Ignore
    ));
    assert!(matches!(
      unwrap_incoming(&awg, &mut []),
      AwgIncoming::Ignore
    ));
    // Shorter than the claimed padding: dropped, not underflowed.
    let padded = AwgParams {
      s4: 300,
      ..AwgParams::default()
    };
    let mut short = vec![4u8, 0, 0, 0, 1, 2, 3];
    assert!(matches!(
      unwrap_incoming(&padded, &mut short),
      AwgIncoming::Ignore
    ));
  }
}
