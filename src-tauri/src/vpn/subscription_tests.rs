use super::*;

#[test]
fn test_parse_uri_list() {
  let sub_id = "test-sub-1";
  let content = r#"
# Test subscription
vless://11111111-2222-3333-4444-555555555555@de-node1.example.com:443?security=reality&sni=google.com#%F0%9F%87%A9%F0%9F%87%AA%20Germany%20Fast
trojan://secret-password@nl-node.example.com:8443?sni=nl.example.com#Netherlands%20Trojan
hysteria2://auth-pass@us-hy2.example.com:443?sni=us.example.com#US%20Hysteria2
ss://YWVzLTEyOC1nY206cGFzc3dvcmQ=@fi-ss.example.com:8388#Finland%20Shadowsocks
wireguard://10.0.0.2@wg-server.example.com:51820#WG-Main
"#;

  let nodes = parse_subscription_content(sub_id, content).expect("Failed to parse URIs");
  assert_eq!(nodes.len(), 5);

  assert_eq!(nodes[0].protocol, "vless");
  assert_eq!(nodes[0].server, "de-node1.example.com");
  assert_eq!(nodes[0].port, 443);
  assert_eq!(nodes[0].country_code.as_deref(), Some("DE"));

  assert_eq!(nodes[1].protocol, "trojan");
  assert_eq!(nodes[1].server, "nl-node.example.com");
  assert_eq!(nodes[1].port, 8443);
  assert_eq!(nodes[1].country_code.as_deref(), Some("NL"));

  assert_eq!(nodes[2].protocol, "hysteria2");
  assert_eq!(nodes[2].server, "us-hy2.example.com");
  assert_eq!(nodes[2].port, 443);
  assert_eq!(nodes[2].country_code.as_deref(), Some("US"));

  assert_eq!(nodes[3].protocol, "shadowsocks");
  assert_eq!(nodes[3].server, "fi-ss.example.com");
  assert_eq!(nodes[3].port, 8388);
  assert_eq!(nodes[3].country_code.as_deref(), Some("FI"));

  assert_eq!(nodes[4].protocol, "wireguard");
  assert_eq!(nodes[4].server, "wg-server.example.com");
  assert_eq!(nodes[4].port, 51820);
}

#[test]
fn test_parse_base64_subscription() {
  use base64::engine::general_purpose::STANDARD;
  use base64::Engine;

  let sub_id = "test-sub-b64";
  let raw_uris = "vless://11111111-2222-3333-4444-555555555555@sg-server.net:443#Singapore%20Node\ntrojan://pass@tr-server.net:443#Turkey%20Trojan\n";
  let b64 = STANDARD.encode(raw_uris);

  let nodes = parse_subscription_content(sub_id, &b64).expect("Failed to parse Base64 sub");
  assert_eq!(nodes.len(), 2);
  assert_eq!(nodes[0].protocol, "vless");
  assert_eq!(nodes[0].server, "sg-server.net");
  assert_eq!(nodes[0].country_code.as_deref(), Some("SG"));

  assert_eq!(nodes[1].protocol, "trojan");
  assert_eq!(nodes[1].server, "tr-server.net");
  assert_eq!(nodes[1].country_code.as_deref(), Some("TR"));
}

#[test]
fn test_parse_clash_yaml() {
  let sub_id = "test-sub-clash";
  let yaml = r#"
port: 7890
socks-port: 7891
proxies:
  - name: "🇩🇪 Frankfurt VLESS"
    type: vless
    server: frankfurt.vpn.org
    port: 443
    uuid: 11111111-2222-3333-4444-555555555555
    tls: true
    servername: frankfurt.vpn.org
  - name: "🇺🇸 New York Trojan"
    type: trojan
    server: ny.vpn.org
    port: 8443
    password: my-secret-pass
    sni: ny.vpn.org
  - name: "🇯🇵 Tokyo Hysteria2"
    type: hysteria2
    server: tokyo.vpn.org
    port: 443
    password: hy2-password
    sni: tokyo.vpn.org
"#;

  let nodes = parse_subscription_content(sub_id, yaml).expect("Failed to parse Clash YAML");
  assert_eq!(nodes.len(), 3);

  assert_eq!(nodes[0].name, "🇩🇪 Frankfurt VLESS");
  assert_eq!(nodes[0].protocol, "vless");
  assert_eq!(nodes[0].server, "frankfurt.vpn.org");
  assert_eq!(nodes[0].country_code.as_deref(), Some("DE"));

  assert_eq!(nodes[1].name, "🇺🇸 New York Trojan");
  assert_eq!(nodes[1].protocol, "trojan");
  assert_eq!(nodes[1].server, "ny.vpn.org");
  assert_eq!(nodes[1].country_code.as_deref(), Some("US"));

  assert_eq!(nodes[2].name, "🇯🇵 Tokyo Hysteria2");
  assert_eq!(nodes[2].protocol, "hysteria2");
  assert_eq!(nodes[2].server, "tokyo.vpn.org");
  assert_eq!(nodes[2].country_code.as_deref(), Some("JP"));
}

#[test]
fn test_country_code_extractor() {
  assert_eq!(
    extract_country_code("🇩🇪 Munich Server"),
    Some("DE".to_string())
  );
  assert_eq!(
    extract_country_code("🇺🇸 US East VIP"),
    Some("US".to_string())
  );
  assert_eq!(
    extract_country_code("🇳🇱 Amsterdam Fast"),
    Some("NL".to_string())
  );
  assert_eq!(
    extract_country_code("Россия Москва 1"),
    Some("RU".to_string())
  );
  assert_eq!(
    extract_country_code("Japan Tokyo Direct"),
    Some("JP".to_string())
  );
  assert_eq!(extract_country_code("Random Server without country"), None);
}
