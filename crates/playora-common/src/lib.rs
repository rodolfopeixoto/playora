//! Shared types between playora-agent (console) and playora-server.
//!
//! All wire types are JSON-serializable, stable, and free of secrets.

pub mod sources;
pub mod systems;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("config: {0}")]
    Config(String),
    #[error("db: {0}")]
    Db(String),
    #[error("network: {0}")]
    Net(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("other: {0}")]
    Other(String),
}

// ============================================================
// IDs (strongly-typed wrappers around UUIDs/strings)
// ============================================================

macro_rules! string_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new() -> Self {
                Self(format!("{}_{}", $prefix, Uuid::new_v4().simple()))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

string_id!(DeviceId, "dev");
string_id!(EventId, "evt");
string_id!(SessionId, "ses");
string_id!(TestId, "tst");
string_id!(SampleId, "smp");

// ============================================================
// Device + profile
// ============================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceProfile {
    R36sDarkosreCloneTest,
    R36sDarkosreClone,
    R36sBoyhomOriginal,
    UnknownLinuxHandheld,
}

impl DeviceProfile {
    pub fn detect_from(hardware: &str) -> Self {
        let h = hardware.to_lowercase();
        if h.contains("g80c") || h.contains("r36s-v12") || h.contains("clone") {
            Self::R36sDarkosreClone
        } else if h.contains("boyhom") {
            Self::R36sBoyhomOriginal
        } else if h.contains("r36s") {
            Self::R36sDarkosreClone
        } else {
            Self::UnknownLinuxHandheld
        }
    }
}

// ============================================================
// Hardware snapshot + tests + resources
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSnapshot {
    pub cpu_model: String,
    pub cpu_arch: String,
    pub cpu_cores: u32,
    pub mem_total_mb: u64,
    pub mem_available_mb: u64,
    pub swap_total_mb: u64,
    pub kernel: String,
    pub uptime_s: u64,
    pub load_avg: Option<(f32, f32, f32)>,
    pub temps_c: BTreeMap<String, f32>,
    pub freqs_mhz: BTreeMap<String, u32>,
    pub governors: BTreeMap<String, String>,
    pub disks: Vec<DiskInfo>,
    pub batteries: Vec<BatteryInfo>,
    pub net_ifs: Vec<NetIfInfo>,
    pub panel_compatible: Option<String>,
    pub panel_resolution: Option<(u32, u32)>,
    pub framebuffer: Option<String>,
    pub audio_cards: Vec<String>,
    pub input_devices: Vec<String>,
    pub usb_devices: Vec<UsbDevice>,
    pub retroarch_detected: bool,
    pub retroarch_version: Option<String>,
    pub hardware_string: Option<String>,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub mount: String,
    pub fstype: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub used_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryInfo {
    pub name: String,
    pub status: Option<String>,
    pub capacity_pct: Option<u32>,
    pub voltage_uv: Option<i64>,
    pub current_ua: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetIfInfo {
    pub name: String,
    pub up: bool,
    pub ipv4: Option<String>,
    pub mac_hash: Option<String>, // sha256 of MAC; never raw
    pub is_wireless: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbDevice {
    pub vendor_id: Option<String>,
    pub product_id: Option<String>,
    pub product: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareTestResult {
    pub test_id: TestId,
    pub test_type: String,
    pub status: String, // pass | fail | warn | skipped
    pub score: Option<f32>,
    pub payload: serde_json::Value,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsageSample {
    pub sample_id: SampleId,
    pub cpu_total_percent: f32,
    pub cpu_per_core: Vec<f32>,
    pub memory_total_mb: u64,
    pub memory_used_mb: u64,
    pub process: Option<ProcessSample>,
    pub temperatures: BTreeMap<String, f32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSample {
    pub name: String,
    pub pid: u32,
    pub cpu_percent: f32,
    pub memory_mb: u64,
}

// ============================================================
// Games + sessions
// ============================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum GameSystem {
    Nes,
    Snes,
    Gb,
    Gbc,
    Gba,
    Genesis,
    Megadrive,
    N64,
    Psx,
    Psp,
    Dreamcast,
    Saturn,
    Atari2600,
    Atari7800,
    Arcade,
    Mame,
    NeoGeo,
    PcEngine,
    MasterSystem,
    GameGear,
    Wonderswan,
    Other,
}

impl GameSystem {
    pub fn from_folder(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "nes" | "famicom" => Self::Nes,
            "snes" | "supernes" => Self::Snes,
            "gb" => Self::Gb,
            "gbc" => Self::Gbc,
            "gba" => Self::Gba,
            "genesis" | "megadrive" => Self::Megadrive,
            "n64" => Self::N64,
            "psx" => Self::Psx,
            "psp" => Self::Psp,
            "dreamcast" => Self::Dreamcast,
            "saturn" => Self::Saturn,
            "atari2600" => Self::Atari2600,
            "atari7800" => Self::Atari7800,
            "arcade" => Self::Arcade,
            "mame" | "mame2003" => Self::Mame,
            "neogeo" => Self::NeoGeo,
            "pcengine" => Self::PcEngine,
            "mastersystem" => Self::MasterSystem,
            "gamegear" => Self::GameGear,
            "wonderswan" => Self::Wonderswan,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameMetadata {
    pub system: GameSystem,
    pub name: String,
    pub rom_path: String,
    pub rom_hash: Option<String>,
    pub file_size: u64,
    pub extension: String,
    pub image_path: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSessionStarted {
    pub session_id: SessionId,
    pub system: GameSystem,
    pub game_name: String,
    pub rom_path: String,
    pub rom_hash: Option<String>,
    pub core: Option<String>,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSessionFinished {
    pub session_id: SessionId,
    pub ended_at: DateTime<Utc>,
    pub duration_seconds: u64,
    pub exit_code: Option<i32>,
    pub save_changed: bool,
    pub max_cpu_percent: Option<f32>,
    pub max_memory_mb: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomScanned {
    pub metadata: GameMetadata,
    pub scanned_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveSnapshot {
    pub game_id: Option<String>,
    pub save_path: String,
    pub save_hash: String,
    pub file_size: u64,
    pub modified_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceHeartbeat {
    pub agent_version: String,
    pub wifi_connected: bool,
    pub free_disk_mb: u64,
    pub pending_events: u32,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActivityStatus {
    Running,
    Ok,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub script: String,
    pub status: ActivityStatus,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub log_path: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub stdout_tail: Option<String>,
}

// ============================================================
// Sprint-1 additive event payloads — DO NOT renumber, only append.
// All new variants are optional; old agents/servers ignore unknown.
// ============================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Critical,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemIssueDetected {
    pub code: String,
    pub severity: IssueSeverity,
    pub title: String,
    pub evidence: Option<String>,
    pub suggested_fix: Option<String>,
    pub auto_fixable: bool,
    pub detected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorScore {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub report_id: String,
    pub score: DoctorScore,
    pub checks_total: u32,
    pub checks_ok: u32,
    pub checks_warn: u32,
    pub checks_fail: u32,
    pub issues: Vec<SystemIssueDetected>,
    pub auto_fixes: Vec<String>,
    pub manual_fixes: Vec<String>,
    pub report_path: Option<String>,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomAuditResult {
    pub audit_id: String,
    pub roms_total: u32,
    pub roms_orphan: u32,
    pub broken_cue: u32,
    pub broken_m3u: u32,
    pub zero_byte: u32,
    pub duplicates: u32,
    pub macos_junk: u32,
    pub gamelist_invalid: u32,
    pub bios_missing: Vec<String>,
    pub unknown_extensions: u32,
    pub report_path: Option<String>,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptStarted {
    pub script: String,
    pub pid: Option<u32>,
    pub args: Option<String>,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptFinished {
    pub script: String,
    pub exit_code: i32,
    pub duration_seconds: u64,
    pub stdout_tail: Option<String>,
    pub ended_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSessionCrashed {
    pub session_id: SessionId,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub stderr_tail: Option<String>,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSessionOrphaned {
    pub session_id: SessionId,
    pub started_at: DateTime<Utc>,
    pub reconciled_at: DateTime<Utc>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveChanged {
    pub system: String,
    pub save_path: String,
    pub old_hash: Option<String>,
    pub new_hash: String,
    pub file_size: u64,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackScreenRecovered {
    pub triggered_by: String,
    pub duration_seconds: u64,
    pub es_restarted: bool,
    pub killed_processes: Vec<String>,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulationStationRestarted {
    pub reason: String,
    pub method: String, // systemd|exec|fallback
    pub captured_at: DateTime<Utc>,
}

// Sprint-4 netplay (experimental; opt-in only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetplayRoomCreated {
    pub room_id: String,
    pub host_code: String,
    pub system: String,
    pub core: String,
    pub content_hash: Option<String>,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetplayRoomJoined {
    pub room_id: String,
    pub host_code: String,
    pub latency_ms: Option<u32>,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameMetadataEvent {
    pub system: String,
    pub name_query: String,
    pub display_name: String,
    pub genre: String,
    pub year: String,
    pub publisher: String,
    pub cover_url: String,
    pub source: String,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreProgress {
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u64,
    pub current_path: Option<String>,
    pub captured_at: DateTime<Utc>,
}

// ============================================================
// Sync envelope
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum EventPayload {
    DeviceHeartbeat(DeviceHeartbeat),
    HardwareSnapshot(HardwareSnapshot),
    HardwareTestResult(HardwareTestResult),
    ResourceSample(ResourceUsageSample),
    GameSessionStarted(GameSessionStarted),
    GameSessionFinished(GameSessionFinished),
    RomScanned(RomScanned),
    SaveSnapshot(SaveSnapshot),
    Activity(Activity),
    RestoreProgress(RestoreProgress),
    GameMetadata(GameMetadataEvent),
    // Sprint-1 additive (append only):
    SystemIssueDetected(SystemIssueDetected),
    DoctorReport(DoctorReport),
    RomAuditResult(RomAuditResult),
    ScriptStarted(ScriptStarted),
    ScriptFinished(ScriptFinished),
    GameSessionCrashed(GameSessionCrashed),
    GameSessionOrphaned(GameSessionOrphaned),
    SaveChanged(SaveChanged),
    BlackScreenRecovered(BlackScreenRecovered),
    EmulationStationRestarted(EmulationStationRestarted),
    NetplayRoomCreated(NetplayRoomCreated),
    NetplayRoomJoined(NetplayRoomJoined),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: EventId,
    pub device_id: DeviceId,
    pub created_at: DateTime<Utc>,
    pub payload: EventPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncBatch {
    pub device_id: DeviceId,
    pub agent_version: String,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAck {
    pub accepted: Vec<EventId>,
    pub duplicates: Vec<EventId>,
    pub rejected: Vec<(EventId, String)>,
}

// ============================================================
// Feature flags + catalog + capabilities
// ============================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FeatureStatus {
    Enabled,
    Disabled,
    Locked,
    Planned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlag {
    pub key: String,
    pub status: FeatureStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureManifest {
    pub device_id: DeviceId,
    pub features: BTreeMap<String, FeatureStatus>,
    pub requirements: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityReport {
    pub hardware_profile: DeviceProfile,
    pub wifi_ok: bool,
    pub storage_ok: bool,
    pub retroarch_detected: bool,
    pub catalog_supported: bool,
    pub runtime_probe_supported: bool,
    pub input_test_supported: bool,
    pub audio_test_supported: bool,
    pub screen_test_supported: bool,
    pub free_space_ok: bool,
    pub agent_version: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogType {
    HomebrewGame,
    DemoAuthorized,
    OpenSourceGame,
    PublicDomain,
    MetadataPack,
    Theme,
    ConfigPack,
    PatchAllowed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogItem {
    pub id: String,
    pub title: String,
    pub system: Option<GameSystem>,
    pub r#type: CatalogType,
    pub description: String,
    pub version: String,
    pub license: String,
    pub author: String,
    pub cover_url: Option<String>,
    pub screenshots: Vec<String>,
    pub download_url: Option<String>,
    pub file_size: Option<u64>,
    pub sha256: Option<String>,
    pub install_path: Option<String>,
    pub supported_device_profiles: Vec<DeviceProfile>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadReport {
    pub catalog_item_id: String,
    pub status: String,
    pub error: Option<String>,
    pub completed_at: Option<DateTime<Utc>>,
}

// ============================================================
// Agent config
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub device_id: DeviceId,
    pub device_name: String,
    pub device_profile: DeviceProfile,
    pub os_family: String,
    pub server_url: String,
    pub auth_token: Option<String>,
    pub rom_paths: Vec<String>,
    pub save_paths: Vec<String>,
    pub metadata_paths: Vec<String>,
    pub scan_interval_minutes: u32,
    pub sync_interval_seconds: u32,
    pub max_batch_size: u32,
    pub enable_runtime_probe: bool,
    pub enable_retroarch_network_control: bool,
    pub retroarch_udp_port: u16,
    pub enable_catalog: bool,
    pub enable_hardware_tests: bool,
    pub enable_resource_sampling: bool,
    pub log_level: String,
    /// Scheduled jobs (UTC hour-of-day). 0..23 fires the job once per day; None disables.
    #[serde(default)]
    pub cloud_backup_daily_hour_utc: Option<u8>,
    #[serde(default)]
    pub scan_daily_hour_utc: Option<u8>,
    #[serde(default)]
    pub extract_roms_daily_hour_utc: Option<u8>,
    #[serde(default)]
    pub fetch_covers_daily_hour_utc: Option<u8>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            device_id: DeviceId::new(),
            device_name: "R36S".into(),
            device_profile: DeviceProfile::UnknownLinuxHandheld,
            os_family: "darkosre-r36".into(),
            server_url: "http://127.0.0.1:8080".into(),
            auth_token: None,
            rom_paths: vec!["/roms".into()],
            save_paths: vec!["/roms/savestates".into()],
            metadata_paths: vec!["/roms".into()],
            scan_interval_minutes: 60,
            sync_interval_seconds: 60,
            max_batch_size: 100,
            enable_runtime_probe: false,
            enable_retroarch_network_control: false,
            retroarch_udp_port: 55355,
            enable_catalog: true,
            enable_hardware_tests: true,
            enable_resource_sampling: true,
            log_level: "info".into(),
            cloud_backup_daily_hour_utc: None,
            scan_daily_hour_utc: None,
            extract_roms_daily_hour_utc: None,
            fetch_covers_daily_hour_utc: None,
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(&mut s, "{b:02x}").unwrap();
    }
    s
}

#[cfg(test)]
mod core_tests {
    use super::*;

    #[test]
    fn device_id_unique_and_prefixed() {
        let a = DeviceId::new();
        let b = DeviceId::new();
        assert_ne!(a, b);
        assert!(a.as_str().starts_with("dev_"));
    }

    #[test]
    fn game_system_round_trip_folder() {
        for f in [
            "nes",
            "snes",
            "gba",
            "n64",
            "psx",
            "arcade",
            "unknown_other",
        ] {
            let s = GameSystem::from_folder(f);
            if f != "unknown_other" {
                assert_ne!(s, GameSystem::Other, "{f}");
            }
        }
    }

    #[test]
    fn device_profile_detect() {
        assert_eq!(
            DeviceProfile::detect_from("G80CA-MB V1.2-20250422 Panel 8"),
            DeviceProfile::R36sDarkosreClone
        );
        assert_eq!(
            DeviceProfile::detect_from(""),
            DeviceProfile::UnknownLinuxHandheld
        );
    }

    #[test]
    fn sha256_hex_byte_length() {
        let h = sha256_hex(&[0xab, 0xcd]);
        assert_eq!(h, "abcd");
    }

    #[test]
    fn event_payload_json_round_trip() {
        let ev = Event {
            event_id: EventId::new(),
            device_id: DeviceId::new(),
            created_at: Utc::now(),
            payload: EventPayload::DeviceHeartbeat(DeviceHeartbeat {
                agent_version: "0.1.0".into(),
                wifi_connected: true,
                free_disk_mb: 1024,
                pending_events: 0,
                captured_at: Utc::now(),
            }),
        };
        let j = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&j).unwrap();
        assert_eq!(back.event_id, ev.event_id);
    }

    #[test]
    fn default_agent_config_safe_defaults() {
        let c = AgentConfig::default();
        assert!(!c.enable_runtime_probe, "runtime probe must default OFF");
        assert!(c.enable_catalog);
        assert!(c.max_batch_size > 0);
    }

    #[test]
    fn event_id_default_is_unique() {
        let a = EventId::default();
        let b = EventId::default();
        assert_ne!(a, b);
        assert!(a.as_str().starts_with("evt_"));
    }

    #[test]
    fn session_test_sample_ids_have_prefix() {
        assert!(SessionId::new().as_str().starts_with("ses_"));
        assert!(TestId::new().as_str().starts_with("tst_"));
        assert!(SampleId::new().as_str().starts_with("smp_"));
    }

    #[test]
    fn device_profile_detection_branches() {
        assert!(matches!(
            DeviceProfile::detect_from("BOYHOM-R36"),
            DeviceProfile::R36sBoyhomOriginal
        ));
        assert!(matches!(
            DeviceProfile::detect_from("Generic-Linux-Box"),
            DeviceProfile::UnknownLinuxHandheld
        ));
        assert!(matches!(
            DeviceProfile::detect_from("r36s anbernic"),
            DeviceProfile::R36sDarkosreClone
        ));
    }

    #[test]
    fn game_system_recognized_folders() {
        assert_eq!(GameSystem::from_folder("nes"), GameSystem::Nes);
        assert_eq!(GameSystem::from_folder("NES"), GameSystem::Nes);
        assert_eq!(GameSystem::from_folder("famicom"), GameSystem::Nes);
        assert_eq!(GameSystem::from_folder("invalid_xyz"), GameSystem::Other);
    }

    #[test]
    fn sync_batch_serializes() {
        let batch = SyncBatch {
            device_id: DeviceId::new(),
            agent_version: "0.0.0".into(),
            events: vec![],
        };
        let s = serde_json::to_string(&batch).unwrap();
        assert!(s.contains("device_id"));
    }

    #[test]
    fn sync_ack_round_trip() {
        let id = EventId::new();
        let ack = SyncAck {
            accepted: vec![id.clone()],
            duplicates: vec![],
            rejected: vec![],
        };
        let s = serde_json::to_string(&ack).unwrap();
        let back: SyncAck = serde_json::from_str(&s).unwrap();
        assert_eq!(back.accepted.len(), 1);
    }

    #[test]
    fn feature_status_round_trip() {
        for v in [
            FeatureStatus::Enabled,
            FeatureStatus::Disabled,
            FeatureStatus::Locked,
            FeatureStatus::Planned,
        ] {
            let s = serde_json::to_string(&v).unwrap();
            let back: FeatureStatus = serde_json::from_str(&s).unwrap();
            assert_eq!(back, v);
        }
    }

    #[test]
    fn sha256_hex_zero_padded() {
        let h = sha256_hex(&[0x01, 0x0f]);
        assert_eq!(h, "010f");
    }
}
