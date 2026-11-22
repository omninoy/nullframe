#![allow(dead_code, unused_variables, unused_imports)]

//! Cookie-bot transport.
//!
//! The bot warms a profile's cookies overnight by driving it on a remote host.
//! None of that behaviour lives here: the schedule and everything the bot
//! actually does are owned by the cloud API.
//!
//! This module is the wire only. It sends the user's own scalars — when to
//! run, for how long, which of their sites, which server-issued preset id —
//! and renders back what the server says happened. It deliberately keeps NO
//! local copy of a schedule: the server holds the only one, so two desktops
//! signed into one account cannot disagree about when the bot runs.

use crate::cloud_errors::{self, BackendFailure, FailureCodes};
use crate::profile::types::BrowserProfile;
use crate::sync::encryption::remote_seal;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

/// Operating systems a remote run can be scheduled on. Anything else (a mobile
/// OS, a typo) has no host, so it is refused here rather than as a failed run
/// at 02:00.
pub const BOT_PLATFORMS: [&str; 3] = ["windows", "macos", "linux"];

const MAX_PROFILE_NAME_UNITS: usize = 200;

static ENROLLED_PROFILES: std::sync::Mutex<Option<HashSet<String>>> = std::sync::Mutex::new(None);

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Failure codes for the schedule routes.
const SCHEDULE_CODES: FailureCodes = FailureCodes {
  bad_request: "COOKIE_BOT_INVALID_SCHEDULE",
  forbidden: "COOKIE_BOT_NOT_ENTITLED",
  not_found: "COOKIE_BOT_NOT_ENROLLED",
  conflict: "COOKIE_BOT_SCHEDULE_CONFLICT",
};

/// Failure codes for the run routes.
const RUN_CODES: FailureCodes = FailureCodes {
  bad_request: "COOKIE_BOT_INVALID_SCHEDULE",
  forbidden: "COOKIE_BOT_NOT_ENTITLED",
  not_found: "COOKIE_BOT_RUN_NOT_FOUND",
  conflict: "COOKIE_BOT_RUN_IN_PROGRESS",
};

/// Failure codes for the read-only reporting routes.
const REPORT_CODES: FailureCodes = FailureCodes {
  bad_request: "COOKIE_BOT_INVALID_PERIOD",
  forbidden: "NOT_TEAM_MEMBER",
  not_found: cloud_errors::UNAVAILABLE,
  conflict: cloud_errors::UNAVAILABLE,
};

/// Failure codes for the user-template routes.
///
/// Distinct from `SCHEDULE_CODES` on every axis that matters: a 404 here is a
/// template that was deleted (possibly from another device), not an unenrolled
/// profile, and a 409 is a name the user already used, not a teammate's
/// enrolment. Sharing the schedule set would have told someone renaming a site
/// list that a colleague already warms this profile.
const TEMPLATE_CODES: FailureCodes = FailureCodes {
  bad_request: "COOKIE_BOT_INVALID_TEMPLATE_NAME",
  forbidden: "COOKIE_BOT_NOT_ENTITLED",
  not_found: "COOKIE_BOT_TEMPLATE_NOT_FOUND",
  conflict: "COOKIE_BOT_TEMPLATE_NAME_TAKEN",
};

/// Every cookie-bot call fails as a code the frontend can translate.
///
/// There is no `Other(String)` carrying backend English: a raw message reaches
/// the user untranslated, which is the bug pattern the `{"code":…}` convention
/// exists to block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieBotError(pub BackendFailure);

impl CookieBotError {
  pub fn code(&self) -> &str {
    &self.0.code
  }

  pub fn status(&self) -> u16 {
    self.0.status
  }

  /// The `{"code":…,"params":{…}}` string a Tauri command returns.
  pub fn to_error_json(&self) -> String {
    self.0.to_error_json()
  }
}

impl std::fmt::Display for CookieBotError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.to_error_json())
  }
}

impl From<BackendFailure> for CookieBotError {
  fn from(failure: BackendFailure) -> Self {
    Self(failure)
  }
}

// --- Wire types -------------------------------------------------------------
//
// One place for every request and response shape, so a backend contract change
// is a single edit here rather than a hunt through call sites.

/// One time-of-day an enrolment fires, on a set of local weekdays.
///
/// Copy, and deliberately tiny: a calendar is a list of these, and the desktop
/// rebuilds that list on every keystroke in the enrolment form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotSlot {
  /// Bitmask of local weekdays, bit 0 = Monday. At least one bit set.
  pub days_mask: u8,
  /// Minutes past local midnight, in the schedule's timezone.
  pub run_at_minute: u16,
}

/// A profile enrolled in the nightly bot.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotSchedule {
  pub profile_id: String,
  pub profile_name: String,
  pub platform: String,
  pub enabled: bool,
  /// Minutes past local midnight the FIRST slot is anchored to. The server
  /// mirrors `slots[0]` onto this pair on every write.
  pub run_at_minute: u16,
  /// The first slot's weekdays, bit 0 = Monday. See `run_at_minute`.
  pub days_mask: u8,
  /// Every time-of-day this enrolment fires.
  ///
  /// `default` rather than required because a server older than multi-slot
  /// scheduling sends only the mirrored pair above, and a decode failure there
  /// would blank the whole Cookie Bot surface rather than show one time instead
  /// of several. Callers must therefore fall back to the pair when this is
  /// empty — never treat an empty list as "fires at no time".
  #[serde(default)]
  pub slots: Vec<CookieBotSlot>,
  pub timezone: String,
  /// Server-issued preset id. Opaque here — what it expands to is the server's.
  pub preset: String,
  /// The template the sites came from, or `None` for the user's own list.
  ///
  /// A built-in id (`low-intent-purchaser`) means `sites` is EMPTY on purpose:
  /// its URLs are server-owned and never sent to a client. A `user:<uuid>` id
  /// is provenance only — those sites were copied onto the enrolment and are
  /// present below.
  #[serde(default)]
  pub template_id: Option<String>,
  pub max_minutes: u32,
  #[serde(default)]
  pub sites: Vec<String>,
  #[serde(default)]
  pub jitter_seconds: u32,

  // The profile facts the desktop declared, echoed back on every read. Kept so
  // the UI can tell that what the server believes about a profile no longer
  // matches what this machine can see, and re-declare it.
  #[serde(default)]
  pub sync_enabled: bool,
  #[serde(default)]
  pub encrypted_sync: bool,
  #[serde(default)]
  pub has_proxy: bool,
  /// Whether that exit is one a remote host could dial. Defaults to false on an
  /// older server that does not send it, which reads as "not reachable" and is
  /// the safe direction.
  #[serde(default)]
  pub proxy_remote_reachable: bool,
  #[serde(default)]
  pub touch_fingerprint: bool,
  #[serde(default)]
  pub sticky_exit: bool,
  /// When those facts were last refreshed.
  #[serde(default)]
  pub profile_state_at: Option<String>,
  /// The last check of the profile's proxy or VPN from this computer. Never
  /// sent by the server; filled in from local state.
  #[serde(default)]
  pub exit_check: Option<crate::cookie_bot_exit::ExitCheckView>,

  /// Why tonight would be refused, or `None`.
  ///
  /// The server computes this on every read precisely so a broken enrolment is
  /// visible the moment it breaks. Dropping it meant a profile whose proxy was
  /// detached in the afternoon still showed a healthy row and a next-run time,
  /// and first announced itself with a skipped run at 02:00.
  #[serde(default)]
  pub blocked_by: Option<String>,

  #[serde(default)]
  pub next_run_at: Option<String>,
  #[serde(default)]
  pub last_run_at: Option<String>,
  #[serde(default)]
  pub last_run_id: Option<String>,
  #[serde(default)]
  pub owner_user_id: Option<String>,
  #[serde(default)]
  pub owner_email: Option<String>,
  #[serde(default)]
  pub updated_at: Option<String>,
}

/// What the desktop sends when enrolling or editing.
///
/// `next_run_at` is absent by design: the server recomputes it and ignores any
/// client value, so there is nothing here for two devices to disagree about.
#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotScheduleInput {
  pub profile_name: String,
  pub platform: String,
  pub enabled: bool,
  pub run_at_minute: u16,
  pub days_mask: u8,
  /// The whole calendar, when the caller has one.
  ///
  /// `skip_serializing_if` is load-bearing rather than tidiness: the server
  /// reads an ABSENT `slots` as "one slot, from the pair above" and refuses a
  /// present-but-empty one, and `null` takes the refusing branch. Serialising
  /// `None` as null would 400 every write from a single-slot form.
  ///
  /// The pair above is still sent, mirrored from `slots[0]`, so a server that
  /// predates multi-slot stores the first time rather than nothing.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub slots: Option<Vec<CookieBotSlot>>,
  pub timezone: String,
  pub preset: String,
  /// A browsing template instead of a typed site list.
  ///
  /// Mutually exclusive with a non-empty `sites`: the server refuses a write
  /// carrying both, because merging a curated persona with the user's own list
  /// produces neither. A caller naming a template sends `sites: []`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub template_id: Option<String>,
  pub max_minutes: u32,
  pub sites: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub jitter_seconds: Option<u32>,

  // The profile facts the server refuses a run on. It cannot read them itself —
  // the profile lives in the user's sync namespace, not in its database — so the
  // desktop reports them and the server decides.
  //
  // Every caller (Tauri, REST, MCP) overwrites all five through
  // `with_profile_state`, derived from the profile itself, so a caller can never
  // assert them. They are therefore `default` on the way IN — which is what the
  // GUI sends, and what the Tauri command's own argument deserialization
  // requires, since demanding them made every enrolment fail with
  // `invalid args schedule: missing field sync_enabled` before the stamping
  // could run — and unconditionally present on the way OUT, because the server
  // rejects a write that omits them.
  //
  // Defaulting is safe in exactly one direction: `bool::default()` is false, so
  // an unstamped input reads as "no sync, no proxy" and is REFUSED. The failure
  // this must never have is the opposite one, a defaulted `has_proxy: true`
  // warming a profile out of the remote host's own address.
  #[serde(default)]
  pub sync_enabled: bool,
  #[serde(default)]
  pub has_proxy: bool,
  #[serde(default)]
  pub proxy_remote_reachable: bool,
  #[serde(default)]
  pub encrypted_sync: bool,
  #[serde(default)]
  pub touch_fingerprint: bool,
  #[serde(default)]
  pub sticky_exit: bool,
  #[serde(default)]
  pub exit_alive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotScheduleList {
  #[serde(default)]
  pub schedules: Vec<CookieBotSchedule>,
  #[serde(default)]
  pub team_id: Option<String>,
  #[serde(default)]
  pub scope: Option<String>,
}

/// A teammate's enrolment of the same profile.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotConflict {
  pub user_id: String,
  pub email: String,
  pub run_at_minute: u16,
  pub timezone: String,
  pub days_mask: u8,
  pub enabled: bool,
  /// Set on the dry-run check: the two enrolments share a weekday and fire
  /// within an hour of each other.
  #[serde(default)]
  pub overlaps: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotScheduleSaved {
  pub schedule: CookieBotSchedule,
  /// Repeated on a successful acknowledged write so the UI can keep showing
  /// the warning rather than pretending the collision went away.
  #[serde(default)]
  pub conflicts: Vec<CookieBotConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotConflictCheck {
  pub profile_id: String,
  #[serde(default)]
  pub conflicts: Vec<CookieBotConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotScheduleDeleted {
  pub profile_id: String,
  pub deleted: bool,
}

/// One night's work on one profile.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotRun {
  pub id: String,
  pub profile_id: String,
  #[serde(default)]
  pub profile_name: Option<String>,
  #[serde(default)]
  pub user_id: Option<String>,
  #[serde(default)]
  pub email: Option<String>,
  /// `schedule` or `manual`.
  #[serde(default)]
  pub team_id: Option<String>,
  pub trigger: String,
  /// `pending` | `running` | `succeeded` | `partial` | `failed` | `skipped` |
  /// `cancelled`.
  pub status: String,
  pub scheduled_for: String,
  /// The jittered instant the run was allowed to start.
  #[serde(default)]
  pub dispatch_after: Option<String>,
  #[serde(default)]
  pub started_at: Option<String>,
  #[serde(default)]
  pub ended_at: Option<String>,
  /// The night's whole budget, which may be split across several chunks.
  #[serde(default)]
  pub max_minutes: u32,
  /// How many browser sessions this night is split into, and which one is
  /// running. "chunk 2 of 3" is the only honest way to report a night the
  /// server split.
  #[serde(default)]
  pub chunks_total: u32,
  #[serde(default)]
  pub chunk_index: u32,
  #[serde(default)]
  pub sites_total: u32,
  #[serde(default)]
  pub sites_visited: u32,
  #[serde(default)]
  pub sites_failed: u32,
  #[serde(default)]
  pub consent_dismissed: u32,
  #[serde(default)]
  pub billed_seconds: u64,
  /// Why it ended the way it did, e.g. `profile_locked`, `no_capacity`.
  #[serde(default)]
  pub outcome_code: Option<String>,
  #[serde(default)]
  pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotRunPage {
  #[serde(default)]
  pub runs: Vec<CookieBotRun>,
  /// Keyset cursor; `None` on the last page.
  #[serde(default)]
  pub next_before: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotRunStarted {
  pub run: CookieBotRun,
  #[serde(default)]
  pub session_id: Option<String>,
}

/// A named intensity the user can pick. The client never learns what it
/// expands to; only enough to label the choice and show its rough cost.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotPreset {
  pub id: String,
  #[serde(default)]
  pub typical_minutes: Option<u32>,
  #[serde(default)]
  pub recommended: bool,
  /// Server-supplied English label, present only so a preset added after this
  /// build still renders. The UI must prefer its own `t()` key for a known id.
  #[serde(default)]
  pub name: Option<String>,
  #[serde(default)]
  pub description: Option<String>,
}

/// A server-owned browsing template: a named answer to "what is this profile
/// for", which the user picks INSTEAD of typing a site list.
///
/// Carries no URLs, and must not gain any: the site pool a template draws from
/// is server-owned, and a published list is one a retailer can filter.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotTemplate {
  pub id: String,
  /// How many sites this template browses. Not which.
  #[serde(default)]
  pub site_count: u32,
  /// Server-supplied English label and blurb, present only so a template added
  /// after this build still renders. The UI prefers its own `t()` key for an id
  /// it recognises.
  #[serde(default)]
  pub name: Option<String>,
  #[serde(default)]
  pub description: Option<String>,
}

/// The bounds the schedule routes actually enforce, as this build reads them.
///
/// Every field is optional because a server that predates `limits` sends none
/// of them, and a client that read a missing bound as `0` would refuse every
/// value the form can produce. Only the bounds the desktop acts on are decoded
/// — serde drops the rest, and this struct is what the GUI ultimately receives,
/// so adding a field here is what makes one reachable from TypeScript.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotLimits {
  #[serde(default)]
  pub min_minutes: Option<u32>,
  #[serde(default)]
  pub max_minutes: Option<u32>,
  #[serde(default)]
  pub min_sites: Option<u32>,
  #[serde(default)]
  pub max_sites: Option<u32>,
  /// Most entries a calendar may carry.
  #[serde(default)]
  pub max_slots: Option<u32>,
  /// Longest name a saved site list may be given.
  #[serde(default)]
  pub max_template_name_length: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotPresetList {
  #[serde(default)]
  pub presets: Vec<CookieBotPreset>,
  /// Which preset the server suggests when the user has expressed no
  /// preference.
  #[serde(default)]
  pub default_preset: Option<String>,
  /// The curated templates on offer. Served beside the presets so a template
  /// added server-side appears without a desktop release.
  #[serde(default)]
  pub templates: Vec<CookieBotTemplate>,
  /// The server's own bounds, when it publishes them. The desktop mirrors a
  /// copy for offline form validation; these win where they disagree.
  #[serde(default)]
  pub limits: Option<CookieBotLimits>,
}

/// One of the caller's OWN saved site lists.
///
/// Carries its URLs, unlike {@link CookieBotTemplate} — they are the user's own
/// and there is nothing to withhold. Applying one copies the sites onto the
/// enrolment, so a list edited later does not silently change what an existing
/// enrolment browses until it is saved again.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotUserTemplate {
  /// Already carries the `user:` prefix: this id's job is to be pasted into a
  /// schedule's `template_id`, and assembling that convention on the client is
  /// how the two kinds of template get confused.
  pub id: String,
  pub name: String,
  #[serde(default)]
  pub sites: Vec<String>,
  #[serde(default)]
  pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RemoteHoursBreakdown {
  #[serde(default)]
  pub interactive_hours: f64,
  #[serde(default)]
  pub bot_hours: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RemoteHoursMember {
  pub user_id: String,
  pub email: String,
  #[serde(default)]
  pub role: Option<String>,
  #[serde(default)]
  pub used_hours: f64,
  #[serde(default)]
  pub interactive_hours: f64,
  #[serde(default)]
  pub bot_hours: f64,
}

/// The remote-hour budget as the server reports it, with the bot/interactive
/// breakdown it sends.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RemoteHoursQuota {
  pub granted_hours: f64,
  pub remaining_hours: f64,
  #[serde(default)]
  pub used_hours: f64,
  #[serde(default)]
  pub period_start: Option<String>,
  #[serde(default)]
  pub period_end: Option<String>,
  /// `user` or `team`.
  #[serde(default)]
  pub scope: Option<String>,
  #[serde(default)]
  pub team_id: Option<String>,
  #[serde(default)]
  pub seats: u32,
  #[serde(default)]
  pub per_seat_hours: f64,
  #[serde(default)]
  pub breakdown: Option<RemoteHoursBreakdown>,
  /// The full roster for an owner or admin; just the caller otherwise.
  #[serde(default)]
  pub members: Vec<RemoteHoursMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotUsageMember {
  pub user_id: String,
  pub email: String,
  #[serde(default)]
  pub role: Option<String>,
  #[serde(default)]
  pub interactive_hours: f64,
  #[serde(default)]
  pub bot_hours: f64,
  #[serde(default)]
  pub used_hours: f64,
  #[serde(default)]
  pub sessions: u32,
  #[serde(default)]
  pub bot_runs: u32,
  #[serde(default)]
  pub bot_runs_failed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotUsageProfile {
  pub profile_id: String,
  #[serde(default)]
  pub profile_name: Option<String>,
  #[serde(default)]
  pub owner_email: Option<String>,
  #[serde(default)]
  pub bot_hours: f64,
  #[serde(default)]
  pub runs: u32,
  /// How many of those runs did not do what they were asked. Its sibling on the
  /// member view is `bot_runs_failed`; both are on the wire and both belong in
  /// the report.
  #[serde(default)]
  pub runs_failed: u32,
  #[serde(default)]
  pub last_run_at: Option<String>,
  #[serde(default)]
  pub last_status: Option<String>,
}

/// The team owner's after-the-fact view of who spent what.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CookieBotUsage {
  pub period: String,
  #[serde(default)]
  pub period_start: Option<String>,
  #[serde(default)]
  pub period_end: Option<String>,
  #[serde(default)]
  pub team_id: Option<String>,
  #[serde(default)]
  pub seats: u32,
  #[serde(default)]
  pub granted_hours: f64,
  #[serde(default)]
  pub used_hours: f64,
  #[serde(default)]
  pub remaining_hours: f64,
  #[serde(default)]
  pub members: Vec<CookieBotUsageMember>,
  #[serde(default)]
  pub profiles: Vec<CookieBotUsageProfile>,
}

// --- Client-side preconditions ---------------------------------------------

/// [`bot_precondition`] for local runs: always allowed.
pub fn run_precondition(_profile: &BrowserProfile) -> Result<(), String> {
  Ok(())
}

/// Whether this profile could ever be warmed by the bot: always allowed locally.
pub fn bot_precondition(
  _profile: &BrowserProfile,
  _exit: &crate::remote_exit::ExitReachability,
) -> Result<(), String> {
  Ok(())
}

/// The profile facts the server needs but cannot see.
///
/// The server holds the schedule; the PROFILE lives in the user's sync
/// namespace, so `sync_enabled`, `has_proxy` and the rest are only knowable
/// here. It requires them on every write rather than defaulting them, because
/// a defaulted `has_proxy` is a profile warmed out of the remote host's own
/// address.
///
/// Derived in one place so the Tauri, REST and MCP call sites cannot drift into
/// three different answers about the same profile.
pub fn profile_state(profile: &BrowserProfile) -> ProfileState {
  profile_state_from(
    profile,
    crate::sync::encryption::has_e2e_password(),
    crate::cookie_bot_exit::exit_alive(profile),
  )
}

/// [`profile_state`] with the facts of this computer handed in.
fn profile_state_from(
  profile: &BrowserProfile,
  sync_password_stored: bool,
  exit_alive: bool,
) -> ProfileState {
  ProfileState {
    sync_enabled: profile.is_sync_enabled(),
    encrypted_sync: remote_seal(profile, sync_password_stored).is_some(),
    // A VPN is an exit node just as much as a proxy is; the server only asks
    // whether the traffic leaves through something the user brought.
    has_proxy: profile.proxy_id.is_some() || profile.vpn_id.is_some(),
    // ...and, separately, whether anyone OTHER than this machine could use it.
    // `has_proxy` answers "did the user bring an exit"; this answers "is that
    // exit an address a remote host can dial". They disagree for every local
    // proxy, which is the case that used to be accepted and then fail remotely.
    // See `remote_exit`.
    proxy_remote_reachable: exit_reachability(profile).is_remote(),
    // Always false: this data model has no mobile/touch profile. `resolved_os`
    // yields only windows, macos or linux, and all three are supported
    // remotely. Reported rather than omitted so the server keeps one
    // required shape, and it stays authoritative: it sees the real fingerprint
    // on the host and can still refuse a run this cannot know to reject.
    touch_fingerprint: false,
    // A VPN is one persistent tunnel, so the night's chunks share an exit. A
    // stored proxy may rotate per connection, and claiming stickiness we cannot
    // guarantee is worse than declining it, so the conservative answer is the
    // safe one either way.
    sticky_exit: profile.vpn_id.is_some(),
    exit_alive,
  }
}

/// What {@link profile_state} derives. Applied onto a schedule input before it
/// is sent.
#[derive(Debug, Clone, Copy)]
pub struct ProfileState {
  pub sync_enabled: bool,
  pub encrypted_sync: bool,
  pub has_proxy: bool,
  /// Whether that exit is an address a remote host can dial.
  pub proxy_remote_reachable: bool,
  pub touch_fingerprint: bool,
  pub sticky_exit: bool,
  /// False once checks from this computer confirm the exit cannot be reached.
  pub exit_alive: bool,
}

/// Whether this profile's exit could be used from a host that is not this one.
///
/// Resolves the profile's proxy or VPN out of local storage — the server cannot
/// do this, because it never sees a proxy record until sync has uploaded one and
/// even then would have to re-derive what the browser will actually dial.
///
/// A profile carrying BOTH a proxy and a VPN is judged on the proxy: that is
/// what the browser is pointed at, and it is the address a remote host has to
/// reach.
pub fn exit_reachability(profile: &BrowserProfile) -> crate::remote_exit::ExitReachability {
  use crate::remote_exit::{classify_proxy, classify_wireguard_endpoint, ExitReachability};

  if let Some(proxy_id) = profile.proxy_id.as_deref() {
    let stored = crate::proxy_manager::PROXY_MANAGER
      .get_stored_proxies()
      .into_iter()
      .find(|candidate| candidate.id == proxy_id);
    return match stored {
      Some(proxy) => classify_proxy(&proxy.proxy_settings),
      // Referenced but missing. Fail closed: a dangling id is not evidence of a
      // reachable exit, and the launch would fail anyway.
      None => ExitReachability::Unknown {
        reason: "the profile references a proxy that no longer exists".to_string(),
        source: "proxy",
      },
    };
  }

  if let Some(vpn_id) = profile.vpn_id.as_deref() {
    let config = crate::vpn::VPN_STORAGE
      .lock()
      .ok()
      .and_then(|storage| storage.load_config(vpn_id).ok());
    return match config {
      Some(config) => match crate::vpn::parse_wireguard_config(&config.config_data) {
        Ok(parsed) => classify_wireguard_endpoint(&parsed.peer_endpoint),
        Err(error) => ExitReachability::Unknown {
          reason: format!("VPN config could not be parsed ({error})"),
          source: "VPN",
        },
      },
      None => ExitReachability::Unknown {
        reason: "the profile references a VPN config that no longer exists".to_string(),
        source: "VPN",
      },
    };
  }

  ExitReachability::None
}

impl CookieBotScheduleInput {
  /// Stamp the profile facts onto an input built from user-chosen values.
  pub fn with_profile_state(mut self, state: ProfileState) -> Self {
    self.sync_enabled = state.sync_enabled;
    self.encrypted_sync = state.encrypted_sync;
    self.has_proxy = state.has_proxy;
    self.proxy_remote_reachable = state.proxy_remote_reachable;
    self.touch_fingerprint = state.touch_fingerprint;
    self.sticky_exit = state.sticky_exit;
    self.exit_alive = state.exit_alive;
    self
  }
}

fn error(code: &str, params: &[(&str, &str)]) -> String {
  let mut object = serde_json::Map::new();
  object.insert(
    "code".to_string(),
    serde_json::Value::String(code.to_string()),
  );
  if !params.is_empty() {
    let map = params
      .iter()
      .map(|(k, v)| {
        (
          (*k).to_string(),
          serde_json::Value::String((*v).to_string()),
        )
      })
      .collect::<serde_json::Map<_, _>>();
    object.insert("params".to_string(), serde_json::Value::Object(map));
  }
  serde_json::Value::Object(object).to_string()
}

// --- Routes -----------------------------------------------------------------

fn base() -> String {
  format!("{}/api/cookie-bot", crate::cloud_auth::CLOUD_API_URL)
}

static APP_HANDLE: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

pub fn init(handle: &tauri::AppHandle) {
  let _ = APP_HANDLE.set(handle.clone());
  start_scheduler(handle.clone());
}

pub fn app_handle() -> Option<tauri::AppHandle> {
  APP_HANDLE.get().cloned()
}

pub fn start_scheduler(_app_handle: tauri::AppHandle) {
  use chrono::Datelike;
  use chrono::Timelike;
  tauri::async_runtime::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
      interval.tick().await;

      let schedules = crate::cookie_bot_storage::load_schedules();
      let now_utc = chrono::Utc::now();

      for (_pid, schedule) in schedules {
        if !schedule.enabled {
          continue;
        }

        let tz: chrono_tz::Tz = schedule.timezone.parse().unwrap_or(chrono_tz::UTC);
        let now_local = now_utc.with_timezone(&tz);

        let weekday_num = now_local.weekday().num_days_from_monday() as u8;
        let day_bit = 1u8 << weekday_num;
        let current_minute = (now_local.hour() * 60 + now_local.minute()) as u16;

        for slot in &schedule.slots {
          if (slot.days_mask & day_bit) != 0 && slot.run_at_minute == current_minute {
            let recent_run = crate::cookie_bot_storage::load_runs().into_iter().any(|r| {
              r.profile_id == schedule.profile_id
                && r.started_at.as_ref().is_some_and(|st| {
                  if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(st) {
                    (now_utc - dt.with_timezone(&chrono::Utc)).num_minutes() < 10
                  } else {
                    false
                  }
                })
            });

            if !recent_run {
              log::info!(
                "Scheduler triggering scheduled run for profile={}",
                schedule.profile_id
              );
              let started = run_now(&schedule.profile_id, Some(schedule.max_minutes)).await;
              if started.is_ok() {
                // Keep the schedule's own timestamps in step with the run the
                // ticker just fired: the UI reads these to show "next run".
                let mut updated = schedule.clone();
                updated.last_run_at = Some(now_utc.to_rfc3339());
                updated.next_run_at =
                  next_run_at_for(&schedule, now_utc + chrono::Duration::minutes(1));
                crate::cookie_bot_storage::upsert_schedule(updated);
              }
            }
          }
        }
      }
    }
  });
}

/// The schedule's next fire instant: the earliest slot occurrence whose
/// weekday bit is set and anchor minute is still ahead, searching up to a
/// week ahead in the schedule's timezone. `None` when the zone does not
/// parse or no slot ever fires. The desktop enrolls without a `next_run_at`
/// (the cloud server used to recompute it), so the local build derives it.
pub fn next_run_at_for(
  schedule: &CookieBotSchedule,
  now: chrono::DateTime<chrono::Utc>,
) -> Option<String> {
  use chrono::Datelike;
  use chrono::Timelike;
  let tz: chrono_tz::Tz = schedule.timezone.parse().ok()?;
  let local_now = now.with_timezone(&tz);
  for day_offset in 0..=7i64 {
    let day = local_now.date_naive() + chrono::Duration::days(day_offset);
    let day_bit = 1u8 << day.weekday().num_days_from_monday();
    let midnight = day
      .and_hms_opt(0, 0, 0)?
      .and_local_timezone(tz)
      .earliest()?;
    let mut best: Option<chrono::DateTime<chrono_tz::Tz>> = None;
    for slot in &schedule.slots {
      if slot.days_mask & day_bit == 0 {
        continue;
      }
      let candidate = midnight + chrono::Duration::minutes(i64::from(slot.run_at_minute));
      if candidate <= local_now {
        continue;
      }
      if best.is_none_or(|current| candidate < current) {
        best = Some(candidate);
      }
    }
    // Slots on later days always fire later than any candidate found today.
    if best.is_some() {
      return best.map(|at| at.with_timezone(&chrono::Utc).to_rfc3339());
    }
  }
  None
}

/// Every enrolment the caller can see.
pub async fn list_schedules(_scope: Option<&str>) -> Result<CookieBotScheduleList, CookieBotError> {
  let map = crate::cookie_bot_storage::load_schedules();
  let mut schedules: Vec<CookieBotSchedule> = map.into_values().collect();
  crate::cookie_bot_exit::attach_views(&mut schedules);
  Ok(CookieBotScheduleList {
    schedules,
    team_id: None,
    scope: Some("mine".to_string()),
  })
}

/// This profile's enrolment, or `None` when there is none.
pub async fn get_schedule(profile_id: &str) -> Result<Option<CookieBotSchedule>, CookieBotError> {
  let mut sched = crate::cookie_bot_storage::get_schedule(profile_id);
  if let Some(ref mut s) = sched {
    crate::cookie_bot_exit::attach_views(std::slice::from_mut(s));
  }
  Ok(sched)
}

#[derive(Debug, Deserialize)]
struct ScheduleEnvelope {
  schedule: CookieBotSchedule,
}

/// Create or replace this profile's enrolment.
pub async fn save_schedule(
  profile_id: &str,
  input: &CookieBotScheduleInput,
  _acknowledge_conflict: bool,
) -> Result<CookieBotScheduleSaved, CookieBotError> {
  let existing = crate::cookie_bot_storage::get_schedule(profile_id);
  let slots = input.slots.clone().unwrap_or_else(|| {
    vec![CookieBotSlot {
      days_mask: input.days_mask,
      run_at_minute: input.run_at_minute,
    }]
  });
  let schedule = CookieBotSchedule {
    profile_id: profile_id.to_string(),
    profile_name: input.profile_name.clone(),
    platform: input.platform.clone(),
    enabled: input.enabled,
    run_at_minute: input.run_at_minute,
    days_mask: input.days_mask,
    slots,
    timezone: input.timezone.clone(),
    preset: input.preset.clone(),
    template_id: input.template_id.clone(),
    max_minutes: input.max_minutes,
    sites: input.sites.clone(),
    jitter_seconds: input.jitter_seconds.unwrap_or(0),
    sync_enabled: true,
    encrypted_sync: false,
    has_proxy: input.has_proxy,
    proxy_remote_reachable: true,
    touch_fingerprint: false,
    sticky_exit: false,
    profile_state_at: Some(chrono::Utc::now().to_rfc3339()),
    exit_check: None,
    blocked_by: None,
    next_run_at: None,
    last_run_at: existing.as_ref().and_then(|s| s.last_run_at.clone()),
    last_run_id: existing.as_ref().and_then(|s| s.last_run_id.clone()),
    owner_user_id: None,
    owner_email: None,
    updated_at: Some(chrono::Utc::now().to_rfc3339()),
  };
  let mut schedule = schedule;
  schedule.next_run_at = next_run_at_for(&schedule, chrono::Utc::now());
  crate::cookie_bot_storage::upsert_schedule(schedule.clone());
  record_enrolment(&mut enrolled_profiles(), profile_id, true);
  crate::cookie_bot_exit::check_after_enrolment(profile_id);
  Ok(CookieBotScheduleSaved {
    schedule,
    conflicts: Vec::new(),
  })
}

/// Re-declare the profile facts the server cannot observe for itself.
///
/// Narrow on purpose: a detached proxy should not have to resend a whole
/// schedule and risk clobbering an edit made from another device in between.
pub async fn update_profile_state(
  profile_id: &str,
  state: ProfileState,
  identity: &ProfileIdentity,
) -> Result<CookieBotSchedule, CookieBotError> {
  let body = profile_state_body(state, identity);

  let envelope: ScheduleEnvelope = request(
    reqwest::Method::POST,
    format!(
      "{}/schedules/{}/profile-state",
      base(),
      urlencoding::encode(profile_id)
    ),
    Vec::new(),
    Some(body),
    SCHEDULE_CODES,
  )
  .await?;
  Ok(envelope.schedule)
}

/// Push this profile's current facts to the server, without blocking the edit
/// that changed them.
///
/// The server refuses a run on the copy the desktop last declared —
/// `has_proxy: false` is `proxy_required`, and that check exists because a run
/// without an exit node egresses from the remote host's own address.
/// Nothing but a full schedule write refreshed that copy, so detaching a proxy
/// from an enrolled profile left `has_proxy: true` on the row and the night ran
/// anyway. This closes that gap at the moment the profile changes.
///
/// Silent on failure by design: a profile edit must not fail because the cloud
/// is unreachable, an unenrolled profile answers `COOKIE_BOT_NOT_ENROLLED`
/// which is the normal case, and the next edit re-declares the same facts.
pub fn report_profile_state(profile: &BrowserProfile) {
  let profile_id = profile.id.to_string();
  if !should_report(&enrolled_profiles(), &profile_id) {
    return;
  }
  let state = profile_state(profile);
  let identity = profile_identity(profile);
  tauri::async_runtime::spawn(async move {
    send_profile_state(profile_id, state, identity).await;
  });
}

/// Re-declare the profile's facts now, even when they did not change, which
/// moves `profile_state_at` and so lifts a block the server holds until then.
pub async fn declare_profile_state(profile: &BrowserProfile) {
  send_profile_state(
    profile.id.to_string(),
    profile_state(profile),
    profile_identity(profile),
  )
  .await;
}

async fn send_profile_state(profile_id: String, state: ProfileState, identity: ProfileIdentity) {
  if crate::cloud_auth::CLOUD_AUTH.is_logged_in().await {
    match update_profile_state(&profile_id, state, &identity).await {
      Ok(_) => {
        STATE_REPORT.succeeded(&format!("profile={profile_id}"));
        record_enrolment(&mut enrolled_profiles(), &profile_id, true);
        log::debug!("Cookie bot profile state re-declared profile={profile_id}");
      }
      // Not enrolled is the common answer and not worth a log line at warn.
      Err(e) if e.code() == "COOKIE_BOT_NOT_ENROLLED" => {
        STATE_REPORT.succeeded(&format!("profile={profile_id}"));
        record_enrolment(&mut enrolled_profiles(), &profile_id, false);
      }
      Err(e) => STATE_REPORT.failed(&format!("profile={profile_id}"), e),
    }
  }
}

static STATE_REPORT: crate::log_streak::KeyedStreak =
  crate::log_streak::KeyedStreak::new(module_path!(), "Cookie bot profile state report");

pub fn report_profiles_using_proxy(proxy_id: &str) {
  report_matching(|profile| profile.proxy_id.as_deref() == Some(proxy_id));
}

pub fn report_profiles_using_vpn(vpn_id: &str) {
  report_matching(|profile| profile.vpn_id.as_deref() == Some(vpn_id));
}

fn report_matching(matches: impl Fn(&BrowserProfile) -> bool) {
  let Ok(profiles) = crate::profile::ProfileManager::instance().list_profiles() else {
    return;
  };
  for profile in profiles.iter().filter(|profile| matches(profile)) {
    report_profile_state(profile);
  }
}

pub async fn report_enrolled_profiles() {
  let auth = &crate::cloud_auth::CLOUD_AUTH;
  if !auth.is_logged_in().await || !auth.can_use_cookie_bot().await {
    return;
  }
  let list = match list_schedules(None).await {
    Ok(list) => list,
    Err(e) => {
      log::debug!("Cookie bot schedules not listed at startup err=\"{e}\"");
      return;
    }
  };
  let Ok(profiles) = crate::profile::ProfileManager::instance().list_profiles() else {
    return;
  };
  for profile in enrolled_local_profiles(&list, profiles) {
    send_profile_state(
      profile.id.to_string(),
      profile_state(&profile),
      profile_identity(&profile),
    )
    .await;
  }
}

fn enrolled_local_profiles(
  list: &CookieBotScheduleList,
  profiles: Vec<BrowserProfile>,
) -> Vec<BrowserProfile> {
  let enrolled: HashSet<&str> = list
    .schedules
    .iter()
    .map(|schedule| schedule.profile_id.as_str())
    .collect();
  profiles
    .into_iter()
    .filter(|profile| enrolled.contains(profile.id.to_string().as_str()))
    .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileIdentity {
  pub timezone: Option<String>,
  pub platform: Option<String>,
  pub profile_name: Option<String>,
}

pub fn profile_identity(profile: &BrowserProfile) -> ProfileIdentity {
  let name = profile.name.trim();
  ProfileIdentity {
    timezone: crate::fingerprint_consistency::fingerprint_timezone(profile)
      .map(|zone| zone.trim().to_string())
      .filter(|zone| !zone.is_empty()),
    platform: profile
      .remote_platform()
      .filter(|os| BOT_PLATFORMS.contains(os))
      .map(str::to_string),
    profile_name: (!name.is_empty() && name.encode_utf16().count() <= MAX_PROFILE_NAME_UNITS)
      .then(|| name.to_string()),
  }
}

fn profile_state_body(state: ProfileState, identity: &ProfileIdentity) -> serde_json::Value {
  let mut body = serde_json::Map::new();
  body.insert("sync_enabled".to_string(), state.sync_enabled.into());
  body.insert("encrypted_sync".to_string(), state.encrypted_sync.into());
  body.insert("has_proxy".to_string(), state.has_proxy.into());
  body.insert(
    "proxy_remote_reachable".to_string(),
    state.proxy_remote_reachable.into(),
  );
  body.insert(
    "touch_fingerprint".to_string(),
    state.touch_fingerprint.into(),
  );
  body.insert("sticky_exit".to_string(), state.sticky_exit.into());
  body.insert("exit_alive".to_string(), state.exit_alive.into());
  if let Some(zone) = &identity.timezone {
    body.insert("timezone".to_string(), zone.as_str().into());
  }
  if let Some(platform) = &identity.platform {
    body.insert("platform".to_string(), platform.as_str().into());
  }
  if let Some(name) = &identity.profile_name {
    body.insert("profile_name".to_string(), name.as_str().into());
  }
  serde_json::Value::Object(body)
}

pub fn forget_enrolments() {
  *enrolled_profiles() = None;
}

fn enrolled_profiles() -> std::sync::MutexGuard<'static, Option<HashSet<String>>> {
  ENROLLED_PROFILES
    .lock()
    .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn should_report(enrolled: &Option<HashSet<String>>, profile_id: &str) -> bool {
  enrolled.as_ref().is_none_or(|ids| ids.contains(profile_id))
}

fn record_enrolment(enrolled: &mut Option<HashSet<String>>, profile_id: &str, is_enrolled: bool) {
  if let Some(ids) = enrolled.as_mut() {
    if is_enrolled {
      ids.insert(profile_id.to_string());
    } else {
      ids.remove(profile_id);
    }
  }
}

fn record_own_schedules(enrolled: &mut Option<HashSet<String>>, list: &CookieBotScheduleList) {
  if list.scope.as_deref() == Some("team") {
    return;
  }
  *enrolled = Some(
    list
      .schedules
      .iter()
      .map(|schedule| schedule.profile_id.clone())
      .collect(),
  );
}

/// Turn the bot off for this profile.
pub async fn delete_schedule(profile_id: &str) -> Result<CookieBotScheduleDeleted, CookieBotError> {
  let deleted = crate::cookie_bot_storage::remove_schedule(profile_id);
  record_enrolment(&mut enrolled_profiles(), profile_id, false);
  Ok(CookieBotScheduleDeleted {
    profile_id: profile_id.to_string(),
    deleted,
  })
}

/// Ask, without writing anything, who else already warms this profile.
pub async fn check_conflicts(
  profile_id: &str,
  _run_at_minute: Option<u16>,
  _timezone: Option<&str>,
  _days_mask: Option<u8>,
) -> Result<CookieBotConflictCheck, CookieBotError> {
  Ok(CookieBotConflictCheck {
    profile_id: profile_id.to_string(),
    conflicts: Vec::new(),
  })
}

/// One page of run history, newest first.
pub async fn list_runs(
  profile_id: Option<&str>,
  _scope: Option<&str>,
  _limit: Option<u32>,
  _before: Option<&str>,
) -> Result<CookieBotRunPage, CookieBotError> {
  let mut runs = crate::cookie_bot_storage::load_runs();
  if let Some(id) = profile_id {
    runs.retain(|r| r.profile_id == id);
  }
  Ok(CookieBotRunPage {
    runs,
    next_before: None,
  })
}

/// Start a run now instead of waiting for tonight.
pub async fn run_now(
  profile_id: &str,
  max_minutes: Option<u32>,
) -> Result<CookieBotRunStarted, CookieBotError> {
  let sched = crate::cookie_bot_storage::get_schedule(profile_id);
  let sites = if let Some(ref s) = sched {
    if !s.sites.is_empty() {
      s.sites.clone()
    } else if let Some(ref tid) = s.template_id {
      crate::cookie_bot_runner::default_sites_for_template(tid)
    } else {
      crate::cookie_bot_runner::default_sites_for_template("general")
    }
  } else {
    crate::cookie_bot_runner::default_sites_for_template("general")
  };
  let max_mins = max_minutes
    .or_else(|| sched.as_ref().map(|s| s.max_minutes))
    .unwrap_or(15);
  let run_id = uuid::Uuid::new_v4().to_string();
  let now = chrono::Utc::now().to_rfc3339();
  let profile_name = sched.as_ref().map(|s| s.profile_name.clone());

  let run = CookieBotRun {
    id: run_id.clone(),
    profile_id: profile_id.to_string(),
    profile_name,
    user_id: None,
    email: None,
    team_id: None,
    trigger: "manual".to_string(),
    status: "pending".to_string(),
    scheduled_for: now.clone(),
    dispatch_after: Some(now.clone()),
    started_at: None,
    ended_at: None,
    max_minutes: max_mins,
    chunks_total: 1,
    chunk_index: 0,
    sites_total: sites.len() as u32,
    sites_visited: 0,
    sites_failed: 0,
    consent_dismissed: 0,
    billed_seconds: 0,
    outcome_code: None,
    session_id: Some(run_id.clone()),
  };

  crate::cookie_bot_storage::upsert_run(run.clone());

  if let Some(app_handle) = app_handle() {
    let pid = profile_id.to_string();
    let run_clone = run.clone();
    tauri::async_runtime::spawn(async move {
      crate::cookie_bot_runner::execute_cookie_warming(app_handle, pid, run_clone, sites, max_mins)
        .await;
    });
  } else {
    log::warn!("Cookie bot cannot run: AppHandle is not initialized yet");
  }

  Ok(CookieBotRunStarted {
    run,
    session_id: Some(run_id),
  })
}

/// Stop a run that is still going.
pub async fn cancel_run(run_id: &str) -> Result<CookieBotRun, CookieBotError> {
  crate::cookie_bot_runner::cancel_local_run(run_id);
  let mut run = crate::cookie_bot_storage::get_run(run_id).unwrap_or_else(|| CookieBotRun {
    id: run_id.to_string(),
    profile_id: String::new(),
    profile_name: None,
    user_id: None,
    email: None,
    team_id: None,
    trigger: "manual".to_string(),
    status: "cancelled".to_string(),
    scheduled_for: chrono::Utc::now().to_rfc3339(),
    dispatch_after: None,
    started_at: None,
    ended_at: Some(chrono::Utc::now().to_rfc3339()),
    max_minutes: 0,
    chunks_total: 1,
    chunk_index: 0,
    sites_total: 0,
    sites_visited: 0,
    sites_failed: 0,
    consent_dismissed: 0,
    billed_seconds: 0,
    outcome_code: Some("cancelled_by_user".to_string()),
    session_id: None,
  });
  run.status = "cancelled".to_string();
  crate::cookie_bot_storage::upsert_run(run.clone());
  Ok(run)
}

#[derive(Debug, Deserialize)]
struct RunEnvelope {
  run: CookieBotRun,
}

/// The intensities the server offers today.
pub async fn list_presets() -> Result<CookieBotPresetList, CookieBotError> {
  Ok(CookieBotPresetList {
    presets: vec![
      CookieBotPreset {
        id: "quick".to_string(),
        typical_minutes: Some(15),
        recommended: false,
        name: Some("Быстрый прогрев".to_string()),
        description: Some("15 минут, базовые поисковики и справочники".to_string()),
      },
      CookieBotPreset {
        id: "standard".to_string(),
        typical_minutes: Some(30),
        recommended: true,
        name: Some("Стандартный прогрев".to_string()),
        description: Some("30 минут, сбалансированное посещение новостей и соцсетей".to_string()),
      },
      CookieBotPreset {
        id: "deep".to_string(),
        typical_minutes: Some(60),
        recommended: false,
        name: Some("Глубокий прогрев".to_string()),
        description: Some("60 минут, широкий спектр ресурсов и e-commerce".to_string()),
      },
    ],
    templates: vec![
      CookieBotTemplate {
        id: "search-and-portals".to_string(),
        site_count: 10,
        name: Some("Поисковики и порталы".to_string()),
        description: Some("Google, Bing, DuckDuckGo, Wikipedia, Reddit".to_string()),
      },
      CookieBotTemplate {
        id: "news-and-media".to_string(),
        site_count: 8,
        name: Some("Новости и медиа".to_string()),
        description: Some("BBC, CNN, Reuters, The Verge, TechCrunch".to_string()),
      },
      CookieBotTemplate {
        id: "ecommerce".to_string(),
        site_count: 7,
        name: Some("E-Commerce и маркетплейсы".to_string()),
        description: Some("Amazon, eBay, AliExpress, Walmart, Target".to_string()),
      },
      CookieBotTemplate {
        id: "social-entertainment".to_string(),
        site_count: 6,
        name: Some("Соцсети и видео".to_string()),
        description: Some("YouTube, Twitch, Reddit, Pinterest, Imgur".to_string()),
      },
      CookieBotTemplate {
        id: "geo-ru".to_string(),
        site_count: 9,
        name: Some("Гео: Россия".to_string()),
        description: Some("Яндекс, VK, Дзен, Ozon, Wildberries, Авито, Хабр".to_string()),
      },
      CookieBotTemplate {
        id: "geo-us".to_string(),
        site_count: 8,
        name: Some("Гео: США".to_string()),
        description: Some("Google, Yahoo, CNN, NYT, Craigslist, Amazon, Reddit".to_string()),
      },
      CookieBotTemplate {
        id: "geo-eu".to_string(),
        site_count: 7,
        name: Some("Гео: Европа".to_string()),
        description: Some("Spiegel, Zeit, Le Monde, BBC, El Mundo, Booking".to_string()),
      },
      CookieBotTemplate {
        id: "tech-dev".to_string(),
        site_count: 6,
        name: Some("Tech и разработка".to_string()),
        description: Some("GitHub, Stack Overflow, Hacker News, dev.to, arXiv".to_string()),
      },
      CookieBotTemplate {
        id: "crypto-web3".to_string(),
        site_count: 6,
        name: Some("Крипто и Web3".to_string()),
        description: Some("CoinMarketCap, CoinGecko, Binance, Etherscan".to_string()),
      },
    ],
    default_preset: Some("standard".to_string()),
    limits: Some(CookieBotLimits {
      min_minutes: Some(1),
      max_minutes: Some(720),
      min_sites: Some(1),
      max_sites: Some(500),
      max_slots: Some(10),
      max_template_name_length: Some(100),
    }),
  })
}

// --- User-defined templates -------------------------------------------------
//
// The caller's own saved site lists. Unlike every other route in this file
// these are addressed by an id the SERVER minted and the client echoes back,
// so each one percent-encodes it: the id is spelled `user:<uuid>`, and a bare
// colon in a path segment is a spelling the router is free to read differently.

#[derive(Debug, Deserialize)]
struct UserTemplateListEnvelope {
  #[serde(default)]
  templates: Vec<CookieBotUserTemplate>,
}

#[derive(Debug, Deserialize)]
struct UserTemplateEnvelope {
  template: CookieBotUserTemplate,
}

#[derive(Debug, Deserialize)]
struct UserTemplateDeleted {
  #[serde(default)]
  deleted: bool,
}

/// Every site list this user has saved, most recently edited first.
pub async fn list_user_templates() -> Result<Vec<CookieBotUserTemplate>, CookieBotError> {
  Ok(crate::cookie_bot_storage::load_user_templates())
}

/// Save a new one.
pub async fn create_user_template(
  name: &str,
  sites: &[String],
) -> Result<CookieBotUserTemplate, CookieBotError> {
  let id = format!("user:{}", uuid::Uuid::new_v4());
  let template = CookieBotUserTemplate {
    id,
    name: name.to_string(),
    sites: sites.to_vec(),
    updated_at: Some(chrono::Utc::now().to_rfc3339()),
  };
  let mut templates = crate::cookie_bot_storage::load_user_templates();
  templates.push(template.clone());
  crate::cookie_bot_storage::save_user_templates(&templates);
  Ok(template)
}

/// Rename one, replace its sites, or both.
pub async fn update_user_template(
  id: &str,
  name: Option<&str>,
  sites: Option<&[String]>,
) -> Result<CookieBotUserTemplate, CookieBotError> {
  let mut templates = crate::cookie_bot_storage::load_user_templates();
  let pos = templates.iter().position(|t| t.id == id).ok_or_else(|| {
    CookieBotError(cloud_errors::BackendFailure {
      status: 404,
      code: "COOKIE_BOT_TEMPLATE_NOT_FOUND".into(),
      params: std::collections::BTreeMap::new(),
    })
  })?;
  if let Some(n) = name {
    templates[pos].name = n.to_string();
  }
  if let Some(s) = sites {
    templates[pos].sites = s.to_vec();
  }
  templates[pos].updated_at = Some(chrono::Utc::now().to_rfc3339());
  let updated = templates[pos].clone();
  crate::cookie_bot_storage::save_user_templates(&templates);
  Ok(updated)
}

/// Delete one. Enrolments that used it keep the sites they copied, so this is
/// never a way to stop a profile being warmed tonight.
pub async fn delete_user_template(id: &str) -> Result<bool, CookieBotError> {
  let mut templates = crate::cookie_bot_storage::load_user_templates();
  let initial = templates.len();
  templates.retain(|t| t.id != id);
  let deleted = templates.len() < initial;
  if deleted {
    crate::cookie_bot_storage::save_user_templates(&templates);
  }
  Ok(deleted)
}

// --- Tauri commands ---------------------------------------------------------
//
// The user-template commands live here rather than in `lib.rs` beside the
// schedule ones because they carry no local precondition: nothing about a saved
// site list depends on a profile this machine holds, so there is no profile to
// look up and no `bot_precondition` to apply. They must still be registered in
// `lib.rs`'s `invoke_handler` to be reachable.

/// Log a refusal and hand the frontend the envelope it translates.
///
/// The raw HTTP text never reaches the user: an untranslated backend sentence
/// in a Japanese UI is the failure the `{"code":…}` convention exists to stop.
fn command_error(context: &str, err: CookieBotError) -> String {
  log::warn!(
    "Cookie bot request failed action=\"{context}\" code={} status={}",
    err.code(),
    err.status()
  );
  err.to_error_json()
}

/// Every site list this user has saved.
#[tauri::command]
pub async fn get_cookie_bot_user_templates() -> Result<Vec<CookieBotUserTemplate>, String> {
  list_user_templates()
    .await
    .map_err(|e| command_error("template list", e))
}

/// Save the current site list under a name.
#[tauri::command]
pub async fn create_cookie_bot_user_template(
  name: String,
  sites: Vec<String>,
) -> Result<CookieBotUserTemplate, String> {
  create_user_template(&name, &sites)
    .await
    .map_err(|e| command_error("template create", e))
}

/// Rename a saved list, replace its sites, or both. Omitted fields are left
/// exactly as they are.
#[tauri::command]
pub async fn update_cookie_bot_user_template(
  id: String,
  name: Option<String>,
  sites: Option<Vec<String>>,
) -> Result<CookieBotUserTemplate, String> {
  update_user_template(&id, name.as_deref(), sites.as_deref())
    .await
    .map_err(|e| command_error("template update", e))
}

/// Delete a saved list. `false` means there was nothing left to delete.
#[tauri::command]
pub async fn delete_cookie_bot_user_template(id: String) -> Result<bool, String> {
  delete_user_template(&id)
    .await
    .map_err(|e| command_error("template delete", e))
}

/// Per-member and per-profile spend for a calendar month (`YYYY-MM`).
pub async fn team_usage(period: Option<&str>) -> Result<CookieBotUsage, CookieBotError> {
  Ok(CookieBotUsage {
    period: period.unwrap_or("current").to_string(),
    period_start: None,
    period_end: None,
    team_id: None,
    seats: 1,
    granted_hours: 9999.0,
    used_hours: 0.0,
    remaining_hours: 9999.0,
    members: Vec::new(),
    profiles: Vec::new(),
  })
}

/// The pooled remote-hour budget: unlimited for local cookie bot.
pub async fn remote_hours_quota() -> Result<RemoteHoursQuota, CookieBotError> {
  Ok(RemoteHoursQuota {
    granted_hours: 9999.0,
    remaining_hours: 9999.0,
    used_hours: 0.0,
    period_start: None,
    period_end: None,
    scope: Some("local".to_string()),
    team_id: None,
    seats: 1,
    per_seat_hours: 9999.0,
    breakdown: None,
    members: Vec::new(),
  })
}

// --- Transport --------------------------------------------------------------

fn http() -> reqwest::Client {
  static CLIENT: crate::app_proxy::SharedClient = crate::app_proxy::SharedClient::new(|builder| {
    builder
      .timeout(REQUEST_TIMEOUT)
      .connect_timeout(CONNECT_TIMEOUT)
  });
  CLIENT.get()
}

/// Append a percent-encoded query string.
///
/// Built here rather than left to the HTTP client so a profile id or a keyset
/// cursor containing a `&` cannot smuggle a second parameter into the request.
pub(crate) fn with_query(url: &str, query: &[(String, String)]) -> String {
  if query.is_empty() {
    return url.to_string();
  }
  let encoded = query
    .iter()
    .map(|(key, value)| {
      format!(
        "{}={}",
        urlencoding::encode(key),
        urlencoding::encode(value)
      )
    })
    .collect::<Vec<_>>()
    .join("&");
  let separator = if url.contains('?') { '&' } else { '?' };
  format!("{url}{separator}{encoded}")
}

/// One request, one place.
///
/// Goes through `api_call_with_retry` so an expired access token is refreshed
/// and the call retried once — otherwise a user whose token aged out overnight
/// sees "not signed in" on a machine that is signed in.
async fn request<T: DeserializeOwned>(
  method: reqwest::Method,
  url: String,
  query: Vec<(String, String)>,
  body: Option<serde_json::Value>,
  codes: FailureCodes,
) -> Result<T, CookieBotError> {
  crate::cloud_auth::CLOUD_AUTH
    .api_call_with_retry(|token| {
      let method = method.clone();
      let url = url.clone();
      let query = query.clone();
      let body = body.clone();
      async move {
        let url = with_query(&url, &query);
        let mut builder = http().request(method, &url).bearer_auth(token);
        if let Some(payload) = body {
          builder = builder.json(&payload);
        }

        let response = builder
          .send()
          .await
          .map_err(|e| format!("reach backend: {e}"))?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
          let text = response.text().await.unwrap_or_default();
          // Encode the status so api_call_with_retry can spot a 401 and
          // classify_message can recover the code afterwards.
          return Err(format!("({status}) {text}"));
        }

        response
          .json::<T>()
          .await
          .map_err(|e| format!("decode response: {e}"))
      }
    })
    .await
    .map_err(|e| CookieBotError(cloud_errors::classify_message(&e, codes)))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::profile::types::SyncMode;
  use crate::remote_exit::ExitReachability;
  use crate::sync::encryption::RemoteSeal;

  fn eligible_profile() -> BrowserProfile {
    BrowserProfile {
      id: uuid::Uuid::nil(),
      name: "warm me".to_string(),
      browser: "wayfern".to_string(),
      version: "latest".to_string(),
      sync_mode: SyncMode::Regular,
      host_os: Some("macos".to_string()),
      proxy_id: Some("proxy-1".to_string()),
      ..Default::default()
    }
  }

  fn profile_in_zone(zone: &str) -> BrowserProfile {
    let mut profile = eligible_profile();
    profile.wayfern_config = Some(crate::wayfern_manager::WayfernConfig {
      fingerprint: Some(serde_json::json!({ "timezone": zone, "language": "de-DE" }).to_string()),
      ..Default::default()
    });
    profile
  }

  #[test]
  fn a_report_names_the_zone_the_platform_and_the_profile() {
    // A Wayfern profile created on macOS runs on a Linux host.
    let identity = profile_identity(&profile_in_zone("Europe/Berlin"));
    assert_eq!(
      identity,
      ProfileIdentity {
        timezone: Some("Europe/Berlin".to_string()),
        platform: Some("linux".to_string()),
        profile_name: Some("warm me".to_string()),
      }
    );

    let body = profile_state_body(
      profile_state_from(&eligible_profile(), false, true),
      &identity,
    );
    assert_eq!(body["timezone"], "Europe/Berlin");
    assert_eq!(body["platform"], "linux");
    assert_eq!(body["profile_name"], "warm me");
    assert_eq!(body["has_proxy"], true);
    assert_eq!(body["sync_enabled"], true);
  }

  #[test]
  fn a_report_leaves_out_what_the_server_would_refuse() {
    let mut profile = eligible_profile();
    profile.host_os = Some("android".to_string());
    profile.name = "n".repeat(MAX_PROFILE_NAME_UNITS + 1);
    let identity = profile_identity(&profile);
    assert_eq!(identity, ProfileIdentity::default());

    let body = profile_state_body(profile_state_from(&profile, false, true), &identity);
    for key in ["timezone", "platform", "profile_name"] {
      assert!(body.get(key).is_none(), "{key} must be omitted");
    }
    assert!(body.get("sync_enabled").is_some());
  }

  #[test]
  fn a_profile_name_is_measured_the_way_the_server_measures_it() {
    let mut profile = eligible_profile();
    profile.name = "😀".repeat(MAX_PROFILE_NAME_UNITS / 2);
    assert!(profile_identity(&profile).profile_name.is_some());
    profile.name = "😀".repeat(MAX_PROFILE_NAME_UNITS / 2 + 1);
    assert!(profile_identity(&profile).profile_name.is_none());
    profile.name = "   ".to_string();
    assert!(profile_identity(&profile).profile_name.is_none());
  }

  fn schedule_list(scope: Option<&str>, ids: &[&str]) -> CookieBotScheduleList {
    let schedules = ids
      .iter()
      .map(|id| {
        serde_json::from_value(serde_json::json!({
          "profile_id": id, "profile_name": "p", "platform": "macos", "enabled": true,
          "run_at_minute": 0, "days_mask": 1, "timezone": "UTC", "preset": "light",
          "max_minutes": 20
        }))
        .expect("a minimal schedule")
      })
      .collect();
    CookieBotScheduleList {
      schedules,
      team_id: None,
      scope: scope.map(str::to_string),
    }
  }

  #[test]
  fn next_run_at_resolves_the_earliest_slot_ahead_of_now() {
    let mut schedule: CookieBotSchedule = serde_json::from_value(serde_json::json!({
      "profile_id": "p", "profile_name": "p", "platform": "macos", "enabled": true,
      "run_at_minute": 600, "days_mask": 127, "timezone": "UTC", "preset": "light",
      "max_minutes": 20,
      "slots": [{"days_mask": 127, "run_at_minute": 600}]
    }))
    .expect("a schedule");
    let now = chrono::DateTime::parse_from_rfc3339("2026-11-20T10:00:00Z")
      .expect("a fixed now")
      .with_timezone(&chrono::Utc);

    // The daily 10:00 anchor is exactly now, so the next fire is tomorrow.
    let next = next_run_at_for(&schedule, now).expect("a daily anchor");
    assert_eq!(
      chrono::DateTime::parse_from_rfc3339(&next),
      chrono::DateTime::parse_from_rfc3339("2026-11-21T10:00:00Z")
    );

    // 2026-11-20 is a Friday (bit 4); a 01:00 Friday slot that already fired
    // this morning rolls a whole week ahead.
    schedule.slots = vec![CookieBotSlot {
      days_mask: 1 << 4,
      run_at_minute: 60,
    }];
    let next = next_run_at_for(&schedule, now).expect("a weekly anchor");
    assert_eq!(
      chrono::DateTime::parse_from_rfc3339(&next),
      chrono::DateTime::parse_from_rfc3339("2026-11-27T01:00:00Z")
    );

    // An unparsable zone gives up instead of panicking.
    schedule.timezone = "Not/AZone".to_string();
    assert_eq!(next_run_at_for(&schedule, now), None);
  }

  #[test]
  fn only_enrolled_profiles_are_reported_once_the_enrolments_are_known() {
    let mut enrolled = None;
    assert!(should_report(&enrolled, "any"));

    record_own_schedules(&mut enrolled, &schedule_list(Some("mine"), &["p1"]));
    assert!(should_report(&enrolled, "p1"));
    assert!(!should_report(&enrolled, "p2"));

    record_enrolment(&mut enrolled, "p2", true);
    assert!(should_report(&enrolled, "p2"));
    record_enrolment(&mut enrolled, "p1", false);
    assert!(!should_report(&enrolled, "p1"));
  }

  #[test]
  fn a_team_listing_does_not_replace_the_callers_own_enrolments() {
    let mut enrolled = None;
    record_own_schedules(&mut enrolled, &schedule_list(Some("mine"), &["mine-1"]));
    record_own_schedules(
      &mut enrolled,
      &schedule_list(Some("team"), &["mine-1", "teammate-1"]),
    );
    assert!(should_report(&enrolled, "mine-1"));
    assert!(!should_report(&enrolled, "teammate-1"));
  }

  #[test]
  fn an_unknown_enrolment_list_is_not_filled_in_by_single_answers() {
    let mut enrolled = None;
    record_enrolment(&mut enrolled, "p1", false);
    assert!(enrolled.is_none());
    assert!(should_report(&enrolled, "p1"));
  }

  #[test]
  fn startup_reports_every_enrolled_profile_this_machine_has() {
    let mut here = eligible_profile();
    here.id = uuid::Uuid::from_u128(1);
    let mut elsewhere = eligible_profile();
    elsewhere.id = uuid::Uuid::from_u128(2);
    let list = schedule_list(Some("mine"), &[&here.id.to_string(), "not-on-this-machine"]);
    let picked = enrolled_local_profiles(&list, vec![here.clone(), elsewhere]);
    assert_eq!(picked.len(), 1);
    assert_eq!(picked[0].id, here.id);
  }

  #[test]
  fn uploads_and_startup_re_declare_the_profile_state() {
    let engine = include_str!("sync/engine.rs");
    let lib = include_str!("lib.rs");
    let upload_proxy = engine
      .split("async fn upload_proxy(")
      .nth(1)
      .expect("upload_proxy must exist");
    assert!(upload_proxy[..upload_proxy.find("\n  }").unwrap()]
      .contains("crate::cookie_bot::report_profiles_using_proxy(&proxy.id);"));
    let upload_vpn = engine
      .split("async fn upload_vpn(")
      .nth(1)
      .expect("upload_vpn must exist");
    assert!(upload_vpn[..upload_vpn.find("\n  }").unwrap()]
      .contains("crate::cookie_bot::report_profiles_using_vpn(&vpn.id);"));
    let profile_sync = engine
      .split("async fn sync_profile_inner(")
      .nth(1)
      .expect("sync_profile_inner must exist");
    let profile_sync = &profile_sync[..profile_sync.find("\n  }").unwrap()];
    let uploaded = profile_sync
      .find(".upload_manifest(")
      .expect("the manifest upload");
    let reported = profile_sync
      .find("crate::cookie_bot::report_profile_state(&updated_profile);")
      .expect("the report after a finished upload");
    assert!(uploaded < reported);
    assert!(lib.contains("tauri::async_runtime::spawn(cookie_bot::report_enrolled_profiles());"));
    let auth = include_str!("cloud_auth.rs");
    let clear = auth
      .split("async fn clear_auth(&self)")
      .nth(1)
      .expect("clear_auth must exist");
    assert!(
      clear[..clear.find("\n  }").unwrap()].contains("crate::cookie_bot::forget_enrolments();")
    );
  }

  #[test]
  fn the_profile_facts_the_server_requires_are_derived_from_the_profile() {
    let state = profile_state_from(&eligible_profile(), false, true);

    assert!(state.sync_enabled);
    assert!(!state.encrypted_sync);
    assert!(state.has_proxy);
    assert!(!state.touch_fingerprint);
    // A stored proxy is not claimed to be sticky — only a VPN tunnel is.
    assert!(!state.sticky_exit);
  }

  #[test]
  fn a_vpn_counts_as_the_exit_node_and_as_a_sticky_one() {
    // `has_proxy` asks whether the user brought an exit, not whether that exit
    // is specifically a stored proxy. A VPN-only profile that reported false
    // would be refused a run it is perfectly entitled to.
    let profile = BrowserProfile {
      proxy_id: None,
      vpn_id: Some("vpn-1".to_string()),
      ..eligible_profile()
    };

    let state = profile_state_from(&profile, false, true);

    assert!(state.has_proxy);
    assert!(state.sticky_exit);
  }

  #[test]
  fn a_profile_with_no_exit_reports_none() {
    let profile = BrowserProfile {
      proxy_id: None,
      vpn_id: None,
      ..eligible_profile()
    };

    assert!(!profile_state_from(&profile, false, true).has_proxy);
  }

  fn code_of(err: &str) -> String {
    serde_json::from_str::<serde_json::Value>(err)
      .expect("a precondition failure must be a JSON error envelope")["code"]
      .as_str()
      .expect("the envelope must name a code")
      .to_string()
  }

  #[test]
  fn a_local_only_profile_passes_bot_precondition() {
    let mut profile = eligible_profile();
    profile.sync_mode = SyncMode::Disabled;
    assert!(bot_precondition(&profile, &ExitReachability::Remote).is_ok());
  }

  #[test]
  fn end_to_end_encrypted_sync_passes_bot_precondition() {
    let mut profile = eligible_profile();
    profile.sync_mode = SyncMode::Encrypted;
    assert!(bot_precondition(&profile, &ExitReachability::Remote).is_ok());
  }

  #[test]
  fn a_sync_password_seals_a_profile_on_regular_sync_too() {
    // The password seals the metadata and the proxy of every synced profile,
    // so a host could read neither, whatever the profile's own sync mode.
    let profile = eligible_profile();
    assert!(profile_state_from(&profile, true, true).encrypted_sync);
    assert!(!profile_state_from(&profile, false, true).encrypted_sync);
    assert_eq!(remote_seal(&profile, true), Some(RemoteSeal::SyncPassword));
    assert_eq!(remote_seal(&profile, false), None);

    let mut local = eligible_profile();
    local.sync_mode = SyncMode::Disabled;
    assert_eq!(remote_seal(&local, true), None, "nothing of it is uploaded");
  }

  #[test]
  fn a_confirmed_dead_exit_is_declared() {
    let state = profile_state_from(&eligible_profile(), false, false);
    assert!(!state.exit_alive);
    let input = CookieBotScheduleInput::default().with_profile_state(state);
    assert!(!input.exit_alive);
    assert_eq!(
      profile_state_body(state, &ProfileIdentity::default())["exit_alive"],
      false
    );
  }

  #[test]
  fn all_platforms_and_exit_configurations_pass_bot_precondition_locally() {
    let mut profile = eligible_profile();
    profile.browser = "camoufox".to_string();
    profile.host_os = Some("android".to_string());
    assert!(bot_precondition(&profile, &ExitReachability::Remote).is_ok());

    profile.host_os = Some("linux".to_string());
    assert!(bot_precondition(&profile, &ExitReachability::Remote).is_ok());

    profile.proxy_id = None;
    profile.vpn_id = None;
    assert!(bot_precondition(&profile, &ExitReachability::None).is_ok());

    for level in ["none", "light", "pro_plus", "ultimate", "custom"] {
      profile.dns_blocklist = Some(level.to_string());
      assert!(bot_precondition(&profile, &ExitReachability::Remote).is_ok());
    }

    profile.host_os = None;
    assert!(bot_precondition(&profile, &ExitReachability::Remote).is_ok());

    assert!(bot_precondition(
      &eligible_profile(),
      &ExitReachability::LocalOnly {
        host: "127.0.0.1".to_string(),
        source: "proxy",
      },
    )
    .is_ok());

    assert!(bot_precondition(
      &eligible_profile(),
      &ExitReachability::UnsupportedKind {
        kind: "VLESS".to_string(),
        source: "proxy",
      },
    )
    .is_ok());

    assert!(bot_precondition(
      &eligible_profile(),
      &ExitReachability::Unknown {
        reason: "VPN config could not be parsed".to_string(),
        source: "VPN",
      },
    )
    .is_ok());
  }

  /// A verbatim schedule payload, field for field, as the cloud API sends it.
  const SERVER_SCHEDULE_VIEW: &str = r#"{
    "profile_id":"p1","profile_name":"Yu","platform":"macos","enabled":true,
    "run_at_minute":120,"days_mask":127,
    "slots":[{"days_mask":127,"run_at_minute":120},{"days_mask":31,"run_at_minute":690}],
    "timezone":"Europe/Berlin","template_id":null,
    "preset":"balanced","max_minutes":45,"sites":["https://example.com"],
    "jitter_seconds":900,"sync_enabled":true,"encrypted_sync":false,
    "has_proxy":true,"touch_fingerprint":false,"sticky_exit":false,
    "profile_state_at":"2026-08-03T09:00:00.000Z","next_run_at":"2026-08-04T00:00:00.000Z",
    "last_run_at":null,"last_run_id":null,"blocked_by":null,"owner_user_id":"u1",
    "owner_email":"a@example.com","updated_at":"2026-08-03T10:00:00.000Z"
  }"#;

  #[test]
  fn the_schedule_payload_matches_what_the_backend_sends() {
    // Pinned against the schedule shape the cloud API serves. A field name
    // that drifts makes every read fail at the decode step, which surfaces as
    // "something went wrong" with no hint that the contract moved.
    let schedule: CookieBotSchedule = serde_json::from_str(SERVER_SCHEDULE_VIEW)
      .expect("the backend's schedule payload must deserialize");

    assert_eq!(schedule.run_at_minute, 120);
    assert_eq!(schedule.days_mask, 127);
    assert_eq!(schedule.max_minutes, 45);
    assert_eq!(schedule.sites, vec!["https://example.com".to_string()]);
    assert!(schedule.last_run_at.is_none());
    // The declared facts, echoed back. Dropped, they left the UI unable to see
    // that the server's copy of a profile no longer matched this machine's.
    assert!(schedule.sync_enabled);
    assert!(schedule.has_proxy);
    assert!(!schedule.encrypted_sync);
    assert!(schedule.profile_state_at.is_some());
    assert!(schedule.blocked_by.is_none());
  }

  #[test]
  fn a_schedule_carries_its_whole_calendar_not_just_the_first_time() {
    // The mirrored pair is `slots[0]`, so a client that read only the pair
    // would show "every night at 02:00" for an enrolment that also runs at
    // 11:30 on weeknights — fewer runs than the user booked, silently.
    let schedule: CookieBotSchedule =
      serde_json::from_str(SERVER_SCHEDULE_VIEW).expect("a multi-slot schedule must deserialize");

    assert_eq!(schedule.slots.len(), 2);
    assert_eq!(schedule.slots[0].run_at_minute, schedule.run_at_minute);
    assert_eq!(schedule.slots[0].days_mask, schedule.days_mask);
    assert_eq!(schedule.slots[1].run_at_minute, 690);
    assert_eq!(schedule.slots[1].days_mask, 31);
  }

  #[test]
  fn a_server_that_predates_multi_slot_still_decodes_with_no_slots() {
    // `slots` absent is a deployment that has not rolled forward, not a broken
    // enrolment. Requiring it would blank the whole Cookie Bot surface against
    // an older backend rather than show the one time it does know about.
    let schedule: CookieBotSchedule = serde_json::from_str(
      r#"{"profile_id":"p1","profile_name":"Yu","platform":"windows","enabled":true,
          "run_at_minute":120,"days_mask":31,"timezone":"UTC","preset":"light",
          "max_minutes":10}"#,
    )
    .expect("a pre-multi-slot schedule must deserialize");

    assert!(schedule.slots.is_empty());
    assert!(schedule.template_id.is_none());
  }

  #[test]
  fn a_templated_enrolment_reports_its_template_and_no_sites() {
    // A built-in template's URLs are server-owned. An empty `sites` here is the
    // contract working, not a schedule with nothing to browse — anything that
    // reads it as "no sites" would show a healthy enrolment as broken.
    let schedule: CookieBotSchedule = serde_json::from_str(
      &SERVER_SCHEDULE_VIEW
        .replace(
          "\"template_id\":null",
          "\"template_id\":\"low-intent-purchaser\"",
        )
        .replace("\"sites\":[\"https://example.com\"]", "\"sites\":[]"),
    )
    .expect("a templated schedule must deserialize");

    assert_eq!(
      schedule.template_id.as_deref(),
      Some("low-intent-purchaser")
    );
    assert!(schedule.sites.is_empty());
    assert!(schedule.blocked_by.is_none());
  }

  #[test]
  fn a_calendar_is_sent_as_slots_and_omitted_entirely_when_there_is_none() {
    // The server reads an ABSENT `slots` as "one slot, from the legacy pair"
    // and REFUSES a null or empty one. Serialising `None` as null would 400
    // every write from a form with a single time on it.
    let one_slot = CookieBotScheduleInput {
      profile_name: "Yu".to_string(),
      platform: "macos".to_string(),
      enabled: true,
      run_at_minute: 120,
      days_mask: 127,
      timezone: "Europe/Berlin".to_string(),
      preset: "balanced".to_string(),
      max_minutes: 45,
      sites: vec!["https://example.com".to_string()],
      ..Default::default()
    };
    let encoded = serde_json::to_value(&one_slot).expect("input must serialize");
    assert!(
      encoded.get("slots").is_none(),
      "an absent calendar must be absent on the wire, not null"
    );
    assert!(encoded.get("template_id").is_none());

    let many = CookieBotScheduleInput {
      slots: Some(vec![
        CookieBotSlot {
          days_mask: 127,
          run_at_minute: 120,
        },
        CookieBotSlot {
          days_mask: 31,
          run_at_minute: 690,
        },
      ]),
      ..one_slot
    };
    let encoded = serde_json::to_value(&many).expect("input must serialize");
    let slots = encoded["slots"].as_array().expect("slots must be a list");
    assert_eq!(slots.len(), 2);
    // Mirrored, because a server that predates multi-slot ignores `slots` and
    // stores this pair. Dropping it would leave that server with no time at all.
    assert_eq!(encoded["run_at_minute"], 120);
    assert_eq!(encoded["days_mask"], 127);
  }

  #[test]
  fn a_templated_write_names_the_template_and_sends_no_sites() {
    // The server refuses a body carrying both: a curated persona merged with
    // the user's own list is neither.
    let input = CookieBotScheduleInput {
      profile_name: "Yu".to_string(),
      platform: "macos".to_string(),
      enabled: true,
      run_at_minute: 120,
      days_mask: 127,
      timezone: "UTC".to_string(),
      preset: "balanced".to_string(),
      max_minutes: 45,
      sites: Vec::new(),
      template_id: Some("low-intent-purchaser".to_string()),
      ..Default::default()
    };
    let encoded = serde_json::to_value(&input).expect("input must serialize");
    assert_eq!(encoded["template_id"], "low-intent-purchaser");
    assert_eq!(
      encoded["sites"].as_array().map(Vec::len),
      Some(0),
      "sites must still be sent, and must be empty, beside a template"
    );
  }

  #[test]
  fn a_broken_enrolment_carries_the_reason_it_cannot_run() {
    // The whole point of `blocked_by`: a profile whose proxy was detached in
    // the afternoon should not first announce itself with a skipped run at
    // 02:00. Dropping the field made the enrolment look healthy until it
    // failed.
    let schedule: CookieBotSchedule = serde_json::from_str(
      &SERVER_SCHEDULE_VIEW
        .replace("\"has_proxy\":true", "\"has_proxy\":false")
        .replace("\"blocked_by\":null", "\"blocked_by\":\"proxy_required\""),
    )
    .expect("a blocked schedule must deserialize");

    assert!(!schedule.has_proxy);
    assert_eq!(schedule.blocked_by.as_deref(), Some("proxy_required"));
  }

  #[test]
  fn a_schedule_missing_every_optional_field_still_decodes() {
    // A freshly created enrolment has never run, so the backend omits or
    // nulls half the object. Failing to decode that would make the enrolment
    // the user just made look broken.
    let schedule: CookieBotSchedule = serde_json::from_str(
      r#"{"profile_id":"p1","profile_name":"Yu","platform":"windows","enabled":false,
          "run_at_minute":0,"days_mask":1,"timezone":"UTC","preset":"light",
          "max_minutes":5}"#,
    )
    .expect("a never-run schedule must deserialize");
    assert!(schedule.sites.is_empty());
    assert_eq!(schedule.jitter_seconds, 0);
    assert!(schedule.next_run_at.is_none());
  }

  #[test]
  fn the_run_payload_matches_what_the_backend_sends() {
    // Verbatim run payload, exactly as the cloud API serves it. `max_minutes`,
    // `chunks_total`, `chunk_index`, `dispatch_after` and `team_id` were all
    // already on the wire and all silently discarded, so a multi-chunk night
    // could not be reported as one.
    let page: CookieBotRunPage = serde_json::from_str(
      r#"{"runs":[{"id":"r1","profile_id":"p1","profile_name":"Yu","user_id":"u1",
          "email":"a@example.com","team_id":"t1","trigger":"schedule","status":"running",
          "scheduled_for":"2026-08-03T00:00:00.000Z",
          "dispatch_after":"2026-08-03T00:07:30.000Z","started_at":"2026-08-03T00:08:00.000Z",
          "ended_at":null,"max_minutes":180,"chunks_total":3,"chunk_index":2,
          "sites_total":12,"sites_visited":11,
          "sites_failed":1,"consent_dismissed":4,"billed_seconds":2220,
          "outcome_code":null,"session_id":"s1"}],"next_before":null}"#,
    )
    .expect("the backend's run page must deserialize");

    let run = &page.runs[0];
    assert_eq!(run.status, "running");
    assert_eq!(run.billed_seconds, 2220);
    assert_eq!(run.sites_visited, 11);
    assert_eq!(run.team_id.as_deref(), Some("t1"));
    assert_eq!(run.max_minutes, 180);
    assert_eq!(run.chunks_total, 3);
    assert_eq!(run.chunk_index, 2);
    assert!(run.dispatch_after.is_some());
    assert!(page.next_before.is_none());
  }

  #[test]
  fn the_usage_report_keeps_the_per_profile_failure_count() {
    // `runs_failed` sits beside `runs` on the wire and is the one number that
    // answers "is this enrolment actually working?" in the team dashboard.
    let usage: CookieBotUsage = serde_json::from_str(
      r#"{"period":"2026-08","period_start":"2026-08-01T00:00:00.000Z",
          "period_end":"2026-09-01T00:00:00.000Z","team_id":"t1","seats":2,
          "granted_hours":400,"used_hours":12.5,"remaining_hours":387.5,
          "members":[],
          "profiles":[{"profile_id":"p1","profile_name":"Yu","owner_email":"a@example.com",
            "bot_hours":12.5,"runs":9,"runs_failed":4,
            "last_run_at":"2026-08-03T00:41:00.000Z","last_status":"failed"}]}"#,
    )
    .expect("the usage report must deserialize");

    assert_eq!(usage.profiles[0].runs, 9);
    assert_eq!(usage.profiles[0].runs_failed, 4);
  }

  #[test]
  fn the_profile_state_body_declares_every_fact_the_server_gates_on() {
    // The narrow re-declaration route. `has_proxy` false is `proxy_required`
    // server-side, so an omitted field silently keeps the stale value and the
    // night runs unproxied.
    let state = profile_state_from(&eligible_profile(), false, true);
    let body = profile_state_body(state, &ProfileIdentity::default());
    for key in [
      "sync_enabled",
      "encrypted_sync",
      "has_proxy",
      "proxy_remote_reachable",
      "touch_fingerprint",
      "sticky_exit",
      "exit_alive",
    ] {
      assert!(
        body.get(key).is_some_and(serde_json::Value::is_boolean),
        "{key} must be declared, not left to the server's stale copy"
      );
    }
  }

  #[test]
  fn the_gui_payload_deserialises_without_the_facts_the_command_stamps() {
    // This is exactly what `saveCookieBotSchedule` in src/lib/cookie-bot.ts
    // sends: the user's own scalars and nothing else. Requiring the profile
    // facts here made Tauri reject the argument with
    // `invalid args schedule: missing field sync_enabled` before
    // `with_profile_state` ever ran, so every enrolment from the GUI failed
    // while REST and MCP — which build the struct in Rust — worked.
    let input: CookieBotScheduleInput = serde_json::from_str(
      r#"{"profile_name":"Yu","platform":"macos","enabled":true,"run_at_minute":120,
          "days_mask":127,"timezone":"Europe/Berlin","preset":"balanced",
          "max_minutes":45,"sites":["https://example.com"]}"#,
    )
    .expect("the frontend's schedule payload must deserialize");

    // Fail-closed: an input nobody stamped claims no sync and no exit node, and
    // the server refuses both. The dangerous default would be the other way.
    assert!(!input.sync_enabled);
    assert!(!input.has_proxy);

    let stamped = input.with_profile_state(profile_state_from(&eligible_profile(), false, true));
    assert!(stamped.sync_enabled);
    assert!(stamped.has_proxy);
  }

  #[test]
  fn a_schedule_input_serialises_without_a_next_run_at() {
    // The server always recomputes the next fire instant. Sending one would
    // invite a client and a server that disagree about when the bot runs.
    let input = CookieBotScheduleInput {
      profile_name: "Yu".to_string(),
      platform: "macos".to_string(),
      enabled: true,
      run_at_minute: 120,
      days_mask: 127,
      timezone: "Europe/Berlin".to_string(),
      preset: "balanced".to_string(),
      max_minutes: 45,
      sites: vec!["https://example.com".to_string()],
      jitter_seconds: None,
      ..Default::default()
    };
    // The server rejects a write missing either of these with
    // COOKIE_BOT_INVALID_SCHEDULE, so they must be on the wire unconditionally
    // — `skip_serializing_if` on them would make every enrolment a 400.
    let encoded = serde_json::to_value(&input).expect("input must serialize");
    assert!(encoded.get("sync_enabled").is_some());
    assert!(encoded.get("has_proxy").is_some());
    assert!(encoded.get("next_run_at").is_none());
    assert!(
      encoded.get("jitter_seconds").is_none(),
      "an unset jitter must be omitted so the server's default applies"
    );
  }

  #[test]
  fn the_quota_payload_survives_a_backend_that_only_sends_the_original_two_keys() {
    // The route predates this feature and returned only these two fields.
    // A deployment that has not rolled forward must still render a budget.
    let quota: RemoteHoursQuota =
      serde_json::from_str(r#"{"granted_hours":200,"remaining_hours":187.25}"#)
        .expect("the legacy quota payload must deserialize");
    assert_eq!(quota.granted_hours, 200.0);
    assert_eq!(quota.seats, 0);
    assert!(quota.members.is_empty());
  }

  #[test]
  fn a_pooled_team_quota_decodes_its_roster() {
    let quota: RemoteHoursQuota = serde_json::from_str(
      r#"{"granted_hours":600,"remaining_hours":0,"used_hours":612.75,"scope":"team",
          "team_id":"t1","seats":3,"per_seat_hours":200,
          "breakdown":{"interactive_hours":100.5,"bot_hours":512.25},
          "members":[{"user_id":"u1","email":"a@example.com","role":"owner",
                      "used_hours":400,"interactive_hours":90,"bot_hours":310}]}"#,
    )
    .expect("the pooled quota payload must deserialize");

    // used_hours is deliberately unclamped while remaining_hours is: an
    // over-spent team must be able to see how far over it went.
    assert_eq!(quota.used_hours, 612.75);
    assert_eq!(quota.remaining_hours, 0.0);
    assert_eq!(quota.seats, 3);
    assert_eq!(quota.members.len(), 1);
    assert_eq!(
      quota.breakdown.map(|b| b.bot_hours),
      Some(512.25),
      "the bot/interactive split is reporting only, but it must survive the wire"
    );
  }

  #[test]
  fn a_conflict_response_carries_the_teammate_the_ui_has_to_name() {
    let saved: CookieBotScheduleSaved = serde_json::from_str(
      r#"{"schedule":{"profile_id":"p1","profile_name":"Yu","platform":"macos",
            "enabled":true,"run_at_minute":120,"days_mask":127,"timezone":"UTC",
            "preset":"deep","max_minutes":90},
          "conflicts":[{"user_id":"u2","email":"alex@example.com","run_at_minute":120,
            "timezone":"UTC","days_mask":127,"enabled":true}]}"#,
    )
    .expect("an acknowledged write must still report the conflict");
    assert_eq!(saved.conflicts[0].email, "alex@example.com");
    assert!(!saved.conflicts[0].overlaps);
  }

  #[test]
  fn errors_render_as_the_envelope_the_frontend_translates() {
    let err = CookieBotError(cloud_errors::classify_message(
      r#"(403) {"code":"COOKIE_BOT_NOT_ENTITLED"}"#,
      SCHEDULE_CODES,
    ));
    assert_eq!(err.code(), "COOKIE_BOT_NOT_ENTITLED");
    assert_eq!(err.status(), 403);
    assert_eq!(err.to_error_json(), r#"{"code":"COOKIE_BOT_NOT_ENTITLED"}"#);
  }

  #[test]
  fn a_bare_404_means_something_different_on_a_schedule_and_on_a_run() {
    // Both routes 404. Sharing one code would tell a user with no enrolment
    // that their run id is wrong, and vice versa.
    assert_eq!(
      cloud_errors::classify_message("(404) Not Found", SCHEDULE_CODES).code,
      "COOKIE_BOT_NOT_ENROLLED"
    );
    assert_eq!(
      cloud_errors::classify_message("(404) Not Found", RUN_CODES).code,
      "COOKIE_BOT_RUN_NOT_FOUND"
    );
  }

  #[test]
  fn query_values_are_encoded_so_they_cannot_smuggle_a_parameter() {
    // A keyset cursor is a server-issued opaque string. One containing `&`
    // would otherwise inject a second parameter into the request.
    let url = with_query(
      "https://api.example.com/runs",
      &[
        ("scope".to_string(), "team".to_string()),
        (
          "before".to_string(),
          "2026-08-03T00:00:00Z&limit=100".to_string(),
        ),
      ],
    );
    assert_eq!(
      url,
      "https://api.example.com/runs?scope=team&before=2026-08-03T00%3A00%3A00Z%26limit%3D100"
    );
    assert_eq!(
      with_query("https://api.example.com/runs", &[]),
      "https://api.example.com/runs",
      "an empty query must not leave a dangling separator"
    );
  }

  #[test]
  fn the_preset_list_carries_ids_not_behaviour() {
    // If this type ever gained the parameters that describe what a preset
    // actually does, the server-owned browsing model would have leaked into the
    // open-source client. Ids and a rough duration are all that may cross.
    let presets: CookieBotPresetList = serde_json::from_str(
      r#"{"presets":[{"id":"balanced","typical_minutes":35,"recommended":true}],
          "default_preset":"balanced"}"#,
    )
    .expect("the preset list must deserialize");
    assert_eq!(presets.presets[0].id, "balanced");
    assert_eq!(presets.presets[0].typical_minutes, Some(35));
    assert_eq!(presets.default_preset.as_deref(), Some("balanced"));
    // An older deployment sends neither of these, and the dialog has to render
    // against it: no templates simply means the picker offers the user's own
    // list, and no limits means the mirrored bounds apply.
    assert!(presets.templates.is_empty());
    assert!(presets.limits.is_none());
  }

  #[test]
  fn a_template_crosses_the_wire_as_a_count_and_never_as_urls() {
    // The site pool is server-owned. If this type ever gained a `sites` field
    // the curation would be published, and a published list is one a retailer
    // can filter.
    let presets: CookieBotPresetList = serde_json::from_str(
      r#"{"presets":[],"default_preset":"balanced",
          "templates":[{"id":"low-intent-purchaser","site_count":32,
            "name":"Low-Intent Purchaser","description":"Price-sensitive browsing."}],
          "limits":{"min_minutes":5,"max_minutes":120,"min_sites":1,"max_sites":40,
            "max_site_length":2048,"max_jitter_seconds":3600,"max_slots":14,
            "max_template_name_length":80}}"#,
    )
    .expect("the preset list must carry templates and limits");

    assert_eq!(presets.templates[0].id, "low-intent-purchaser");
    assert_eq!(presets.templates[0].site_count, 32);
    let limits = presets.limits.expect("limits must decode");
    assert_eq!(limits.max_slots, Some(14));
    assert_eq!(limits.max_template_name_length, Some(80));
    assert_eq!(limits.max_sites, Some(40));
  }

  #[test]
  fn a_saved_list_arrives_with_the_prefix_a_schedule_write_needs() {
    // The id is what `template_id` takes verbatim. Handing the client a bare
    // uuid and expecting it to prepend `user:` is how a saved list gets looked
    // up against the built-in catalogue instead — which answers "no sites" and
    // silently unschedules the profile.
    let envelope: UserTemplateListEnvelope = serde_json::from_str(
      r#"{"templates":[{"id":"user:1c9a…","name":"My shops",
          "sites":["https://example.com"],"updated_at":"2026-08-05T10:00:00.000Z"}]}"#,
    )
    .expect("the user template list must deserialize");

    let template = &envelope.templates[0];
    assert!(template.id.starts_with("user:"));
    assert_eq!(template.name, "My shops");
    assert_eq!(template.sites.len(), 1);
  }

  #[test]
  fn deleting_a_saved_list_that_is_already_gone_is_not_a_failure() {
    // The route never 404s, so a delete retried after a dropped response has to
    // read as "nothing left to do" rather than as an error the user must act on.
    let deleted: UserTemplateDeleted =
      serde_json::from_str(r#"{"deleted":false,"id":"user:gone"}"#)
        .expect("a no-op delete must deserialize");
    assert!(!deleted.deleted);
  }

  #[test]
  fn a_template_404_is_a_missing_list_and_not_an_unenrolled_profile() {
    // Sharing SCHEDULE_CODES here would tell someone renaming a site list that
    // their profile is not enrolled, and a name collision that a teammate
    // already warms the profile.
    assert_eq!(
      cloud_errors::classify_message("(404) Not Found", TEMPLATE_CODES).code,
      "COOKIE_BOT_TEMPLATE_NOT_FOUND"
    );
    assert_eq!(
      cloud_errors::classify_message("(409) Conflict", TEMPLATE_CODES).code,
      "COOKIE_BOT_TEMPLATE_NAME_TAKEN"
    );
  }
}
