use super::*;
use tempfile::tempdir;

#[tokio::test]
async fn test_proxy_chain_crud() {
  let tmp = tempdir().unwrap();
  let mgr = ProxyChainManager::with_dir(tmp.path());

  assert!(mgr.list_chains().is_empty());

  // Add chain
  let chain1 = mgr
    .add_chain(
      "Fast Chain: UK -> NL".to_string(),
      "proxy-uk".to_string(),
      "proxy-nl".to_string(),
    )
    .unwrap();

  assert_eq!(chain1.name, "Fast Chain: UK -> NL");
  assert_eq!(chain1.hop1_proxy_id, "proxy-uk");
  assert_eq!(chain1.hop2_proxy_id, "proxy-nl");

  let list = mgr.list_chains();
  assert_eq!(list.len(), 1);
  assert_eq!(list[0].id, chain1.id);

  // Rejection when same hop
  let err = mgr.add_chain(
    "Loop Chain".to_string(),
    "proxy-1".to_string(),
    "proxy-1".to_string(),
  );
  assert!(err.is_err());

  // Update chain
  let updated = mgr
    .update_chain(
      &chain1.id,
      Some("Updated Chain".to_string()),
      None,
      Some("proxy-de".to_string()),
    )
    .unwrap();
  assert_eq!(updated.name, "Updated Chain");
  assert_eq!(updated.hop2_proxy_id, "proxy-de");

  // Delete chain
  mgr.delete_chain(&chain1.id).unwrap();
  assert!(mgr.list_chains().is_empty());
}
