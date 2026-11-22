//! Local offline backup: every profile (configuration and browser data) and
//! optionally the proxy list, in one portable `.neobackup` archive.
//!
//! A backup is a zip: a manifest, an optional `proxies.json`, and each
//! profile's directory as it sits under `profiles/`. With a password the zip
//! is sealed into an AES-256-GCM envelope instead — a wrong password fails at
//! the first chunk, long before anything is written into `profiles/`.
//!
//! What deliberately does not travel:
//! - cloud-managed proxies, which the account re-fetches on sign-in;
//! - live process state (`process_id`, the last launch), which describes the
//!   machine that made the backup;
//! - the caches, which Chromium rebuilds and which are most of the bytes.
//!
//! A restore is idempotent by id: a profile already on this machine is skipped
//! unless the caller passes `overwrite`, which replaces its directory whole —
//! through a staging folder, so a failed extraction never costs the original.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

/// Bumped when the archive layout changes in a way an older build cannot read.
const FORMAT_VERSION: u32 = 1;
const MANIFEST_ENTRY: &str = "backup_manifest.json";
const PROXIES_ENTRY: &str = "proxies.json";
const PROFILES_PREFIX: &str = "profiles/";
/// The extension the save dialog offers; a `target_path` without one gets it.
const ARCHIVE_EXTENSION: &str = "neobackup";

/// The sealed envelope's magic. A `.neobackup` that starts with these bytes is
/// encrypted; anything else is read as a plain zip.
const ENVELOPE_MAGIC: &[u8; 8] = b"NEOBK100";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
/// One GCM chunk. Sealing a whole archive in one shot would need the plaintext
/// in memory; chunking keeps the peak at `CHUNK_SIZE` and lets a wrong
/// password fail on the first megabyte rather than after reading everything.
const CHUNK_SIZE: usize = 1024 * 1024;

/// What one archive says about itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
  pub format_version: u32,
  pub created_at: u64,
  pub app_version: String,
  pub profile_count: usize,
  pub includes_proxies: bool,
  pub encrypted: bool,
}

/// What a finished backup reports.
#[derive(Debug, Clone, Serialize)]
pub struct BackupResult {
  pub path: String,
  pub profile_count: usize,
  pub proxy_count: usize,
  pub encrypted: bool,
  pub size_bytes: u64,
}

/// What a restore did.
#[derive(Debug, Clone, Serialize)]
pub struct RestoreResult {
  pub profiles_restored: usize,
  pub profiles_skipped: usize,
  pub proxies_restored: usize,
}

fn err(context: &str, detail: impl std::fmt::Display) -> String {
  crate::backend_error_with_detail("BACKUP_WRITE_FAILED", format!("{context}: {detail}"))
}

fn restore_err(detail: impl std::fmt::Display) -> String {
  crate::backend_error_with_detail("BACKUP_RESTORE_FAILED", detail.to_string())
}

/// The archive work runs on the blocking pool: it reads or writes whole
/// profile directories, and the window would freeze for as long.
async fn blocking<T: Send + 'static>(
  work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
  crate::profile::manager::run_blocking(work)
    .await
    .map_err(|e| crate::backend_error_with_detail("INTERNAL_ERROR", e))?
}

/// The on-disk path for an export target: the user's choice with the archive
/// extension appended when they dropped it.
fn archive_path(target: &str) -> PathBuf {
  let path = PathBuf::from(target);
  if path
    .extension()
    .is_none_or(|ext| ext.to_string_lossy().to_lowercase() != ARCHIVE_EXTENSION)
  {
    let mut name = path
      .file_name()
      .map(|name| name.to_os_string())
      .unwrap_or_default();
    name.push(format!(".{ARCHIVE_EXTENSION}"));
    path.with_file_name(name)
  } else {
    path
  }
}

/// Every file under `dir`, relative to it, skipping the caches Chromium
/// rebuilds and the launcher's own per-launch documents. Shared with the
/// per-profile export so an archive and a backup leave out the same bytes.
fn collect_files(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
  fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| err("could not read a profile directory", e))?;
    for entry in entries.flatten() {
      let path = entry.path();
      let Ok(relative) = path.strip_prefix(root) else {
        continue;
      };
      let relative = relative.to_string_lossy().replace('\\', "/");
      if crate::profile::portable::is_excluded(&relative) {
        continue;
      }
      let file_type = entry
        .file_type()
        .map_err(|e| err("could not stat a file", e))?;
      if file_type.is_symlink() {
        // A symlink into an archive is either useless on the other machine or
        // a way out of the extraction directory. Neither travels.
        continue;
      }
      if file_type.is_dir() {
        walk(root, &path, out)?;
      } else if file_type.is_file() {
        out.push((relative, path));
      }
    }
    Ok(())
  }

  let mut files = Vec::new();
  walk(dir, dir, &mut files)?;
  files.sort_by(|a, b| a.0.cmp(&b.0));
  Ok(files)
}

/// Sum every file's size under `path`, tolerating a directory that vanished
/// mid-walk: the count feeds a "freed N MB" toast, not a ledger.
fn dir_size(path: &Path) -> u64 {
  let Ok(entries) = fs::read_dir(path) else {
    return 0;
  };
  let mut total = 0;
  for entry in entries.flatten() {
    let Ok(file_type) = entry.file_type() else {
      continue;
    };
    let path = entry.path();
    if file_type.is_dir() {
      total += dir_size(&path);
    } else if let Ok(metadata) = entry.metadata() {
      total += metadata.len();
    }
  }
  total
}

// ---------------------------------------------------------------------------
// The password envelope
// ---------------------------------------------------------------------------

/// The chunk nonce: the envelope's random base nonce with the chunk index in
/// its last four bytes. Unique per chunk for any archive under 32 PiB.
fn chunk_nonce(base: &[u8; NONCE_LEN], index: u32) -> [u8; NONCE_LEN] {
  let mut nonce = *base;
  let counter = (index as u64).to_be_bytes();
  for (byte, counter_byte) in nonce[NONCE_LEN - 4..].iter_mut().zip(counter) {
    *byte ^= counter_byte;
  }
  nonce
}

/// The chunk's AAD: the magic and the chunk index, so chunks cannot be
/// reordered or replayed between archives; the last chunk adds a `final` tag
/// so the tail cannot be truncated away.
fn chunk_aad(index: u32, is_final: bool) -> Vec<u8> {
  let mut aad = ENVELOPE_MAGIC.to_vec();
  aad.extend_from_slice(&index.to_be_bytes());
  if is_final {
    aad.extend_from_slice(b"final");
  }
  aad
}

/// Seal `plain_path` into `sealed_path` under `password`.
fn seal_file(plain_path: &Path, sealed_path: &Path, password: &str) -> Result<(), String> {
  let salt: [u8; SALT_LEN] = rand::rng().random();
  let base_nonce: [u8; NONCE_LEN] = rand::rng().random();
  let key = crate::sync::encryption::derive_vault_key(password.as_bytes(), &salt)
    .map_err(|e| err("key derivation", e))?;
  let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(key));

  let mut plain = fs::File::open(plain_path).map_err(|e| err("could not reopen the archive", e))?;
  let mut sealed =
    fs::File::create(sealed_path).map_err(|e| err("could not create the encrypted archive", e))?;

  let mut header = Vec::with_capacity(ENVELOPE_MAGIC.len() + SALT_LEN + NONCE_LEN);
  header.extend_from_slice(ENVELOPE_MAGIC);
  header.extend_from_slice(&salt);
  header.extend_from_slice(&base_nonce);
  sealed
    .write_all(&header)
    .map_err(|e| err("could not write the envelope header", e))?;

  let mut chunk = vec![0u8; CHUNK_SIZE];
  let mut index = 0u32;
  loop {
    let mut read = 0;
    while read < chunk.len() {
      match plain.read(&mut chunk[read..]) {
        Ok(0) => break,
        Ok(n) => read += n,
        Err(e) => return Err(err("could not read the archive", e)),
      }
    }
    if read == 0 && index > 0 {
      // The previous chunk was the last one.
      break;
    }
    let is_final = read < chunk.len();
    let payload = Payload {
      msg: &chunk[..read],
      aad: &chunk_aad(index, is_final),
    };
    let sealed_chunk = cipher
      .encrypt(&Nonce::from(chunk_nonce(&base_nonce, index)), payload)
      .map_err(|_| err("encryption failed", "the cipher rejected a chunk"))?;
    sealed
      .write_all(&sealed_chunk)
      .map_err(|e| err("could not write the encrypted archive", e))?;
    if is_final {
      break;
    }
    index = index
      .checked_add(1)
      .ok_or_else(|| err("archive too large", "chunk counter exhausted"))?;
  }

  sealed
    .flush()
    .map_err(|e| err("could not finish the encrypted archive", e))?;
  Ok(())
}

/// Whether the file behind `reader` has nothing more to give. One chunk was
/// just read; a full-size chunk may still be the last one.
fn reader_at_eof(reader: &mut impl BufRead) -> Result<bool, String> {
  match reader.fill_buf() {
    Ok(buffer) => Ok(buffer.is_empty()),
    Err(e) => Err(restore_err(format!("could not read the archive: {e}"))),
  }
}

/// Open a sealed envelope into `plain_path` under `password`.
///
/// A wrong password (or a damaged archive) fails here, before the caller ever
/// touches `profiles/` — the decryption target is a scratch file either way.
fn open_sealed_file(sealed_path: &Path, plain_path: &Path, password: &str) -> Result<(), String> {
  let mut sealed = std::io::BufReader::new(
    fs::File::open(sealed_path)
      .map_err(|e| restore_err(format!("could not open the archive: {e}")))?,
  );

  let invalid = || crate::backend_error("BACKUP_ARCHIVE_INVALID");
  let mut magic = [0u8; ENVELOPE_MAGIC.len()];
  sealed.read_exact(&mut magic).map_err(|_| invalid())?;
  if magic != *ENVELOPE_MAGIC {
    return Err(invalid());
  }
  let mut salt = [0u8; SALT_LEN];
  sealed.read_exact(&mut salt).map_err(|_| invalid())?;
  let mut base_nonce = [0u8; NONCE_LEN];
  sealed.read_exact(&mut base_nonce).map_err(|_| invalid())?;

  let derived = crate::sync::encryption::derive_vault_key(password.as_bytes(), &salt)
    .map_err(|e| restore_err(format!("key derivation failed: {e}")))?;
  let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(derived));

  let mut plain = fs::File::create(plain_path)
    .map_err(|e| restore_err(format!("could not prepare the extraction: {e}")))?;

  let mut index = 0u32;
  loop {
    // One ciphertext chunk plus its tag; the last one may be shorter.
    let limit = CHUNK_SIZE + TAG_LEN;
    let mut buffer = vec![0u8; limit];
    let mut read = 0;
    while read < limit {
      match sealed.read(&mut buffer[read..]) {
        Ok(0) => break,
        Ok(n) => read += n,
        Err(e) => return Err(restore_err(format!("could not read the archive: {e}"))),
      }
    }
    if read == 0 {
      // Clean EOF before a final chunk: the archive was truncated.
      return Err(invalid());
    }
    let is_last = if read < limit {
      true
    } else {
      reader_at_eof(&mut sealed)?
    };
    if read < TAG_LEN {
      return Err(invalid());
    }

    let payload = Payload {
      msg: &buffer[..read],
      aad: &chunk_aad(index, is_last),
    };
    let plain_chunk = cipher
      .decrypt(&Nonce::from(chunk_nonce(&base_nonce, index)), payload)
      .map_err(|_| {
        // The first chunk failing is a wrong password. A later chunk failing
        // is a wrong password too (Argon2 keys every chunk identically), so
        // name that, unless the failure is in a middle chunk of an archive
        // that opened fine — then it is damage, not a typo.
        if index == 0 || is_last {
          crate::backend_error("INCORRECT_PASSWORD")
        } else {
          invalid()
        }
      })?;
    plain
      .write_all(&plain_chunk)
      .map_err(|e| restore_err(format!("could not write the extraction: {e}")))?;
    if is_last {
      return Ok(());
    }
    index = index.checked_add(1).ok_or_else(invalid)?;
  }
}

// ---------------------------------------------------------------------------
// Backup
// ---------------------------------------------------------------------------

/// Write the whole backup zip for `profiles` (and optionally the proxy list)
/// to `target`, then seal it when `password` says so. The zip is built in a
/// `.part` scratch file and only renamed/sealed into place at the end, so a
/// failed backup never leaves a half-written `.neobackup` behind.
fn backup_blocking(
  profile_ids: Option<Vec<String>>,
  include_proxies: bool,
  target: &Path,
  password: Option<&str>,
) -> Result<BackupResult, String> {
  let manager = crate::profile::ProfileManager::instance();

  let profiles = match profile_ids {
    Some(ids) if !ids.is_empty() => {
      let mut profiles = Vec::with_capacity(ids.len());
      for id in &ids {
        let profile_uuid =
          uuid::Uuid::parse_str(id).map_err(|_| crate::backend_error("INVALID_PROFILE_ID"))?;
        profiles.push(
          manager
            .lookup_profile(&profile_uuid)
            .map_err(|e| crate::wrap_backend_error(e, "Could not read the profiles"))?
            .ok_or_else(|| crate::backend_error("PROFILE_NOT_FOUND"))?,
        );
      }
      profiles
    }
    _ => manager
      .list_profiles()
      .map_err(|e| crate::wrap_backend_error(e, "Could not read the profiles"))?,
  };
  if profiles.is_empty() {
    return Err(crate::backend_error("BACKUP_EMPTY"));
  }
  // A backup reads whole profile directories; a browser writing to one at the
  // same time archives half-written databases.
  for profile in &profiles {
    if profile
      .process_id
      .is_some_and(crate::proxy_storage::is_process_running)
    {
      return Err(crate::backend_error_with_detail(
        "PROFILE_RUNNING",
        &profile.name,
      ));
    }
  }

  let proxies = if include_proxies {
    crate::proxy_manager::PROXY_MANAGER
      .get_stored_proxies()
      .into_iter()
      .filter(|proxy| !proxy.is_cloud_managed && !proxy.is_cloud_derived)
      .collect::<Vec<_>>()
  } else {
    Vec::new()
  };

  let manifest = BackupManifest {
    format_version: FORMAT_VERSION,
    created_at: crate::proxy_manager::now_secs(),
    app_version: format!("NullFrame {}", env!("CARGO_PKG_VERSION")),
    profile_count: profiles.len(),
    includes_proxies: include_proxies,
    encrypted: password.is_some(),
  };

  fs::create_dir_all(target.parent().unwrap_or(Path::new(".")))
    .map_err(|e| err("could not create the destination folder", e))?;
  let scratch = target.with_extension("part");

  let outcome = write_zip(
    &scratch,
    &profiles,
    &proxies,
    &manifest,
    manager.get_profiles_dir().as_path(),
  );
  if let Err(error) = outcome {
    let _ = fs::remove_file(&scratch);
    return Err(error);
  }

  // The zip alone is the archive; the sealed envelope is staged beside it and
  // moved into place only once complete.
  match password {
    Some(password) => {
      let staging = target.with_extension("sealing");
      let outcome = seal_file(&scratch, &staging, password);
      let _ = fs::remove_file(&scratch);
      if let Err(error) = outcome {
        let _ = fs::remove_file(&staging);
        return Err(error);
      }
      replace_file(&staging, target)?;
    }
    None => replace_file(&scratch, target)?,
  }

  let size_bytes = fs::metadata(target).map(|m| m.len()).unwrap_or(0);
  Ok(BackupResult {
    path: target.to_string_lossy().into_owned(),
    profile_count: profiles.len(),
    proxy_count: proxies.len(),
    encrypted: password.is_some(),
    size_bytes,
  })
}

/// Move `staging` onto `target`, replacing whatever was there. Windows will
/// not rename over an existing file, so the old one goes first; that window
/// is one `rename` wide, and both files are ours.
fn replace_file(staging: &Path, target: &Path) -> Result<(), String> {
  if target.exists() {
    fs::remove_file(target).map_err(|e| err("could not replace the archive", e))?;
  }
  fs::rename(staging, target).map_err(|e| err("could not finish the archive", e))
}

/// Write the backup zip: manifest, optional proxies, every profile directory.
fn write_zip(
  scratch: &Path,
  profiles: &[crate::profile::BrowserProfile],
  proxies: &[crate::proxy_manager::StoredProxy],
  manifest: &BackupManifest,
  profiles_dir: &Path,
) -> Result<(), String> {
  let file = fs::File::create(scratch).map_err(|e| err("could not create the archive", e))?;
  let mut writer = zip::ZipWriter::new(file);
  let options: zip::write::SimpleFileOptions =
    zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

  let start = |writer: &mut zip::ZipWriter<fs::File>, name: &str| -> Result<(), String> {
    writer.start_file(name, options).map_err(|e| err(name, e))
  };

  start(&mut writer, MANIFEST_ENTRY)?;
  writer
    .write_all(
      serde_json::to_string_pretty(manifest)
        .map_err(|e| err("manifest", e))?
        .as_bytes(),
    )
    .map_err(|e| err("manifest", e))?;

  if manifest.includes_proxies {
    let body = serde_json::json!({ "proxies": proxies });
    start(&mut writer, PROXIES_ENTRY)?;
    writer
      .write_all(
        serde_json::to_string_pretty(&body)
          .map_err(|e| err("proxies", e))?
          .as_bytes(),
      )
      .map_err(|e| err("proxies", e))?;
  }

  for profile in profiles {
    let uuid_dir = profiles_dir.join(profile.id.to_string());
    let prefix = format!("{PROFILES_PREFIX}{}", profile.id);
    // The metadata entry is written explicitly rather than collected, so a
    // profile whose data directory is still empty backs up as configuration.
    let metadata = serde_json::to_string_pretty(profile)
      .map_err(|e| err("a profile could not be serialized", e))?;
    start(&mut writer, &format!("{prefix}/metadata.json"))?;
    writer
      .write_all(metadata.as_bytes())
      .map_err(|e| err("metadata", e))?;

    #[cfg(target_os = "windows")]
    if profile.password_protected {
      // A profile created before it had a key file holds its key only in a
      // form this Windows account can open; the backup must carry the file
      // for the restore to read the data again on this machine.
      let data_dir = uuid_dir.join("profile");
      if let Err(e) = crate::profile_import::keyring::write_key_file_from_local_state(&data_dir) {
        log::warn!(
          "Backup carries no key file profile={} err=\"{e}\"",
          profile.id
        );
      }
    }

    for (relative, absolute) in collect_files(&uuid_dir)? {
      // The metadata entry is written explicitly above; the walk sees it too,
      // and a second copy would make the archive unreadable.
      if relative == "metadata.json" {
        continue;
      }
      let data = match fs::read(&absolute) {
        Ok(data) => data,
        // A browser file can vanish between the walk and the read; that is
        // not a reason to fail a backup of everything else.
        Err(e) => {
          log::warn!(
            "Backup skipped a file path=\"{}\" err=\"{e}\"",
            absolute.display()
          );
          continue;
        }
      };
      // The walk's paths are relative to the profile's own directory; inside
      // the archive they live under the profile's uuid.
      start(
        &mut writer,
        &format!("{PROFILES_PREFIX}{}/{}", profile.id, relative),
      )?;
      writer.write_all(&data).map_err(|e| err(&relative, e))?;
    }
  }

  writer
    .finish()
    .map_err(|e| err("could not finish the archive", e))?;
  Ok(())
}

/// Back up profiles (all, or the listed ids) and optionally the proxy list to
/// `target_path` as a `.neobackup` archive, sealed under `password` when one
/// is given.
#[tauri::command]
pub async fn create_backup(
  profile_ids: Option<Vec<String>>,
  include_proxies: Option<bool>,
  target_path: String,
  password: Option<String>,
) -> Result<BackupResult, String> {
  let include_proxies = include_proxies.unwrap_or(true);
  blocking(move || {
    let target = archive_path(&target_path);
    backup_blocking(profile_ids, include_proxies, &target, password.as_deref())
  })
  .await
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

fn open_archive(path: &Path) -> Result<zip::ZipArchive<fs::File>, String> {
  let file = fs::File::open(path).map_err(|e| restore_err(format!("could not open it: {e}")))?;
  zip::ZipArchive::new(file).map_err(|e| restore_err(format!("it is not a readable archive: {e}")))
}

fn read_entry(archive: &mut zip::ZipArchive<fs::File>, name: &str) -> Result<String, String> {
  let mut entry = archive
    .by_name(name)
    .map_err(|_| crate::backend_error("BACKUP_ARCHIVE_INVALID"))?;
  let mut body = String::new();
  entry
    .read_to_string(&mut body)
    .map_err(|e| restore_err(format!("{name} could not be read: {e}")))?;
  Ok(body)
}

/// Whether an entry name stays inside its extraction root. An archive is
/// untrusted input, and `..` in a name is how an extraction writes over a
/// user's files.
fn safe_entry_name(name: &str) -> Option<String> {
  let mut cleaned = PathBuf::new();
  for part in Path::new(name).components() {
    match part {
      std::path::Component::Normal(part) => cleaned.push(part),
      _ => return None,
    }
  }
  let text = cleaned.to_string_lossy().replace('\\', "/");
  (!text.is_empty()).then_some(text)
}

/// Copy the archive entry `index` into `destination`.
fn extract_entry(
  archive: &mut zip::ZipArchive<fs::File>,
  index: usize,
  destination: &Path,
) -> Result<(), String> {
  if let Some(parent) = destination.parent() {
    fs::create_dir_all(parent)
      .map_err(|e| restore_err(format!("could not create {}: {e}", parent.display())))?;
  }
  let mut entry = archive
    .by_index(index)
    .map_err(|e| restore_err(format!("could not read entry {index}: {e}")))?;
  let mut file = fs::File::create(destination)
    .map_err(|e| restore_err(format!("could not write {}: {e}", destination.display())))?;
  std::io::copy(&mut entry, &mut file)
    .map_err(|e| restore_err(format!("could not write {}: {e}", destination.display())))?;
  Ok(())
}

/// The raw entry name, relative to `prefix`, judged for extraction.
///
/// Judging the RAW name is the point: this zip's `enclosed_name` resolves
/// `..` away instead of refusing it, so a traversal entry survives as an
/// innocent name — and it is exactly the entry a restore should name and
/// refuse. `safe_entry_name` then guarantees no component climbs, and the
/// enclosed form is what the destination is built from.
fn entry_under_prefix(
  entry: &zip::read::ZipFile<'_, fs::File>,
  prefix: &str,
) -> Result<Option<String>, String> {
  let unsafe_archive = || crate::backend_error("BACKUP_UNSAFE_ARCHIVE");
  let raw = entry.name().map_err(|_| unsafe_archive())?;
  let Some(raw_relative) = raw.strip_prefix(prefix) else {
    return Ok(None);
  };
  // `safe_entry_name` refuses any name whose components would climb out of
  // the profile's own directory; the value itself is not needed.
  if safe_entry_name(raw_relative).is_none() {
    return Err(unsafe_archive());
  }
  let enclosed = entry
    .enclosed_name()
    .ok_or_else(unsafe_archive)?
    .to_string_lossy()
    .replace('\\', "/");
  // A `..` the enclosed form resolved out of the prefix itself is the same
  // traversal wearing a normalized name.
  let Some(enclosed_relative) = enclosed.strip_prefix(prefix) else {
    return Err(unsafe_archive());
  };
  Ok((!enclosed_relative.is_empty()).then(|| enclosed_relative.to_string()))
}

/// Extract every `profiles/<uuid>/` entry into `root` — everything but the
/// metadata, which `save_profile` writes after the data is in place.
fn extract_profile_data(
  archive: &mut zip::ZipArchive<fs::File>,
  root: &Path,
  uuid: &str,
) -> Result<(), String> {
  fs::create_dir_all(root)
    .map_err(|e| restore_err(format!("could not create the profile directory: {e}")))?;
  let canonical = root
    .canonicalize()
    .map_err(|e| restore_err(format!("could not resolve the profile directory: {e}")))?;
  let prefix = format!("{PROFILES_PREFIX}{uuid}/");

  for index in 0..archive.len() {
    let relative = {
      let entry = archive
        .by_index(index)
        .map_err(|e| restore_err(format!("could not read entry {index}: {e}")))?;
      if entry.is_dir() {
        continue;
      }
      match entry_under_prefix(&entry, &prefix)? {
        Some(relative) if relative != "metadata.json" => relative,
        _ => continue,
      }
    };
    // `relative` comes from the enclosed name: no component can climb, and
    // the `starts_with` below is the belt over those braces.
    let destination = canonical.join(&relative);
    if !destination.starts_with(&canonical) {
      return Err(crate::backend_error("BACKUP_UNSAFE_ARCHIVE"));
    }
    extract_entry(archive, index, &destination)?;
  }
  Ok(())
}

/// The profiles an archive carries, by uuid, from its entry names.
fn archived_profile_uuids(archive: &mut zip::ZipArchive<fs::File>) -> Result<Vec<String>, String> {
  let mut uuids: Vec<String> = Vec::new();
  for index in 0..archive.len() {
    let entry = archive
      .by_index(index)
      .map_err(|e| restore_err(format!("could not read entry {index}: {e}")))?;
    let raw = entry
      .name()
      .map_err(|_| crate::backend_error("BACKUP_UNSAFE_ARCHIVE"))?;
    let Some(rest) = raw.strip_prefix(PROFILES_PREFIX) else {
      continue;
    };
    let Some(uuid) = rest.split('/').next() else {
      continue;
    };
    if uuid.parse::<uuid::Uuid>().is_ok() && !uuids.iter().any(|seen| seen == uuid) {
      uuids.push(uuid.to_string());
    }
  }
  if uuids.is_empty() {
    return Err(crate::backend_error("BACKUP_ARCHIVE_INVALID"));
  }
  Ok(uuids)
}

/// Restore the archive's proxies into `proxies_dir` and the manager's memory,
/// keeping their ids so profiles that reference them keep working.
fn restore_proxies(body: &str, overwrite: bool, result: &mut RestoreResult) {
  let Ok(wrapped) = serde_json::from_str::<serde_json::Value>(body) else {
    return;
  };
  let Some(proxies) = wrapped["proxies"].as_array() else {
    return;
  };
  let proxies_dir = crate::app_dirs::proxies_dir();
  if fs::create_dir_all(&proxies_dir).is_err() {
    return;
  }
  for value in proxies {
    let Ok(proxy) = serde_json::from_value::<crate::proxy_manager::StoredProxy>(value.clone())
    else {
      continue;
    };
    // Cloud proxies are the account's, fetched fresh on sign-in; a stale copy
    // from another machine would fight the live one.
    if proxy.is_cloud_managed || proxy.is_cloud_derived {
      continue;
    }
    if !crate::app_dirs::is_plain_path_part(&proxy.id) {
      continue;
    }
    let exists = crate::proxy_manager::PROXY_MANAGER
      .get_stored_proxies()
      .iter()
      .any(|existing| existing.id == proxy.id);
    if exists && !overwrite {
      continue;
    }
    let file = proxies_dir.join(format!("{}.json", proxy.id));
    let Ok(content) = serde_json::to_string_pretty(&proxy) else {
      continue;
    };
    if crate::app_dirs::write_owner_only(&file, content.as_bytes()).is_ok() {
      crate::proxy_manager::PROXY_MANAGER.upsert_stored_proxy(proxy);
      result.proxies_restored += 1;
    }
  }
}

/// Restore profiles and proxies from a `.neobackup` archive.
fn restore_blocking(
  archive_path: &Path,
  password: Option<&str>,
  overwrite: bool,
) -> Result<RestoreResult, String> {
  // An encrypted archive is a sealed envelope, not a zip. Decrypt it to a
  // scratch file first: a wrong password fails here, before anything under
  // profiles/ is touched.
  let mut plain_path: Option<PathBuf> = None;
  let mut magic = [0u8; ENVELOPE_MAGIC.len()];
  {
    let mut file = fs::File::open(archive_path)
      .map_err(|e| restore_err(format!("could not open the archive: {e}")))?;
    file
      .read_exact(&mut magic)
      .map_err(|_| crate::backend_error("BACKUP_ARCHIVE_INVALID"))?;
  }
  let source: PathBuf = if magic == *ENVELOPE_MAGIC {
    let Some(password) = password else {
      return Err(crate::backend_error("INCORRECT_PASSWORD"));
    };
    let scratch = archive_path.with_extension("restore.part");
    if let Err(error) = open_sealed_file(archive_path, &scratch, password) {
      let _ = fs::remove_file(&scratch);
      return Err(error);
    }
    plain_path = Some(scratch.clone());
    scratch
  } else {
    archive_path.to_path_buf()
  };

  let outcome = restore_from_zip(&source, overwrite);
  if let Some(scratch) = plain_path {
    let _ = fs::remove_file(&scratch);
  }
  outcome
}

fn restore_from_zip(source: &Path, overwrite: bool) -> Result<RestoreResult, String> {
  let mut archive = open_archive(source)?;

  let manifest: BackupManifest = serde_json::from_str(&read_entry(&mut archive, MANIFEST_ENTRY)?)
    .map_err(|_| crate::backend_error("BACKUP_ARCHIVE_INVALID"))?;
  if manifest.format_version > FORMAT_VERSION {
    return Err(crate::backend_error_with_detail(
      "BACKUP_TOO_NEW",
      manifest.format_version.to_string(),
    ));
  }

  let manager = crate::profile::ProfileManager::instance();
  let profiles_dir = manager.get_profiles_dir();
  let uuids = archived_profile_uuids(&mut archive)?;

  let mut result = RestoreResult {
    profiles_restored: 0,
    profiles_skipped: 0,
    proxies_restored: 0,
  };

  for uuid in &uuids {
    let stored: crate::profile::BrowserProfile = serde_json::from_str(&read_entry(
      &mut archive,
      &format!("{PROFILES_PREFIX}{uuid}/metadata.json"),
    )?)
    .map_err(|e| restore_err(format!("a profile's metadata is unusable: {e}")))?;

    let existing = manager
      .lookup_profile(&stored.id)
      .map_err(|e| restore_err(format!("could not read the profiles: {e}")))?;
    if existing.is_some() && !overwrite {
      result.profiles_skipped += 1;
      continue;
    }
    let final_dir = profiles_dir.join(stored.id.to_string());
    if let Some(existing) = existing {
      // Replacing a live browser's directory under it corrupts both; the
      // running check names the profile that is up, not the backup's note.
      if existing
        .process_id
        .is_some_and(crate::proxy_storage::is_process_running)
      {
        return Err(crate::backend_error_with_detail(
          "PROFILE_RUNNING",
          &existing.name,
        ));
      }
      // Extract into a staging folder first: a failed extraction must not
      // have already taken the original down.
      let staging = profiles_dir.join(format!("{}.restoring", stored.id));
      let _ = fs::remove_dir_all(&staging);
      extract_profile_data(&mut archive, &staging, uuid)?;
      fs::remove_dir_all(&final_dir)
        .map_err(|e| restore_err(format!("could not replace the profile: {e}")))?;
      fs::rename(&staging, &final_dir)
        .map_err(|e| restore_err(format!("could not place the profile: {e}")))?;
    } else {
      extract_profile_data(&mut archive, &final_dir, uuid)?;
    }

    // The restore is idempotent by id; the machine-local state is not.
    let profile = crate::profile::BrowserProfile {
      process_id: None,
      last_launch: None,
      last_sync: None,
      ..stored
    };
    manager
      .save_profile(&profile)
      .map_err(|e| restore_err(format!("could not save the profile: {e}")))?;
    result.profiles_restored += 1;
  }

  if manifest.includes_proxies {
    // An archive from a build before the proxy list travelled simply has no
    // such entry; a restore of the profiles alone still stands.
    if let Ok(body) = read_entry(&mut archive, PROXIES_ENTRY) {
      restore_proxies(&body, overwrite, &mut result);
    }
  }

  if result.profiles_restored > 0 {
    let _ = crate::events::emit("profiles-changed", serde_json::json!({}));
  }
  if result.proxies_restored > 0 {
    let _ = crate::events::emit_empty("proxies-changed");
  }
  Ok(result)
}

/// Restore profiles and proxies from a `.neobackup` archive (plain or sealed)
/// into `profiles/`, skipping profiles this machine already has unless
/// `overwrite` replaces them.
#[tauri::command]
pub async fn restore_backup(
  archive_path: String,
  password: Option<String>,
  overwrite: Option<bool>,
) -> Result<RestoreResult, String> {
  blocking(move || {
    restore_blocking(
      Path::new(&archive_path),
      password.as_deref(),
      overwrite.unwrap_or(false),
    )
  })
  .await
}

// ---------------------------------------------------------------------------
// Quick cache clean
// ---------------------------------------------------------------------------

/// The heavy caches Chromium rebuilds, relative to a profile's data directory.
/// `Default/` holds the per-user set; `ShaderCache` sits at the user-data root.
const CACHE_PATHS: [&str; 8] = [
  "Default/Cache",
  "Default/Code Cache",
  "Default/GPUCache",
  "Default/Service Worker/CacheStorage",
  "Default/DawnCache",
  "Default/DawnGraphiteCache",
  "Default/GrShaderCache",
  "ShaderCache",
];

/// The cookie store files, for a purge that does throw away the sessions.
const COOKIE_FILES: [&str; 6] = [
  "Default/Network/Cookies",
  "Default/Network/Cookies-journal",
  "Default/Network/Cookies-wal",
  "Default/Cookies",
  "Default/Cookies-journal",
  "Default/Cookies-wal",
];

/// Delete a profile's heavy Chromium caches (and its cookie store when
/// `purge_cookies` asks), keeping every login. Returns the bytes freed.
fn clean_blocking(profile_id: &str, purge_cookies: bool) -> Result<u64, String> {
  let manager = crate::profile::ProfileManager::instance();
  let profile_uuid =
    uuid::Uuid::parse_str(profile_id).map_err(|_| crate::backend_error("INVALID_PROFILE_ID"))?;
  let profile = manager
    .lookup_profile(&profile_uuid)
    .map_err(|e| crate::wrap_backend_error(e, "Could not read the profile"))?
    .ok_or_else(|| crate::backend_error("PROFILE_NOT_FOUND"))?;
  if profile
    .process_id
    .is_some_and(crate::proxy_storage::is_process_running)
  {
    return Err(crate::backend_error_with_detail(
      "PROFILE_RUNNING",
      &profile.name,
    ));
  }

  let data_dir = manager
    .get_profiles_dir()
    .join(profile.id.to_string())
    .join("profile");
  if !data_dir.is_dir() {
    return Ok(0);
  }

  let mut freed = 0u64;
  for relative in CACHE_PATHS {
    let path = data_dir.join(relative);
    if !path.exists() {
      continue;
    }
    let size = dir_size(&path);
    match fs::remove_dir_all(&path) {
      Ok(()) => freed += size,
      // A file held open by a scanner or an indexer is not a reason to report
      // the clean as a failure: what was removed stays removed.
      Err(e) => log::warn!(
        "Cache clean skipped a directory path=\"{}\" err=\"{e}\"",
        path.display()
      ),
    }
  }

  if purge_cookies {
    for relative in COOKIE_FILES {
      let path = data_dir.join(relative);
      if !path.is_file() {
        continue;
      }
      let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
      match fs::remove_file(&path) {
        Ok(()) => freed += size,
        Err(e) => log::warn!(
          "Cache clean skipped a cookie file path=\"{}\" err=\"{e}\"",
          path.display()
        ),
      }
    }
  }

  Ok(freed)
}

/// Free a profile's disk: the caches Chromium rebuilds, and the cookie store
/// only when `purge_cookies` says so. Returns the bytes freed.
#[tauri::command]
pub async fn clean_profile_cache(
  profile_id: String,
  purge_cookies: Option<bool>,
) -> Result<u64, String> {
  let purge_cookies = purge_cookies.unwrap_or(false);
  blocking(move || clean_blocking(&profile_id, purge_cookies)).await
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  fn sample_profile(name: &str) -> crate::profile::BrowserProfile {
    crate::profile::BrowserProfile {
      id: uuid::Uuid::new_v4(),
      name: name.to_string(),
      browser: "wayfern".to_string(),
      version: "152.0.7977.64".to_string(),
      created_at: Some(crate::proxy_manager::now_secs()),
      ..crate::profile::BrowserProfile::default()
    }
  }

  fn seed_profile_dir(root: &Path, profile: &crate::profile::BrowserProfile) {
    let uuid_dir = root.join(profile.id.to_string());
    let data = uuid_dir.join("profile");
    fs::create_dir_all(data.join("Default/Network")).unwrap();
    fs::write(
      uuid_dir.join("metadata.json"),
      serde_json::to_string_pretty(profile).unwrap(),
    )
    .unwrap();
    fs::write(data.join("Default/Network/Cookies"), b"cookie-db").unwrap();
    fs::write(data.join("Local State"), b"{}").unwrap();
    fs::create_dir_all(data.join("Default/Cache")).unwrap();
    fs::write(data.join("Default/Cache/data_0"), vec![0u8; 4096]).unwrap();
  }

  fn entry_names(path: &Path) -> Vec<String> {
    let mut archive = open_archive(path).unwrap();
    (0..archive.len())
      .map(|i| archive.by_index(i).unwrap().name().unwrap().to_string())
      .collect()
  }

  #[test]
  fn collect_files_skips_the_caches_the_browser_rebuilds() {
    let root = TempDir::new().unwrap();
    let profiles_dir = root.path().join("profiles");
    let profile = sample_profile("Shop");
    seed_profile_dir(&profiles_dir, &profile);

    let collected = collect_files(&profiles_dir.join(profile.id.to_string())).unwrap();
    let relatives: Vec<&str> = collected.iter().map(|(r, _)| r.as_str()).collect();
    assert!(relatives.contains(&"metadata.json"));
    assert!(relatives.contains(&"profile/Default/Network/Cookies"));
    assert!(
      !relatives.iter().any(|r| r.contains("Cache")),
      "caches are rebuilt by the browser and must not travel: {relatives:?}"
    );
  }

  #[test]
  fn an_encrypted_backup_round_trips_through_the_envelope() {
    let root = TempDir::new().unwrap();
    let plain = root.path().join("plain.zip");
    fs::write(&plain, b"archive-body").unwrap();
    let sealed = root.path().join("backup.neobackup");

    seal_file(&plain, &sealed, "correct horse").unwrap();
    assert_eq!(
      &fs::read(&sealed).unwrap()[..ENVELOPE_MAGIC.len()],
      ENVELOPE_MAGIC,
      "a sealed archive announces itself with the envelope magic"
    );

    let opened = root.path().join("opened.zip");
    open_sealed_file(&sealed, &opened, "correct horse").unwrap();
    assert_eq!(fs::read(&opened).unwrap(), b"archive-body");

    // A wrong password fails with the coded error, before any extraction.
    let error = open_sealed_file(&sealed, &opened, "wrong").unwrap_err();
    assert!(error.contains("INCORRECT_PASSWORD"), "got: {error}");
  }

  #[test]
  fn a_multi_chunk_envelope_round_trips_and_a_truncated_tail_is_refused() {
    let root = TempDir::new().unwrap();
    let plain = root.path().join("plain.zip");
    // Two chunks' worth of plaintext, so the tail loss is detectable.
    fs::write(&plain, vec![7u8; CHUNK_SIZE + 64]).unwrap();
    let sealed = root.path().join("backup.neobackup");
    seal_file(&plain, &sealed, "pw").unwrap();

    let opened = root.path().join("opened.zip");
    open_sealed_file(&sealed, &opened, "pw").unwrap();
    assert_eq!(fs::read(&opened).unwrap(), vec![7u8; CHUNK_SIZE + 64]);

    let full = fs::read(&sealed).unwrap();
    let truncated = root.path().join("truncated.neobackup");
    fs::write(&truncated, &full[..full.len() - 64]).unwrap();
    let error = open_sealed_file(&truncated, &opened, "pw").unwrap_err();
    assert!(
      error.contains("BACKUP_ARCHIVE_INVALID") || error.contains("INCORRECT_PASSWORD"),
      "a truncated tail must not decrypt silently: {error}"
    );
  }

  #[test]
  fn the_archive_extension_is_appended_only_when_missing() {
    assert_eq!(
      archive_path("/tmp/backup").file_name().unwrap(),
      "backup.neobackup"
    );
    assert_eq!(
      archive_path("/tmp/backup.neobackup").file_name().unwrap(),
      "backup.neobackup"
    );
    assert_eq!(
      archive_path("/tmp/backup.zip").file_name().unwrap(),
      "backup.zip.neobackup"
    );
  }

  #[test]
  fn an_entry_name_that_leaves_its_directory_is_not_a_path() {
    assert_eq!(
      safe_entry_name("profile/Default/Cookies").as_deref(),
      Some("profile/Default/Cookies")
    );
    assert_eq!(safe_entry_name("../escaped"), None);
    assert_eq!(safe_entry_name("profile/../../escaped"), None);
    assert_eq!(safe_entry_name(""), None);
  }

  #[test]
  fn a_backup_and_restore_round_trip_through_the_data_dir() {
    let data = TempDir::new().unwrap();
    let _guard = crate::app_dirs::set_test_data_dir(data.path().to_path_buf());
    let profiles_dir = crate::app_dirs::profiles_dir();

    let profile = sample_profile("Shop");
    seed_profile_dir(&profiles_dir, &profile);

    let target = data.path().join("backup.neobackup");
    let result = backup_blocking(Some(vec![profile.id.to_string()]), true, &target, None).unwrap();
    assert_eq!(result.profile_count, 1);
    assert!(target.is_file());

    let names = entry_names(&target);
    assert!(names.contains(&MANIFEST_ENTRY.to_string()));
    assert!(names.contains(&PROXIES_ENTRY.to_string()));
    assert!(
      names.contains(&format!("{PROFILES_PREFIX}{}/metadata.json", profile.id)),
      "the profile's configuration travels: {names:?}"
    );
    assert!(
      names.contains(&format!(
        "{PROFILES_PREFIX}{}/profile/Default/Network/Cookies",
        profile.id
      )),
      "the cookie store travels: {names:?}"
    );
    assert!(
      !names.iter().any(|n| n.contains("Cache")),
      "caches are rebuilt by the browser and must not travel: {names:?}"
    );

    // A restore over the same machine without overwrite skips; with overwrite
    // it replaces the profile and keeps the sessions.
    let skipped = restore_blocking(&target, None, false).unwrap();
    assert_eq!(skipped.profiles_restored, 0);
    assert_eq!(skipped.profiles_skipped, 1);

    // Remove the profile, then restore it back: an idempotent-by-id restore.
    let uuid_dir = profiles_dir.join(profile.id.to_string());
    fs::remove_dir_all(&uuid_dir).unwrap();
    let restored = restore_blocking(&target, None, false).unwrap();
    assert_eq!(restored.profiles_restored, 1);
    assert_eq!(
      fs::read(uuid_dir.join("profile/Default/Network/Cookies")).unwrap(),
      b"cookie-db"
    );
    assert!(!uuid_dir.join("profile/Default/Cache").exists());
    assert!(
      uuid_dir.join("metadata.json").is_file(),
      "save_profile rewrites the metadata after the data lands"
    );
    let reread = crate::profile::ProfileManager::instance()
      .find_profile(&profile.id.to_string())
      .unwrap();
    assert_eq!(reread.name, "Shop");
    assert_eq!(reread.process_id, None);
  }

  #[test]
  fn a_restore_refuses_an_archive_that_climbs_out_of_its_directory() {
    let root = TempDir::new().unwrap();
    // Hermetic: the lookup and the extraction read the test data dir, never
    // this machine's real profiles folder.
    let _guard = crate::app_dirs::set_test_data_dir(root.path().join("data").to_path_buf());
    let hostile = root.path().join("hostile.neobackup");
    let hostile_profile = crate::profile::BrowserProfile {
      id: uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
      name: "Hostile".to_string(),
      browser: "wayfern".to_string(),
      ..crate::profile::BrowserProfile::default()
    };
    {
      let file = fs::File::create(&hostile).unwrap();
      let mut writer = zip::ZipWriter::new(file);
      let options: zip::write::SimpleFileOptions = zip::write::FileOptions::default();
      writer.start_file(MANIFEST_ENTRY, options).unwrap();
      writer
        .write_all(
          serde_json::json!({
            "format_version": FORMAT_VERSION,
            "created_at": 1,
            "app_version": "test",
            "profile_count": 1,
            "includes_proxies": false,
            "encrypted": false,
          })
          .to_string()
          .as_bytes(),
        )
        .unwrap();
      writer
        .start_file(
          format!("{PROFILES_PREFIX}11111111-1111-1111-1111-111111111111/metadata.json"),
          options,
        )
        .unwrap();
      writer
        .write_all(serde_json::to_string(&hostile_profile).unwrap().as_bytes())
        .unwrap();
      writer
        .start_file(
          format!("{PROFILES_PREFIX}11111111-1111-1111-1111-111111111111/profile/../../escaped"),
          options,
        )
        .unwrap();
      writer.write_all(b"nope").unwrap();
      writer.finish().unwrap();
    }
    let error = restore_blocking(&hostile, None, false).unwrap_err();
    assert!(error.contains("BACKUP_UNSAFE_ARCHIVE"), "got: {error}");
    assert!(!root.path().join("escaped").exists());
  }

  #[test]
  fn an_archive_from_a_newer_build_is_refused_by_name() {
    let root = TempDir::new().unwrap();
    let future = root.path().join("future.neobackup");
    {
      let file = fs::File::create(&future).unwrap();
      let mut writer = zip::ZipWriter::new(file);
      let options: zip::write::SimpleFileOptions = zip::write::FileOptions::default();
      writer.start_file(MANIFEST_ENTRY, options).unwrap();
      writer
        .write_all(
          serde_json::json!({
            "format_version": FORMAT_VERSION + 1,
            "created_at": 1,
            "app_version": "NullFrame 99.0.0",
            "profile_count": 0,
            "includes_proxies": false,
            "encrypted": false,
          })
          .to_string()
          .as_bytes(),
        )
        .unwrap();
      writer.finish().unwrap();
    }
    let error = restore_blocking(&future, None, false).unwrap_err();
    assert!(error.contains("BACKUP_TOO_NEW"), "got: {error}");
  }

  #[test]
  fn a_file_that_is_not_an_archive_is_a_coded_error_not_a_panic() {
    let root = TempDir::new().unwrap();
    let bogus = root.path().join("notes.txt");
    fs::write(&bogus, b"just some text").unwrap();
    let error = restore_blocking(&bogus, None, false).unwrap_err();
    assert!(
      error.contains("BACKUP_ARCHIVE_INVALID") || error.contains("BACKUP_RESTORE_FAILED"),
      "got: {error}"
    );
  }

  #[test]
  fn a_cache_clean_frees_the_rebuildable_bytes_and_keeps_the_sessions() {
    let data = TempDir::new().unwrap();
    let _guard = crate::app_dirs::set_test_data_dir(data.path().to_path_buf());
    let profiles_dir = crate::app_dirs::profiles_dir();

    let profile = sample_profile("Shop");
    seed_profile_dir(&profiles_dir, &profile);

    let freed = clean_blocking(&profile.id.to_string(), false).unwrap();
    assert!(freed >= 4096, "the cache's bytes count as freed: {freed}");
    let data_dir = profiles_dir.join(profile.id.to_string()).join("profile");
    assert!(!data_dir.join("Default/Cache").exists());
    assert!(!data_dir.join("ShaderCache").exists());
    // The sessions survive a cache clean.
    assert_eq!(
      fs::read(data_dir.join("Default/Network/Cookies")).unwrap(),
      b"cookie-db"
    );
    assert!(data_dir.join("Local State").is_file());

    // A second clean has nothing left to free.
    assert_eq!(clean_blocking(&profile.id.to_string(), false).unwrap(), 0);
  }

  #[test]
  fn a_cookie_purge_takes_the_sessions_with_it() {
    let data = TempDir::new().unwrap();
    let _guard = crate::app_dirs::set_test_data_dir(data.path().to_path_buf());
    let profiles_dir = crate::app_dirs::profiles_dir();

    let profile = sample_profile("Shop");
    seed_profile_dir(&profiles_dir, &profile);

    let freed = clean_blocking(&profile.id.to_string(), true).unwrap();
    assert!(freed >= 4096 + b"cookie-db".len() as u64);
    let data_dir = profiles_dir.join(profile.id.to_string()).join("profile");
    assert!(!data_dir.join("Default/Network/Cookies").exists());
  }

  #[test]
  fn an_unknown_profile_is_a_coded_error() {
    let data = TempDir::new().unwrap();
    let _guard = crate::app_dirs::set_test_data_dir(data.path().to_path_buf());
    let error = clean_blocking(&uuid::Uuid::new_v4().to_string(), false).unwrap_err();
    assert!(error.contains("PROFILE_NOT_FOUND"), "got: {error}");
  }
}
