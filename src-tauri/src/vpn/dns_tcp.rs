//! DNS-over-TCP for the WG worker (RFC 7766).
//!
//! Plain UDP-53 through some tunnels is simply dropped — a CDN-fronted
//! AmneziaWG hop carried TCP fine while every UDP query to the same server
//! vanished, leaving domains unresolved and pages dead. TCP-53 through that
//! same tunnel works, so the worker falls back to it. Only the tiny slice of
//! DNS this worker needs is implemented: one A or AAAA question, one answer
//! read back.

use smoltcp::wire::IpAddress;

/// Build a recursive A/AAAA query for `domain` (RFC 1035 §4.1).
pub fn build_query(domain: &str, want_ipv6: bool, txid: u16) -> Vec<u8> {
  let mut q = Vec::with_capacity(17 + domain.len());
  q.extend_from_slice(&txid.to_be_bytes());
  q.extend_from_slice(&[0x01, 0x00]); // flags: recursion desired
  q.extend_from_slice(&[0x00, 0x01]); // one question
  q.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00]); // no rrs
  for label in domain.split('.').filter(|l| !l.is_empty()) {
    let bytes = label.as_bytes();
    q.push(bytes.len().min(63) as u8);
    q.extend_from_slice(&bytes[..bytes.len().min(63)]);
  }
  q.push(0);
  let qtype: u16 = if want_ipv6 { 28 } else { 1 };
  q.extend_from_slice(&qtype.to_be_bytes());
  q.extend_from_slice(&[0x00, 0x01]); // class IN
  q
}

/// Wrap a query for the TCP transport: a 2-byte big-endian length prefix.
pub fn frame_query(query: &[u8]) -> Vec<u8> {
  let mut out = Vec::with_capacity(query.len() + 2);
  out.extend_from_slice(&(query.len() as u16).to_be_bytes());
  out.extend_from_slice(query);
  out
}

/// The usable outcome of parsing one DNS response.
#[derive(Debug)]
pub enum DnsAnswer {
  Resolved(IpAddress),
  /// The server answered but carried no record of the wanted family.
  NoRecord,
  /// Not a response to this query, a failure code, or malformed.
  Invalid,
}

/// Parse a DNS response message, taking the first A/AAAA answer (following
/// CNAME chains implicitly by skipping other record types).
pub fn parse_response(msg: &[u8], want_ipv6: bool) -> DnsAnswer {
  // Fixed header; the QR bit must be set (this is a response).
  if msg.len() < 12 || msg[2] & 0x80 == 0 {
    return DnsAnswer::Invalid;
  }
  if msg[3] & 0x0f != 0 {
    // RCODE != NOERROR: NXDOMAIN and friends count as "no such record".
    return DnsAnswer::NoRecord;
  }
  let qd = u16::from_be_bytes([msg[4], msg[5]]);
  let an = u16::from_be_bytes([msg[6], msg[7]]);
  let mut pos = 12usize;
  for _ in 0..qd {
    pos = match skip_name(msg, pos) {
      Some(p) => p + 4, // QTYPE + QCLASS
      None => return DnsAnswer::Invalid,
    };
  }
  for _ in 0..an {
    pos = match skip_name(msg, pos) {
      Some(p) => p,
      None => return DnsAnswer::Invalid,
    };
    if pos + 10 > msg.len() {
      return DnsAnswer::Invalid;
    }
    let rtype = u16::from_be_bytes([msg[pos], msg[pos + 1]]);
    let rdlength = u16::from_be_bytes([msg[pos + 8], msg[pos + 9]]) as usize;
    let rdata = pos + 10;
    if rdata + rdlength > msg.len() {
      return DnsAnswer::Invalid;
    }
    let address = match (rtype, want_ipv6, rdlength) {
      (1, false, 4) => Some(IpAddress::Ipv4(smoltcp::wire::Ipv4Address::new(
        msg[rdata],
        msg[rdata + 1],
        msg[rdata + 2],
        msg[rdata + 3],
      ))),
      (28, true, 16) => {
        let mut o = [0u8; 16];
        o.copy_from_slice(&msg[rdata..rdata + 16]);
        Some(IpAddress::Ipv6(smoltcp::wire::Ipv6Address::from(o)))
      }
      // The right type with a wrong rdata size is malformed, not skippable.
      (1, false, _) | (28, true, _) => return DnsAnswer::Invalid,
      _ => None,
    };
    if let Some(ip) = address {
      return DnsAnswer::Resolved(ip);
    }
    pos = rdata + rdlength;
  }
  DnsAnswer::NoRecord
}

/// Skip one (possibly compressed) name at `pos`. Returns the position just
/// past the name, or None on malformed input.
fn skip_name(msg: &[u8], mut pos: usize) -> Option<usize> {
  let mut hops = 0;
  loop {
    if pos >= msg.len() {
      return None;
    }
    let len = msg[pos];
    if len & 0xc0 == 0xc0 {
      // Compression pointer: two bytes, then the name ends here.
      if pos + 2 > msg.len() {
        return None;
      }
      pos += 2;
      hops += 1;
      if hops > 16 {
        return None;
      }
      return Some(pos);
    }
    if len == 0 {
      return Some(pos + 1);
    }
    if len & 0xc0 != 0 {
      return None;
    }
    pos += 1 + len as usize;
    hops += 1;
    if hops > 32 {
      return None;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const DOMAIN: &str = "ip-api.com";

  #[test]
  fn the_query_carries_the_name_and_class_in() {
    let q = build_query(DOMAIN, false, 0x1a2b);
    // header(12) + 2 labels (6,3) + terminator + qtype(2) + qclass(2)
    assert_eq!(q.len(), 12 + 7 + 4 + 1 + 2 + 2);
    assert_eq!(&q[0..2], &[0x1a, 0x2b]);
    assert_eq!(&q[2..4], &[0x01, 0x00]);
    assert_eq!(&q[12..19], &[6, b'i', b'p', b'-', b'a', b'p', b'i']);
    assert_eq!(&q[19..23], &[3, b'c', b'o', b'm']);
    assert_eq!(&q[23..26], &[0, 0, 1]); // root, A query
    assert_eq!(&q[26..28], &[0, 1]); // IN
                                     // AAAA flips only the question type.
    let q6 = build_query(DOMAIN, true, 1);
    assert_eq!(&q6[24..26], &[0, 28]);
  }

  #[test]
  fn the_query_frames_with_a_length_prefix() {
    let q = build_query(DOMAIN, false, 1);
    let framed = frame_query(&q);
    assert_eq!(framed.len(), q.len() + 2);
    assert_eq!(&framed[0..2], &(q.len() as u16).to_be_bytes());
    assert_eq!(&framed[2..], &q[..]);
  }

  fn response(answer_bytes: &[u8], rtype: u16, qname_encoded: &[u8]) -> Vec<u8> {
    let mut m = vec![0u8; 12];
    m[2] = 0x80; // QR set
    m[4..6].copy_from_slice(&1u16.to_be_bytes()); // one question
    m[6..8].copy_from_slice(&1u16.to_be_bytes()); // one answer
    m.extend_from_slice(qname_encoded);
    m.extend_from_slice(&[0, 1, 0, 1]); // question: A / IN
    m.extend_from_slice(qname_encoded);
    m.extend_from_slice(&rtype.to_be_bytes());
    m.extend_from_slice(&[0, 1]); // class IN
    m.extend_from_slice(&300u32.to_be_bytes()); // TTL
    m.extend_from_slice(&(answer_bytes.len() as u16).to_be_bytes());
    m.extend_from_slice(answer_bytes);
    m
  }

  const NAME: &[u8] = &[
    6, b'i', b'p', b'-', b'a', b'p', b'i', 3, b'c', b'o', b'm', 0,
  ];

  #[test]
  fn an_a_answer_resolves_to_v4() {
    let m = response(&[93, 184, 216, 34], 1, NAME);
    match parse_response(&m, false) {
      DnsAnswer::Resolved(IpAddress::Ipv4(v4)) => {
        assert_eq!(v4.octets(), [93, 184, 216, 34]);
      }
      other => panic!("expected v4, got {other:?}"),
    }
  }

  #[test]
  fn an_aaaa_answer_resolves_to_v6() {
    let m = response(
      &[0x20, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
      28,
      NAME,
    );
    assert!(matches!(parse_response(&m, true), DnsAnswer::Resolved(_)));
  }

  #[test]
  fn a_cname_before_the_record_is_followed_by_skipping() {
    // Answer 1: CNAME to "example.org"; answer 2: the A record.
    let mut m = vec![0u8; 12];
    m[2] = 0x80;
    m[4..6].copy_from_slice(&1u16.to_be_bytes());
    m[6..8].copy_from_slice(&2u16.to_be_bytes());
    m.extend_from_slice(NAME);
    m.extend_from_slice(&[0, 1, 0, 1]);
    m.extend_from_slice(NAME);
    let cname = [
      7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'o', b'r', b'g', 0,
    ];
    m.extend_from_slice(&[0, 5]); // CNAME
    m.extend_from_slice(&[0, 1]);
    m.extend_from_slice(&60u32.to_be_bytes());
    m.extend_from_slice(&(cname.len() as u16).to_be_bytes());
    m.extend_from_slice(&cname);
    m.extend_from_slice(NAME);
    m.extend_from_slice(&[0, 1, 0, 1]);
    m.extend_from_slice(&60u32.to_be_bytes());
    m.extend_from_slice(&4u16.to_be_bytes());
    m.extend_from_slice(&[10, 0, 0, 1]);
    match parse_response(&m, false) {
      DnsAnswer::Resolved(IpAddress::Ipv4(v4)) => assert_eq!(v4.octets(), [10, 0, 0, 1]),
      other => panic!("expected v4 past the CNAME, got {other:?}"),
    }
  }

  #[test]
  fn nxdomain_is_no_record_and_failures_are_invalid() {
    let mut m = vec![0u8; 12];
    m[2] = 0x80;
    m[3] = 0x03; // NXDOMAIN
    assert!(matches!(parse_response(&m, false), DnsAnswer::NoRecord));

    let mut m2 = vec![0u8; 12];
    m2[3] = 0x01; // a query, not a response (QR clear)
    assert!(matches!(parse_response(&m2, false), DnsAnswer::Invalid));

    // A truncated answer claiming 4 bytes it does not carry.
    let m3 = response(&[1, 2], 1, NAME);
    assert!(matches!(parse_response(&m3, false), DnsAnswer::Invalid));
  }
}
