//! Inventário: junta a descoberta USB, o ModemManager e a sonda AT no `Device`
//! que o ADR-0005 define, e guarda por dispositivo a rota de envio e o cadeado
//! que serializa o acesso à porta.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::at::{self, AtProbe};
use crate::modemmanager::{self, MmModem};
use crate::usb::{self, Kind, UsbDevice};

/// `Device` do ADR-0005 — o que o agente reporta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Device {
    pub device_key: String,
    pub vendor_id: String,
    pub product_id: String,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub kind: Kind,
    pub transport: Transport,
    pub port: Option<String>,
    pub capable: bool,
    pub reason: Option<String>,
    pub operator_name: Option<String>,
    pub signal_percent: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    AtSerial,
    Modemmanager,
    None,
}

impl Transport {
    pub fn as_str(self) -> &'static str {
        match self {
            Transport::AtSerial => "at_serial",
            Transport::Modemmanager => "modemmanager",
            Transport::None => "none",
        }
    }
}

/// Como se envia por um dispositivo (interno; não vai para o servidor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    AtSerial { port: String },
    ModemManager { index: String },
    None,
}

/// Rota e cadeado de um dispositivo capaz, prontos para um envio.
#[derive(Clone)]
pub struct SendTarget {
    pub route: Route,
    pub lock: Arc<Mutex<()>>,
}

/// Opções de sondagem.
#[derive(Debug, Clone, Copy)]
pub struct ProbeOptions {
    /// Quanto esperar, depois de um dispositivo aparecer, antes de lhe falar AT
    /// quando o ModemManager está activo — ele sonda as portas nos primeiros
    /// segundos, e dois diálogos AT na mesma porta corrompem-se.
    pub mm_grace: Duration,
    /// Re-sonda AT periodicamente (sinal, operador) mesmo sem mudanças.
    pub refresh_every: Duration,
}

struct Cached {
    fingerprint: String,
    probe: AtProbe,
    at: Instant,
}

pub struct Inventory {
    sysfs_root: PathBuf,
    opts: ProbeOptions,
    first_seen: HashMap<String, Instant>,
    at_cache: HashMap<String, Cached>,
    locks: HashMap<String, Arc<Mutex<()>>>,
    routes: HashMap<String, Route>,
    devices: Vec<Device>,
}

const REASON_WAIT_MM: &str = "a aguardar o ModemManager (dispositivo acabado de ligar)";
const REASON_NO_TTY: &str = "candidato a modem sem porta série";
const REASON_BUSY: &str = "porta ocupada com um envio; ainda não sondada";

impl Inventory {
    pub fn new(sysfs_root: PathBuf, opts: ProbeOptions) -> Self {
        Inventory {
            sysfs_root,
            opts,
            first_seen: HashMap::new(),
            at_cache: HashMap::new(),
            locks: HashMap::new(),
            routes: HashMap::new(),
            devices: Vec::new(),
        }
    }

    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    /// Rota de envio para um `device_key`, se o dispositivo está ligado e capaz.
    /// `Err` traz a razão para devolver ao servidor.
    pub fn target(&self, device_key: &str) -> std::result::Result<SendTarget, String> {
        let Some(device) = self.devices.iter().find(|d| d.device_key == device_key) else {
            return Err("dispositivo não está ligado a este gateway".to_string());
        };
        if !device.capable {
            return Err(format!(
                "dispositivo não está capaz de enviar: {}",
                device.reason.as_deref().unwrap_or("sem razão")
            ));
        }
        match (self.routes.get(device_key), self.locks.get(device_key)) {
            (Some(route), Some(lock)) if *route != Route::None => Ok(SendTarget {
                route: route.clone(),
                lock: lock.clone(),
            }),
            _ => Err("dispositivo sem transporte de envio".to_string()),
        }
    }

    fn lock_for(&mut self, key: &str) -> Arc<Mutex<()>> {
        self.locks
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Volta a ler o USB, o ModemManager e (quando preciso) sonda AT.
    pub async fn refresh(&mut self) -> Result<()> {
        let root = self.sysfs_root.clone();
        let usb_devices = tokio::task::spawn_blocking(move || usb::scan(&root))
            .await
            .context("tarefa de leitura do sysfs")??;
        let mm = modemmanager::list_modems().await;
        let mm_active = mm.is_some();
        let mm_modems = mm.unwrap_or_default();

        let now = Instant::now();
        let mut present = HashSet::new();
        let mut devices = Vec::new();
        let mut routes = HashMap::new();

        for dev in &usb_devices {
            let key = usb::device_key(dev);
            let mm = mm_modems.iter().find(|m| m.matches_usb_dir(&dev.dir_name));
            let Some(class) = usb::classify(dev) else {
                continue;
            };
            present.insert(key.clone());
            let first_seen = *self.first_seen.entry(key.clone()).or_insert(now);
            let lock = self.lock_for(&key);

            let (device, route) = if let Some(mm) = mm {
                from_modemmanager(dev, &key, mm)
            } else if class.kind == Kind::Modem {
                let ttys = dev.tty_paths();
                if ttys.is_empty() {
                    let d = base_device(dev, &key, Kind::Modem, Some(REASON_NO_TTY.into()));
                    (d, Route::None)
                } else if mm_active && now.duration_since(first_seen) < self.opts.mm_grace {
                    let d = base_device(dev, &key, Kind::Modem, Some(REASON_WAIT_MM.into()));
                    (d, Route::None)
                } else {
                    let probe = self.at_probe(&key, &ttys, lock).await;
                    from_at_probe(dev, &key, probe)
                }
            } else {
                (
                    base_device(dev, &key, class.kind, class.reason),
                    Route::None,
                )
            };
            routes.insert(key, route);
            devices.push(device);
        }

        // Esquece o que foi desligado — ao voltar, conta como novo.
        self.first_seen.retain(|k, _| present.contains(k));
        self.at_cache.retain(|k, _| present.contains(k));
        self.locks
            .retain(|k, l| present.contains(k) || Arc::strong_count(l) > 1);
        self.devices = devices;
        self.routes = routes;
        Ok(())
    }

    /// Sonda AT com cache. Re-sonda só quando o conjunto de portas muda ou a
    /// sonda envelheceu, e NUNCA enquanto um envio segura o cadeado da porta.
    async fn at_probe(&mut self, key: &str, ttys: &[String], lock: Arc<Mutex<()>>) -> AtProbe {
        let fingerprint = ttys.join(",");
        let fresh = self
            .at_cache
            .get(key)
            .filter(|c| c.fingerprint == fingerprint && c.at.elapsed() < self.opts.refresh_every);
        if let Some(c) = fresh {
            return c.probe.clone();
        }
        let Ok(guard) = lock.try_lock_owned() else {
            return match self.at_cache.get(key) {
                Some(c) => c.probe.clone(),
                None => AtProbe {
                    port: ttys[0].clone(),
                    answered: false,
                    capable: false,
                    reason: Some(REASON_BUSY.into()),
                    operator_name: None,
                    signal_percent: None,
                },
            };
        };
        let ports = ttys.to_vec();
        let probe = tokio::task::spawn_blocking(move || {
            let _guard = guard;
            at::probe_ports_blocking(&ports)
        })
        .await
        .unwrap_or_else(|e| AtProbe {
            port: String::new(),
            answered: false,
            capable: false,
            reason: Some(format!("a sonda AT falhou: {e}")),
            operator_name: None,
            signal_percent: None,
        });
        self.at_cache.insert(
            key.to_string(),
            Cached {
                fingerprint,
                probe: probe.clone(),
                at: Instant::now(),
            },
        );
        probe
    }
}

fn base_device(dev: &UsbDevice, key: &str, kind: Kind, reason: Option<String>) -> Device {
    Device {
        device_key: key.to_string(),
        vendor_id: dev.vendor_id.clone(),
        product_id: dev.product_id.clone(),
        manufacturer: dev.manufacturer.clone(),
        product: dev.product.clone(),
        serial: dev.serial.clone(),
        kind,
        transport: Transport::None,
        port: dev.tty_paths().into_iter().next(),
        capable: false,
        reason,
        operator_name: None,
        signal_percent: None,
    }
}

/// Dispositivo gerido pelo ModemManager. Pura — testada.
pub fn from_modemmanager(dev: &UsbDevice, key: &str, mm: &MmModem) -> (Device, Route) {
    let reason = mm.unusable_reason();
    let capable = reason.is_none();
    let device = Device {
        kind: Kind::Modem,
        transport: Transport::Modemmanager,
        port: mm
            .primary_port
            .as_ref()
            .map(|p| format!("/dev/{p}"))
            .or_else(|| dev.tty_paths().into_iter().next()),
        capable,
        reason,
        operator_name: mm.operator_name.clone(),
        signal_percent: mm.signal_percent,
        ..base_device(dev, key, Kind::Modem, None)
    };
    let route = if capable {
        Route::ModemManager {
            index: mm.index.clone(),
        }
    } else {
        Route::None
    };
    (device, route)
}

/// Dispositivo sondado por AT. Pura — testada.
pub fn from_at_probe(dev: &UsbDevice, key: &str, probe: AtProbe) -> (Device, Route) {
    let port = (!probe.port.is_empty()).then(|| probe.port.clone());
    let device = Device {
        transport: if probe.answered {
            Transport::AtSerial
        } else {
            Transport::None
        },
        port: port.clone().or_else(|| dev.tty_paths().into_iter().next()),
        capable: probe.capable,
        reason: probe.reason,
        operator_name: probe.operator_name,
        signal_percent: probe.signal_percent,
        ..base_device(dev, key, Kind::Modem, None)
    };
    let route = match (probe.capable, port) {
        (true, Some(port)) => Route::AtSerial { port },
        _ => Route::None,
    };
    (device, route)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb::UsbInterface;

    fn huawei() -> UsbDevice {
        UsbDevice {
            dir_name: "1-2".into(),
            vendor_id: "12d1".into(),
            product_id: "1506".into(),
            device_class: 0,
            manufacturer: Some("HUAWEI".into()),
            product: Some("HUAWEI Mobile".into()),
            serial: None,
            interfaces: vec![UsbInterface {
                name: "1-2:1.0".into(),
                class: 0xff,
                subclass: 2,
                protocol: 1,
                label: None,
                ttys: vec!["ttyUSB0".into(), "ttyUSB2".into()],
            }],
        }
    }

    #[test]
    fn device_serialises_to_the_adr_shape() {
        let d = base_device(&huawei(), "12d1:1506@1-2", Kind::Unknown, Some("x".into()));
        let v = serde_json::to_value(&d).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        let mut expected = vec![
            "device_key",
            "vendor_id",
            "product_id",
            "manufacturer",
            "product",
            "serial",
            "kind",
            "transport",
            "port",
            "capable",
            "reason",
            "operator_name",
            "signal_percent",
        ];
        expected.sort();
        assert_eq!(keys, expected);
        assert_eq!(v["kind"], "unknown");
        assert_eq!(v["transport"], "none");
        assert_eq!(v["serial"], serde_json::Value::Null);
    }

    #[test]
    fn modemmanager_device_is_capable_unless_locked() {
        let mm = MmModem {
            index: "0".into(),
            device: "/sys/devices/x/1-2".into(),
            primary_port: Some("ttyUSB0".into()),
            state: "registered".into(),
            operator_name: Some("UNITEL".into()),
            signal_percent: Some(67),
        };
        let (d, r) = from_modemmanager(&huawei(), "k", &mm);
        assert!(d.capable);
        assert_eq!(d.transport, Transport::Modemmanager);
        assert_eq!(d.port.as_deref(), Some("/dev/ttyUSB0"));
        assert_eq!(r, Route::ModemManager { index: "0".into() });

        let locked = MmModem {
            state: "locked".into(),
            ..mm
        };
        let (d, r) = from_modemmanager(&huawei(), "k", &locked);
        assert!(!d.capable);
        assert!(d.reason.is_some());
        assert_eq!(r, Route::None);
    }

    #[test]
    fn at_probe_maps_to_device_and_route() {
        let ok = AtProbe {
            port: "/dev/ttyUSB2".into(),
            answered: true,
            capable: true,
            reason: None,
            operator_name: Some("AFRICELL".into()),
            signal_percent: Some(40),
        };
        let (d, r) = from_at_probe(&huawei(), "k", ok);
        assert_eq!(d.transport, Transport::AtSerial);
        assert_eq!(d.port.as_deref(), Some("/dev/ttyUSB2"));
        assert_eq!(
            r,
            Route::AtSerial {
                port: "/dev/ttyUSB2".into()
            }
        );

        let silent = AtProbe {
            port: "/dev/ttyUSB0".into(),
            answered: false,
            capable: false,
            reason: Some("nenhuma porta série respondeu a AT".into()),
            operator_name: None,
            signal_percent: None,
        };
        let (d, r) = from_at_probe(&huawei(), "k", silent);
        assert_eq!(d.transport, Transport::None);
        assert!(!d.capable);
        assert_eq!(r, Route::None);
    }

    #[tokio::test]
    async fn target_explains_missing_and_incapable_devices() {
        let mut inv = Inventory::new(
            PathBuf::from("/nao-usado"),
            ProbeOptions {
                mm_grace: Duration::ZERO,
                refresh_every: Duration::from_secs(60),
            },
        );
        let err = inv.target("nada").err().unwrap();
        assert_eq!(err, "dispositivo não está ligado a este gateway");

        let d = base_device(&huawei(), "k", Kind::AndroidAdb, Some("sem modem".into()));
        inv.devices = vec![d.clone()];
        inv.routes.insert("k".into(), Route::None);
        assert!(inv.target("k").err().unwrap().contains("sem modem"));

        inv.devices = vec![Device {
            capable: true,
            reason: None,
            ..d
        }];
        inv.routes.insert(
            "k".into(),
            Route::AtSerial {
                port: "/dev/ttyUSB0".into(),
            },
        );
        let lock = inv.lock_for("k");
        let t = inv.target("k").ok().unwrap();
        assert!(Arc::ptr_eq(&t.lock, &lock));
    }
}
