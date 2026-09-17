//! Documentos versionados do estúdio (ADR-0014 §5): cenas de mistura, macros,
//! sobreposições, cenas de luz, perfis de correcção por câmara e alinhamentos.
//!
//! O servidor GUARDA e VALIDA; quem aplica é o browser (a mistura, a correcção,
//! as macros) ou o agente de luz. Cada corpo é tipado e recusa campos
//! desconhecidos; o que se guarda é o corpo normalizado (re-serializado), não o
//! texto que chegou.

use delonix_meet_core::DomainError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    MixerScene,
    Macro,
    Overlay,
    LightScene,
    CameraProfile,
    Rundown,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::MixerScene,
        Kind::Macro,
        Kind::Overlay,
        Kind::LightScene,
        Kind::CameraProfile,
        Kind::Rundown,
    ];

    /// Valor na coluna `kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MixerScene => "mixer_scene",
            Self::Macro => "macro",
            Self::Overlay => "overlay",
            Self::LightScene => "light_scene",
            Self::CameraProfile => "camera_profile",
            Self::Rundown => "rundown",
        }
    }

    /// Segmento do caminho (`/mixer-scenes`).
    pub fn path_segment(self) -> &'static str {
        match self {
            Self::MixerScene => "mixer-scenes",
            Self::Macro => "macros",
            Self::Overlay => "overlays",
            Self::LightScene => "light-scenes",
            Self::CameraProfile => "camera-profiles",
            Self::Rundown => "rundowns",
        }
    }
}

pub const MAX_NAME: usize = 80;
/// Tecto do corpo serializado: uma mesa de 64 canais cabe com folga.
pub const MAX_BODY_BYTES: usize = 256 * 1024;

/// O que o adaptador grava depois de validar.
#[derive(Debug, Clone, PartialEq)]
pub struct Validated {
    pub name: String,
    pub body: Value,
    /// Tecla única por tipo e estúdio (`F2`, `mod+1`), se o tipo a tiver.
    pub key: Option<String>,
    /// Calculado no servidor (alinhamento: duração total).
    pub summary: Option<Value>,
    /// Documentos DESTE estúdio que o corpo refere (macros → cenas/sobreposições).
    pub references: Vec<Uuid>,
}

fn bad(field: impl Into<String>, why: impl Into<String>) -> DomainError {
    let why = why.into();
    DomainError::invalid(
        "studio.invalid_document",
        format!("documento inválido: {why}"),
    )
    .with_field(field, why)
}

fn num(field: &str, v: f64, lo: f64, hi: f64) -> Result<(), DomainError> {
    if !v.is_finite() || v < lo || v > hi {
        return Err(bad(field, format!("tem de estar entre {lo} e {hi}")));
    }
    Ok(())
}

fn text(field: &str, v: &str, max: usize) -> Result<(), DomainError> {
    if v.chars().count() > max || v.chars().any(char::is_control) {
        return Err(bad(field, format!("máximo {max} caracteres, sem controlo")));
    }
    Ok(())
}

pub fn validate_name(name: &str) -> Result<String, DomainError> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > MAX_NAME || n.chars().any(char::is_control) {
        return Err(bad("name", format!("1–{MAX_NAME} caracteres")));
    }
    Ok(n.to_string())
}

fn function_key(field: &str, k: &str) -> Result<String, DomainError> {
    let ok = k
        .strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=12).contains(&n) && k.len() <= 3 && !k[1..].starts_with('0'));
    if !ok {
        return Err(bad(field, "tecla F1–F12"));
    }
    Ok(k.to_string())
}

fn modifier_key(field: &str, k: &str) -> Result<String, DomainError> {
    let ok = k
        .strip_prefix("mod+")
        .is_some_and(|d| d.len() == 1 && matches!(d.as_bytes()[0], b'1'..=b'9'));
    if !ok {
        return Err(bad(field, "tecla mod+1–mod+9"));
    }
    Ok(k.to_string())
}

// ---------------------------------------------------------------- mistura ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MixerScene {
    pub sample_rate: u32,
    pub bit_depth: u8,
    pub loudness: Loudness,
    pub master: Master,
    #[serde(default)]
    pub buses: Vec<Bus>,
    pub channels: Vec<Channel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Loudness {
    pub target_lufs: f64,
    pub true_peak_max_dbtp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Master {
    pub level_db: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusKind {
    Aux,
    Record,
    Program,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bus {
    pub id: String,
    pub name: String,
    pub kind: BusKind,
    pub level_db: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    pub number: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    pub gain_db: f64,
    /// `null` = −∞ (fader em baixo).
    pub fader_db: Option<f64>,
    pub mute: bool,
    pub solo: bool,
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub buses: Vec<String>,
    pub eq: Eq4,
    pub dynamics: Dynamics,
    #[serde(default)]
    pub cleanup: Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BandType {
    LowShelf,
    Peak,
    HighShelf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Band {
    #[serde(rename = "type")]
    pub band_type: BandType,
    pub freq_hz: f64,
    pub gain_db: f64,
    pub q: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Eq4 {
    pub enabled: bool,
    pub bands: Vec<Band>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    pub enabled: bool,
    pub threshold_db: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compressor {
    pub enabled: bool,
    pub threshold_db: f64,
    pub ratio: f64,
    pub attack_ms: f64,
    pub release_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limiter {
    pub enabled: bool,
    pub ceiling_db: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dynamics {
    pub gate: Gate,
    pub compressor: Compressor,
    pub limiter: Limiter,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cleanup {
    #[serde(default)]
    pub echo_cancellation: bool,
    /// `null` = desligada.
    #[serde(default)]
    pub noise_reduction_db: Option<f64>,
    #[serde(default)]
    pub silence_removal: bool,
    #[serde(default)]
    pub voice_leveling: bool,
}

fn validate_mixer(s: &MixerScene) -> Result<(), DomainError> {
    if !matches!(s.sample_rate, 44_100 | 48_000) {
        return Err(bad("body.sample_rate", "44100 ou 48000"));
    }
    if !matches!(s.bit_depth, 16 | 24) {
        return Err(bad("body.bit_depth", "16 ou 24"));
    }
    num(
        "body.loudness.target_lufs",
        s.loudness.target_lufs,
        -30.0,
        -5.0,
    )?;
    num(
        "body.loudness.true_peak_max_dbtp",
        s.loudness.true_peak_max_dbtp,
        -9.0,
        0.0,
    )?;
    num("body.master.level_db", s.master.level_db, -90.0, 10.0)?;
    if s.buses.len() > 8 {
        return Err(bad("body.buses", "no máximo 8 barramentos"));
    }
    let mut bus_ids: Vec<&str> = Vec::new();
    for (i, b) in s.buses.iter().enumerate() {
        let f = format!("body.buses[{i}]");
        if b.id.is_empty()
            || b.id.len() > 16
            || !b.id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            return Err(bad(format!("{f}.id"), "1–16 caracteres [A-Za-z0-9_]"));
        }
        if bus_ids.contains(&b.id.as_str()) {
            return Err(bad(format!("{f}.id"), "id repetido"));
        }
        bus_ids.push(&b.id);
        text(&format!("{f}.name"), &b.name, 40)?;
        num(&format!("{f}.level_db"), b.level_db, -90.0, 10.0)?;
    }
    if s.channels.is_empty() || s.channels.len() > 64 {
        return Err(bad("body.channels", "1 a 64 canais"));
    }
    let mut numbers: Vec<u32> = Vec::new();
    for (i, c) in s.channels.iter().enumerate() {
        let f = format!("body.channels[{i}]");
        if !(1..=64).contains(&c.number) || numbers.contains(&c.number) {
            return Err(bad(format!("{f}.number"), "1–64 e único"));
        }
        numbers.push(c.number);
        text(&format!("{f}.name"), &c.name, 40)?;
        if let Some(inp) = &c.input {
            text(&format!("{f}.input"), inp, 40)?;
        }
        num(&format!("{f}.gain_db"), c.gain_db, -60.0, 24.0)?;
        if let Some(fd) = c.fader_db {
            num(&format!("{f}.fader_db"), fd, -90.0, 10.0)?;
        }
        num(&format!("{f}.pan"), c.pan, -1.0, 1.0)?;
        for b in &c.buses {
            if !bus_ids.contains(&b.as_str()) {
                return Err(bad(
                    format!("{f}.buses"),
                    format!("barramento «{b}» não existe"),
                ));
            }
        }
        if c.eq.bands.len() != 4 {
            return Err(bad(format!("{f}.eq.bands"), "exactamente 4 bandas"));
        }
        for (j, b) in c.eq.bands.iter().enumerate() {
            let g = format!("{f}.eq.bands[{j}]");
            num(&format!("{g}.freq_hz"), b.freq_hz, 20.0, 20_000.0)?;
            num(&format!("{g}.gain_db"), b.gain_db, -18.0, 18.0)?;
            num(&format!("{g}.q"), b.q, 0.1, 10.0)?;
        }
        let d = &c.dynamics;
        num(
            &format!("{f}.dynamics.gate.threshold_db"),
            d.gate.threshold_db,
            -90.0,
            0.0,
        )?;
        num(
            &format!("{f}.dynamics.compressor.threshold_db"),
            d.compressor.threshold_db,
            -60.0,
            0.0,
        )?;
        num(
            &format!("{f}.dynamics.compressor.ratio"),
            d.compressor.ratio,
            1.0,
            20.0,
        )?;
        num(
            &format!("{f}.dynamics.compressor.attack_ms"),
            d.compressor.attack_ms,
            0.1,
            200.0,
        )?;
        num(
            &format!("{f}.dynamics.compressor.release_ms"),
            d.compressor.release_ms,
            5.0,
            3000.0,
        )?;
        num(
            &format!("{f}.dynamics.limiter.ceiling_db"),
            d.limiter.ceiling_db,
            -20.0,
            0.0,
        )?;
        if let Some(nr) = c.cleanup.noise_reduction_db {
            num(&format!("{f}.cleanup.noise_reduction_db"), nr, -40.0, 0.0)?;
        }
    }
    Ok(())
}

// ----------------------------------------------------------------- macros ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacroDoc {
    pub key: String,
    pub steps: Vec<MacroStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    Cut,
    Mix,
    Wipe,
    Stinger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MacroStep {
    SetPreview {
        source_number: u32,
    },
    SetProgram {
        source_number: u32,
    },
    Transition {
        kind: TransitionKind,
        duration_ms: u32,
    },
    Overlay {
        overlay_id: Uuid,
        on: bool,
    },
    LightScene {
        document_id: Uuid,
    },
    MixerScene {
        document_id: Uuid,
    },
    Channel {
        number: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fader_db: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mute: Option<bool>,
    },
    Wait {
        ms: u32,
    },
    Recording {
        on: bool,
    },
    EndBroadcast,
}

fn validate_macro(m: &MacroDoc, refs: &mut Vec<Uuid>) -> Result<String, DomainError> {
    let key = function_key("body.key", &m.key)?;
    if m.steps.is_empty() || m.steps.len() > 50 {
        return Err(bad("body.steps", "1 a 50 passos"));
    }
    for (i, s) in m.steps.iter().enumerate() {
        let f = format!("body.steps[{i}]");
        match s {
            MacroStep::SetPreview { source_number } | MacroStep::SetProgram { source_number } => {
                if !(1..=16).contains(source_number) {
                    return Err(bad(format!("{f}.source_number"), "1–16"));
                }
            }
            MacroStep::Transition { duration_ms, .. } => num(
                &format!("{f}.duration_ms"),
                f64::from(*duration_ms),
                0.0,
                10_000.0,
            )?,
            MacroStep::Overlay { overlay_id, .. } => refs.push(*overlay_id),
            MacroStep::LightScene { document_id } | MacroStep::MixerScene { document_id } => {
                refs.push(*document_id)
            }
            MacroStep::Channel {
                number,
                fader_db,
                mute,
            } => {
                if !(1..=64).contains(number) {
                    return Err(bad(format!("{f}.number"), "1–64"));
                }
                if let Some(fd) = fader_db {
                    num(&format!("{f}.fader_db"), *fd, -90.0, 10.0)?;
                }
                if fader_db.is_none() && mute.is_none() {
                    return Err(bad(f, "o passo «channel» muda fader_db, mute ou os dois"));
                }
            }
            MacroStep::Wait { ms } => num(&format!("{f}.ms"), f64::from(*ms), 0.0, 60_000.0)?,
            MacroStep::Recording { .. } | MacroStep::EndBroadcast => {}
        }
    }
    Ok(key)
}

// --------------------------------------------------------- sobreposições ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayDoc {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(rename = "type")]
    pub overlay_type: OverlayType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lower_third: Option<LowerThird>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<Logo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<Clock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll: Option<PollOverlay>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayType {
    LowerThird,
    Logo,
    Clock,
    Poll,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LowerThird {
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Logo {
    pub text: String,
    pub position: Corner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clock {
    pub format: String,
    pub timezone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PollOverlay {
    pub question: String,
    pub options: Vec<String>,
}

fn validate_overlay(o: &OverlayDoc) -> Result<Option<String>, DomainError> {
    let key = o
        .key
        .as_deref()
        .map(|k| modifier_key("body.key", k))
        .transpose()?;
    // Exactamente o bloco do tipo, e nenhum outro: um `logo` num `clock` seria
    // um campo escrito e ignorado.
    let present = [
        (OverlayType::LowerThird, o.lower_third.is_some()),
        (OverlayType::Logo, o.logo.is_some()),
        (OverlayType::Clock, o.clock.is_some()),
        (OverlayType::Poll, o.poll.is_some()),
    ];
    for (t, is) in present {
        if is != (t == o.overlay_type) {
            return Err(bad(
                "body",
                "o corpo traz exactamente o bloco do seu tipo (lower_third, logo, clock ou poll)",
            ));
        }
    }
    if let Some(l) = &o.lower_third {
        if l.title.trim().is_empty() {
            return Err(bad("body.lower_third.title", "obrigatório"));
        }
        text("body.lower_third.title", &l.title, 80)?;
        text("body.lower_third.subtitle", &l.subtitle, 120)?;
    }
    if let Some(l) = &o.logo {
        text("body.logo.text", &l.text, 60)?;
    }
    if let Some(c) = &o.clock {
        text("body.clock.format", &c.format, 20)?;
        // Forma IANA (`Africa/Luanda`, `UTC`) — a lista completa é do browser.
        if c.timezone.is_empty()
            || c.timezone.len() > 40
            || !c
                .timezone
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'+'))
        {
            return Err(bad("body.clock.timezone", "fuso IANA, p.ex. Africa/Luanda"));
        }
    }
    if let Some(p) = &o.poll {
        text("body.poll.question", &p.question, 200)?;
        if p.options.len() < 2 || p.options.len() > 10 {
            return Err(bad("body.poll.options", "2 a 10 opções"));
        }
        for (i, op) in p.options.iter().enumerate() {
            text(&format!("body.poll.options[{i}]"), op, 80)?;
        }
    }
    Ok(key)
}

// ---------------------------------------------------------------- luz ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightSceneDoc {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub transition_ms: u32,
    pub fixtures: Vec<FixtureLevel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureLevel {
    pub fixture_key: String,
    pub level: f64,
    #[serde(default)]
    pub cct_k: Option<u32>,
}

pub fn validate_fixture_levels(field: &str, v: &[FixtureLevel]) -> Result<(), DomainError> {
    if v.is_empty() || v.len() > 128 {
        return Err(bad(field, "1 a 128 aparelhos"));
    }
    let mut seen: Vec<&str> = Vec::new();
    for (i, f) in v.iter().enumerate() {
        let g = format!("{field}[{i}]");
        super::light::validate_fixture_key(&f.fixture_key)
            .map_err(|_| bad(format!("{g}.fixture_key"), "chave de aparelho inválida"))?;
        if seen.contains(&f.fixture_key.as_str()) {
            return Err(bad(format!("{g}.fixture_key"), "aparelho repetido"));
        }
        seen.push(&f.fixture_key);
        num(&format!("{g}.level"), f.level, 0.0, 100.0)?;
        if let Some(k) = f.cct_k {
            num(&format!("{g}.cct_k"), f64::from(k), 1000.0, 10_000.0)?;
        }
    }
    Ok(())
}

fn validate_light(l: &LightSceneDoc) -> Result<Option<String>, DomainError> {
    let key = l
        .key
        .as_deref()
        .map(|k| function_key("body.key", k))
        .transpose()?;
    num(
        "body.transition_ms",
        f64::from(l.transition_ms),
        0.0,
        60_000.0,
    )?;
    validate_fixture_levels("body.fixtures", &l.fixtures)?;
    Ok(key)
}

// ------------------------------------------------------------- perfis ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strength {
    Off,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraProfileDoc {
    pub source_number: u32,
    pub exposure_ev: f64,
    pub temperature_k: u32,
    #[serde(default)]
    pub tint: f64,
    pub contrast: f64,
    pub face_enhance: Strength,
    pub noise_reduction: Strength,
    #[serde(default)]
    pub background_blur: bool,
    #[serde(default)]
    pub white_balance_match: bool,
}

fn validate_profile(p: &CameraProfileDoc) -> Result<(), DomainError> {
    if !(1..=16).contains(&p.source_number) {
        return Err(bad("body.source_number", "1–16"));
    }
    num("body.exposure_ev", p.exposure_ev, -3.0, 3.0)?;
    num(
        "body.temperature_k",
        f64::from(p.temperature_k),
        2000.0,
        10_000.0,
    )?;
    num("body.tint", p.tint, -100.0, 100.0)?;
    num("body.contrast", p.contrast, 0.5, 3.0)?;
    Ok(())
}

// --------------------------------------------------------- alinhamento ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RundownDoc {
    pub items: Vec<RundownItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RundownItem {
    pub title: String,
    pub duration_ms: u64,
    #[serde(default)]
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macro_key: Option<String>,
}

fn validate_rundown(r: &RundownDoc) -> Result<Value, DomainError> {
    if r.items.is_empty() || r.items.len() > 200 {
        return Err(bad("body.items", "1 a 200 itens"));
    }
    let mut total: u64 = 0;
    for (i, it) in r.items.iter().enumerate() {
        let f = format!("body.items[{i}]");
        if it.title.trim().is_empty() {
            return Err(bad(format!("{f}.title"), "obrigatório"));
        }
        text(&format!("{f}.title"), &it.title, 120)?;
        text(&format!("{f}.note"), &it.note, 300)?;
        num(
            &format!("{f}.duration_ms"),
            it.duration_ms as f64,
            0.0,
            86_400_000.0,
        )?;
        if let Some(k) = &it.macro_key {
            function_key(&format!("{f}.macro_key"), k)?;
        }
        total += it.duration_ms;
    }
    Ok(serde_json::json!({"total_duration_ms": total, "item_count": r.items.len()}))
}

// --------------------------------------------------------------- entrada ---

fn parse<T: for<'de> Deserialize<'de>>(body: Value) -> Result<T, DomainError> {
    serde_json::from_value(body).map_err(|e| bad("body", e.to_string()))
}

fn normalized<T: Serialize>(t: &T) -> Result<Value, DomainError> {
    let v = serde_json::to_value(t).map_err(DomainError::internal)?;
    if v.to_string().len() > MAX_BODY_BYTES {
        return Err(bad("body", format!("máximo {} KiB", MAX_BODY_BYTES / 1024)));
    }
    Ok(v)
}

/// Valida e normaliza um documento. É a ÚNICA porta de entrada de um corpo.
pub fn validate(kind: Kind, name: &str, body: Value) -> Result<Validated, DomainError> {
    let name = validate_name(name)?;
    let mut references = Vec::new();
    let (body, key, summary) = match kind {
        Kind::MixerScene => {
            let d: MixerScene = parse(body)?;
            validate_mixer(&d)?;
            (normalized(&d)?, None, None)
        }
        Kind::Macro => {
            let d: MacroDoc = parse(body)?;
            let key = validate_macro(&d, &mut references)?;
            (normalized(&d)?, Some(key), None)
        }
        Kind::Overlay => {
            let d: OverlayDoc = parse(body)?;
            let key = validate_overlay(&d)?;
            (normalized(&d)?, key, None)
        }
        Kind::LightScene => {
            let d: LightSceneDoc = parse(body)?;
            let key = validate_light(&d)?;
            (normalized(&d)?, key, None)
        }
        Kind::CameraProfile => {
            let d: CameraProfileDoc = parse(body)?;
            validate_profile(&d)?;
            (normalized(&d)?, None, None)
        }
        Kind::Rundown => {
            let d: RundownDoc = parse(body)?;
            let s = validate_rundown(&d)?;
            (normalized(&d)?, None, Some(s))
        }
    };
    references.sort();
    references.dedup();
    Ok(Validated {
        name,
        body,
        key,
        summary,
        references,
    })
}

/// A versão enviada tem de ser a actual (concorrência optimista).
pub fn check_version(current: i32, sent: i32) -> Result<(), DomainError> {
    if current != sent {
        return Err(DomainError::conflict(
            "studio.version_conflict",
            format!(
                "o documento mudou entretanto (versão actual {current}, enviada {sent}) — recarregue antes de gravar"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn band(t: &str, f: f64) -> Value {
        json!({"type": t, "freq_hz": f, "gain_db": 0, "q": 0.7})
    }

    fn channel(n: u32) -> Value {
        json!({"number": n, "name": format!("CH {n}"), "gain_db": 0, "fader_db": -2.1,
               "mute": false, "solo": false, "pan": 0, "buses": ["aux1"],
               "eq": {"enabled": true, "bands": [band("low_shelf", 80.0), band("peak", 420.0),
                                                  band("peak", 2400.0), band("high_shelf", 8000.0)]},
               "dynamics": {"gate": {"enabled": true, "threshold_db": -42},
                            "compressor": {"enabled": true, "threshold_db": -18, "ratio": 3.2,
                                           "attack_ms": 12, "release_ms": 180},
                            "limiter": {"enabled": true, "ceiling_db": -2}},
               "cleanup": {"echo_cancellation": true, "noise_reduction_db": -14}})
    }

    fn mixer() -> Value {
        json!({"sample_rate": 48000, "bit_depth": 24,
               "loudness": {"target_lufs": -16, "true_peak_max_dbtp": -1},
               "master": {"level_db": -3},
               "buses": [{"id": "aux1", "name": "retorno", "kind": "aux", "level_db": -8}],
               "channels": [channel(1), channel(2)]})
    }

    #[test]
    fn cena_de_mistura_do_template_passa() {
        let v = validate(Kind::MixerScene, "Entrevista", mixer()).unwrap();
        assert_eq!(v.name, "Entrevista");
        assert!(v.key.is_none());
        assert_eq!(v.body["channels"][1]["number"], 2);
    }

    #[test]
    fn mistura_invalida_diz_o_campo() {
        let mut m = mixer();
        m["channels"][0]["eq"]["bands"][2]["gain_db"] = json!(24);
        let e = validate(Kind::MixerScene, "x", m).unwrap_err();
        assert_eq!(e.code, "studio.invalid_document");
        assert_eq!(e.details[0].field, "body.channels[0].eq.bands[2].gain_db");

        let mut m = mixer();
        m["channels"][1]["number"] = json!(1);
        assert_eq!(
            validate(Kind::MixerScene, "x", m).unwrap_err().details[0].field,
            "body.channels[1].number"
        );

        let mut m = mixer();
        m["channels"][0]["buses"] = json!(["nao-existe"]);
        assert!(validate(Kind::MixerScene, "x", m).is_err());

        let mut m = mixer();
        m["channels"][0]["eq"]["bands"] = json!([band("peak", 100.0)]);
        assert!(validate(Kind::MixerScene, "x", m).is_err());

        let mut m = mixer();
        m["loudness"]["target_lufs"] = json!(-40);
        assert!(validate(Kind::MixerScene, "x", m).is_err());
    }

    #[test]
    fn campo_desconhecido_nao_e_engolido() {
        let mut m = mixer();
        m["reverb"] = json!(true);
        let e = validate(Kind::MixerScene, "x", m).unwrap_err();
        assert_eq!(e.code, "studio.invalid_document");
        assert!(e.message.contains("reverb"), "{}", e.message);
    }

    #[test]
    fn fader_nulo_e_menos_infinito() {
        let mut m = mixer();
        m["channels"][0]["fader_db"] = Value::Null;
        let v = validate(Kind::MixerScene, "x", m).unwrap();
        assert!(v.body["channels"][0]["fader_db"].is_null());
    }

    #[test]
    fn macro_com_tecla_passos_e_referencias() {
        let scene = Uuid::new_v4();
        let ov = Uuid::new_v4();
        let v = validate(
            Kind::Macro,
            "Abertura",
            json!({"key": "F1", "steps": [
                {"action": "transition", "kind": "stinger", "duration_ms": 600},
                {"action": "overlay", "overlay_id": ov, "on": true},
                {"action": "light-scene", "document_id": scene},
                {"action": "channel", "number": 11, "fader_db": -18},
                {"action": "wait", "ms": 500},
                {"action": "end-broadcast"}
            ]}),
        )
        .unwrap();
        assert_eq!(v.key.as_deref(), Some("F1"));
        let mut expected = vec![scene, ov];
        expected.sort();
        assert_eq!(v.references, expected);
    }

    #[test]
    fn macro_recusa_tecla_passo_e_accao_desconhecida() {
        for key in ["F0", "F13", "F01", "G1", "f1"] {
            assert!(
                validate(
                    Kind::Macro,
                    "m",
                    json!({"key": key, "steps": [{"action": "end-broadcast"}]})
                )
                .is_err(),
                "{key}"
            );
        }
        assert!(validate(Kind::Macro, "m", json!({"key": "F2", "steps": []})).is_err());
        assert!(validate(
            Kind::Macro,
            "m",
            json!({"key": "F2", "steps": [{"action": "format-disk"}]})
        )
        .is_err());
        assert!(validate(
            Kind::Macro,
            "m",
            json!({"key": "F2", "steps": [{"action": "channel", "number": 1}]})
        )
        .is_err());
        assert!(validate(
            Kind::Macro,
            "m",
            json!({"key": "F2", "steps": [{"action": "wait", "ms": 60001}]})
        )
        .is_err());
    }

    #[test]
    fn sobreposicao_traz_so_o_bloco_do_tipo() {
        let v = validate(
            Kind::Overlay,
            "Legenda",
            json!({"key": "mod+1", "type": "lower-third",
                   "lower_third": {"title": "Ana Mbala", "subtitle": "directora de tecnologia"}}),
        )
        .unwrap();
        assert_eq!(v.key.as_deref(), Some("mod+1"));
        assert!(validate(Kind::Overlay, "x", json!({"type": "clock"})).is_err());
        assert!(validate(
            Kind::Overlay,
            "x",
            json!({"type": "clock", "clock": {"format": "HH:mm", "timezone": "Africa/Luanda"},
                   "logo": {"text": "a", "position": "top-left"}})
        )
        .is_err());
        assert!(validate(
            Kind::Overlay,
            "x",
            json!({"type": "poll", "poll": {"question": "?", "options": ["só uma"]}})
        )
        .is_err());
        assert!(validate(
            Kind::Overlay,
            "x",
            json!({"key": "mod+0", "type": "logo", "logo": {"text": "D", "position": "top-left"}})
        )
        .is_err());
    }

    #[test]
    fn cena_de_luz() {
        let v = validate(
            Kind::LightScene,
            "Entrevista",
            json!({"key": "F2", "transition_ms": 1800, "fixtures": [
                {"fixture_key": "dmx:1:1", "level": 86, "cct_k": 5200},
                {"fixture_key": "hue:bridge-1:3", "level": 48, "cct_k": null}
            ]}),
        )
        .unwrap();
        assert_eq!(v.key.as_deref(), Some("F2"));
        assert!(validate(
            Kind::LightScene,
            "x",
            json!({"transition_ms": 0, "fixtures": [{"fixture_key": "dmx:1:1", "level": 101}]})
        )
        .is_err());
        assert!(validate(
            Kind::LightScene,
            "x",
            json!({"transition_ms": 0, "fixtures": [
                {"fixture_key": "dmx:1:1", "level": 1}, {"fixture_key": "dmx:1:1", "level": 2}]})
        )
        .is_err());
    }

    #[test]
    fn perfil_de_camara() {
        let ok = json!({"source_number": 2, "exposure_ev": 0.4, "temperature_k": 5200, "tint": 0,
                        "contrast": 1.0, "face_enhance": "medium", "noise_reduction": "low"});
        assert!(validate(Kind::CameraProfile, "CAM 2", ok.clone()).is_ok());
        let mut bad = ok;
        bad["temperature_k"] = json!(15000);
        assert!(validate(Kind::CameraProfile, "CAM 2", bad).is_err());
    }

    #[test]
    fn alinhamento_calcula_a_duracao() {
        let v = validate(
            Kind::Rundown,
            "Programa",
            json!({"items": [
                {"title": "Abertura", "duration_ms": 60000, "note": "stinger", "macro_key": "F1"},
                {"title": "Entrevista", "duration_ms": 720000}
            ]}),
        )
        .unwrap();
        assert_eq!(
            v.summary.unwrap(),
            json!({"total_duration_ms": 780000, "item_count": 2})
        );
        assert!(validate(Kind::Rundown, "x", json!({"items": []})).is_err());
    }

    #[test]
    fn versao_desactualizada_e_conflito() {
        assert!(check_version(3, 3).is_ok());
        let e = check_version(4, 3).unwrap_err();
        assert_eq!(e.code, "studio.version_conflict");
    }

    #[test]
    fn caminhos_e_colunas_dos_seis_tipos() {
        let segs: Vec<&str> = Kind::ALL.iter().map(|k| k.path_segment()).collect();
        assert_eq!(
            segs,
            [
                "mixer-scenes",
                "macros",
                "overlays",
                "light-scenes",
                "camera-profiles",
                "rundowns"
            ]
        );
        assert_eq!(Kind::CameraProfile.as_str(), "camera_profile");
    }
}
