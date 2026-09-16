//! Descoberta de dispositivos USB por `sysfs` — sem libusb nem libudev.
//!
//! Lê `/sys/bus/usb/devices/*` (ou outra raiz, para os testes), recolhe os
//! atributos de cada dispositivo e das suas interfaces, e classifica-o numa
//! função pura. A classificação diz o que o dispositivo PARECE ser; se um modem
//! é capaz de enviar decide-o a sonda (`at` ou `modemmanager`), não esta.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Uma interface USB (`<dispositivo>:<cfg>.<n>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbInterface {
    pub name: String,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    /// Texto `interface` (ex.: «MTP»), quando o dispositivo o publica.
    pub label: Option<String>,
    /// Portas série filhas desta interface (`ttyACM0`, `ttyUSB2`, …).
    pub ttys: Vec<String>,
}

/// Um dispositivo USB tal como o `sysfs` o descreve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbDevice {
    /// Nome da pasta em `/sys/bus/usb/devices` (ex.: `1-1.5`, `usb1`).
    pub dir_name: String,
    pub vendor_id: String,
    pub product_id: String,
    pub device_class: u8,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub interfaces: Vec<UsbInterface>,
}

impl UsbDevice {
    /// Todas as portas série do dispositivo, ordenadas, como caminhos `/dev`.
    pub fn tty_paths(&self) -> Vec<String> {
        let mut ttys: Vec<String> = self
            .interfaces
            .iter()
            .flat_map(|i| i.ttys.iter())
            .map(|t| format!("/dev/{t}"))
            .collect();
        ttys.sort_by_key(|t| natural_key(t));
        ttys.dedup();
        ttys
    }
}

/// Ordena `ttyUSB10` depois de `ttyUSB2`.
fn natural_key(s: &str) -> (String, u32) {
    let digits = s.len() - s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (prefix, num) = s.split_at(s.len() - digits);
    (prefix.to_string(), num.parse().unwrap_or(0))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Modem,
    AndroidAdb,
    AndroidMtp,
    MassStorageModem,
    Unknown,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Modem => "modem",
            Kind::AndroidAdb => "android_adb",
            Kind::AndroidMtp => "android_mtp",
            Kind::MassStorageModem => "mass_storage_modem",
            Kind::Unknown => "unknown",
        }
    }
}

/// Resultado da classificação. `reason` só vem preenchido quando o tipo, por si,
/// já decide que o dispositivo NÃO é capaz; para `Modem` decide a sonda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub kind: Kind,
    pub reason: Option<String>,
}

pub const REASON_ADB: &str = "telefone Android com depuração USB (ADB): o Android não expõe \
por USB um modem de SMS suportado, e o ADB não envia SMS sem root. Use uma pen/modem GSM USB \
ou um telefone que exponha uma porta AT";
pub const REASON_MTP: &str = "telefone Android em modo de ficheiros (MTP/PTP), sem modem \
exposto: nenhuma opção do telefone resolve isto sem uma porta AT. Use uma pen/modem GSM USB \
ou um telefone que exponha uma porta AT";
pub const REASON_MASS_STORAGE: &str = "modem em modo de armazenamento (CD virtual): precisa \
de usb_modeswitch para passar a modo modem";
pub const REASON_UNKNOWN: &str = "dispositivo sem porta série nem modem reconhecível \
(se for um telefone, pode estar só a carregar)";

const CLASS_MASS_STORAGE: u8 = 0x08;
const CLASS_HUB: u8 = 0x09;
const CLASS_STILL_IMAGE: u8 = 0x06;
const VENDOR_HUAWEI: &str = "12d1";
const VENDOR_ZTE: &str = "19d2";

/// Classifica um dispositivo. `None` = ignorar (hubs e root hubs).
///
/// Ordem: hub → porta série (candidato a modem) → ADB → MTP/PTP → modem em
/// modo de armazenamento → desconhecido. A porta série vem ANTES do ADB de
/// propósito: um telefone que exponha ADB **e** uma porta AT é um candidato
/// legítimo, e quem decide se envia é a sonda.
pub fn classify(dev: &UsbDevice) -> Option<Classification> {
    if dev.dir_name.starts_with("usb")
        || dev.device_class == CLASS_HUB
        || dev.interfaces.iter().any(|i| i.class == CLASS_HUB)
    {
        return None;
    }
    let has_tty = dev.interfaces.iter().any(|i| !i.ttys.is_empty());
    if has_tty {
        return Some(Classification {
            kind: Kind::Modem,
            reason: None,
        });
    }
    let adb = dev
        .interfaces
        .iter()
        .any(|i| i.class == 0xff && i.subclass == 0x42 && i.protocol == 0x01);
    if adb {
        return Some(Classification {
            kind: Kind::AndroidAdb,
            reason: Some(REASON_ADB.to_string()),
        });
    }
    let mtp_label = |s: &Option<String>| {
        s.as_deref()
            .is_some_and(|v| v.to_ascii_uppercase().contains("MTP"))
    };
    let mtp = dev.interfaces.iter().any(|i| i.class == CLASS_STILL_IMAGE)
        || mtp_label(&dev.product)
        || dev.interfaces.iter().any(|i| mtp_label(&i.label));
    if mtp {
        return Some(Classification {
            kind: Kind::AndroidMtp,
            reason: Some(REASON_MTP.to_string()),
        });
    }
    let only_mass_storage =
        !dev.interfaces.is_empty() && dev.interfaces.iter().all(|i| i.class == CLASS_MASS_STORAGE);
    if (dev.vendor_id == VENDOR_HUAWEI || dev.vendor_id == VENDOR_ZTE) && only_mass_storage {
        return Some(Classification {
            kind: Kind::MassStorageModem,
            reason: Some(REASON_MASS_STORAGE.to_string()),
        });
    }
    Some(Classification {
        kind: Kind::Unknown,
        reason: Some(REASON_UNKNOWN.to_string()),
    })
}

/// Chave estável entre ligações: `vvvv:pppp:<série>` quando o número de série
/// identifica alguma coisa; senão `vvvv:pppp@<pasta sysfs>` (a pasta é o caminho
/// USB — a porta física).
///
/// Há fabricantes que gravam lixo no número de série — medido nesta máquina a
/// 2026-09-16: `000000000` num adaptador sem fios e `SN0001` numa webcam. Dois
/// aparelhos iguais com o mesmo lixo seriam UM dispositivo para o servidor, e a
/// selecção de um passava para o outro.
pub fn device_key(dev: &UsbDevice) -> String {
    match dev.serial.as_deref().map(str::trim) {
        Some(s) if serial_is_meaningful(s) => {
            format!("{}:{}:{}", dev.vendor_id, dev.product_id, s)
        }
        _ => format!("{}:{}@{}", dev.vendor_id, dev.product_id, dev.dir_name),
    }
}

fn serial_is_meaningful(s: &str) -> bool {
    let first = s.chars().next();
    s.len() >= 8
        && !s.chars().all(|c| Some(c) == first)
        && !s.eq_ignore_ascii_case("0123456789")
        && !s.to_ascii_uppercase().starts_with("SN000")
}

fn read_attr(dir: &Path, name: &str) -> Option<String> {
    let v = fs::read_to_string(dir.join(name)).ok()?;
    let v = v.trim();
    (!v.is_empty()).then(|| v.to_string())
}

fn read_hex_u8(dir: &Path, name: &str) -> u8 {
    read_attr(dir, name)
        .and_then(|v| u8::from_str_radix(&v, 16).ok())
        .unwrap_or(0)
}

fn list_dir_names(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = rd
        .filter_map(Result::ok)
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    out.sort();
    out
}

fn is_tty_name(name: &str) -> bool {
    name.starts_with("ttyUSB") || name.starts_with("ttyACM")
}

fn read_interface(name: String, path: &Path) -> UsbInterface {
    let mut ttys = Vec::new();
    for (child, child_path) in list_dir_names(path) {
        if is_tty_name(&child) {
            // usb-serial (option, qcserial): `<iface>/ttyUSB0`
            ttys.push(child);
        } else if child == "tty" {
            // cdc_acm: `<iface>/tty/ttyACM0`
            ttys.extend(
                list_dir_names(&child_path)
                    .into_iter()
                    .map(|(n, _)| n)
                    .filter(|n| is_tty_name(n)),
            );
        }
    }
    UsbInterface {
        name,
        class: read_hex_u8(path, "bInterfaceClass"),
        subclass: read_hex_u8(path, "bInterfaceSubClass"),
        protocol: read_hex_u8(path, "bInterfaceProtocol"),
        label: read_attr(path, "interface"),
        ttys,
    }
}

/// Enumera os dispositivos (pastas com `idVendor`) debaixo de `root`.
pub fn scan(root: &Path) -> io::Result<Vec<UsbDevice>> {
    // Falha cedo e com o caminho se a raiz não existir: sem isto, uma raiz
    // errada dava um inventário vazio indistinguível de «nada ligado».
    fs::read_dir(root).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("não foi possível ler {}: {e}", root.display()),
        )
    })?;
    let mut devices = Vec::new();
    for (name, path) in list_dir_names(root) {
        if name.contains(':') {
            continue; // interface, não dispositivo
        }
        let Some(vendor_id) = read_attr(&path, "idVendor") else {
            continue;
        };
        let prefix = format!("{name}:");
        let interfaces = list_dir_names(&path)
            .into_iter()
            .filter(|(n, p)| n.starts_with(&prefix) && p.is_dir())
            .map(|(n, p)| read_interface(n, &p))
            .collect();
        devices.push(UsbDevice {
            vendor_id: vendor_id.to_ascii_lowercase(),
            product_id: read_attr(&path, "idProduct")
                .unwrap_or_default()
                .to_ascii_lowercase(),
            device_class: read_hex_u8(&path, "bDeviceClass"),
            manufacturer: read_attr(&path, "manufacturer"),
            product: read_attr(&path, "product"),
            serial: read_attr(&path, "serial"),
            interfaces,
            dir_name: name,
        });
    }
    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(class: u8, subclass: u8, protocol: u8, ttys: &[&str]) -> UsbInterface {
        UsbInterface {
            name: "x:1.0".into(),
            class,
            subclass,
            protocol,
            label: None,
            ttys: ttys.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn dev(vendor: &str, class: u8, interfaces: Vec<UsbInterface>) -> UsbDevice {
        UsbDevice {
            dir_name: "1-2".into(),
            vendor_id: vendor.into(),
            product_id: "0001".into(),
            device_class: class,
            manufacturer: None,
            product: None,
            serial: None,
            interfaces,
        }
    }

    #[test]
    fn hubs_and_root_hubs_are_skipped() {
        assert_eq!(
            classify(&dev("1a86", 0x09, vec![iface(9, 0, 0, &[])])),
            None
        );
        let mut root = dev("1d6b", 0x09, vec![]);
        root.dir_name = "usb1".into();
        assert_eq!(classify(&root), None);
        // dispositivo de classe 00 cujas interfaces são hub
        assert_eq!(classify(&dev("0a12", 0, vec![iface(9, 0, 0, &[])])), None);
    }

    #[test]
    fn adb_phone_is_not_capable_and_says_why() {
        let d = dev(
            "18d1",
            0,
            vec![iface(0xff, 0xff, 0, &[]), iface(0xff, 0x42, 0x01, &[])],
        );
        let c = classify(&d).unwrap();
        assert_eq!(c.kind, Kind::AndroidAdb);
        assert!(c.reason.unwrap().contains("ADB"));
    }

    #[test]
    fn mtp_phone_by_class_by_product_or_by_interface_label() {
        let by_class = dev("04e8", 0, vec![iface(6, 1, 1, &[])]);
        assert_eq!(classify(&by_class).unwrap().kind, Kind::AndroidMtp);

        let mut by_product = dev("04e8", 0, vec![iface(0xff, 0xff, 0, &[])]);
        by_product.product = Some("SAMSUNG_Android MTP".into());
        assert_eq!(classify(&by_product).unwrap().kind, Kind::AndroidMtp);

        let mut labelled = iface(0xff, 0xff, 0, &[]);
        labelled.label = Some("MTP".into());
        let by_label = dev("2717", 0, vec![labelled]);
        assert_eq!(classify(&by_label).unwrap().kind, Kind::AndroidMtp);
    }

    #[test]
    fn tty_makes_a_modem_candidate_even_with_adb() {
        let d = dev(
            "1e0e",
            0,
            vec![
                iface(0xff, 0x42, 0x01, &[]),
                iface(0xff, 0, 0, &["ttyUSB2"]),
            ],
        );
        let c = classify(&d).unwrap();
        assert_eq!(c.kind, Kind::Modem);
        assert_eq!(c.reason, None);
    }

    #[test]
    fn huawei_and_zte_in_mass_storage_need_modeswitch() {
        for v in ["12d1", "19d2"] {
            let d = dev(v, 0, vec![iface(8, 6, 0x50, &[])]);
            let c = classify(&d).unwrap();
            assert_eq!(c.kind, Kind::MassStorageModem, "vendor {v}");
            assert!(c.reason.unwrap().contains("usb_modeswitch"));
        }
        // uma pen de outro fabricante é só armazenamento
        let other = dev("0781", 0, vec![iface(8, 6, 0x50, &[])]);
        assert_eq!(classify(&other).unwrap().kind, Kind::Unknown);
    }

    #[test]
    fn everything_else_is_unknown() {
        let webcam = dev("3277", 0xef, vec![iface(0x0e, 1, 0, &[])]);
        let c = classify(&webcam).unwrap();
        assert_eq!(c.kind, Kind::Unknown);
        assert!(c.reason.is_some());
    }

    #[test]
    fn device_key_prefers_serial() {
        let mut d = dev("12d1", 0, vec![]);
        d.product_id = "1506".into();
        assert_eq!(device_key(&d), "12d1:1506@1-2");
        d.serial = Some("   ".into());
        assert_eq!(device_key(&d), "12d1:1506@1-2");
        d.serial = Some("RFCX61J9F5V".into());
        assert_eq!(device_key(&d), "12d1:1506:RFCX61J9F5V");
        // Lixo medido em aparelhos reais: não identifica nada.
        for junk in ["000000000", "SN0001", "ABC123", "FFFFFFFFFFFF"] {
            d.serial = Some(junk.into());
            assert_eq!(device_key(&d), "12d1:1506@1-2", "{junk}");
        }
    }

    #[test]
    fn tty_paths_are_sorted_naturally() {
        let d = dev(
            "12d1",
            0,
            vec![
                iface(0xff, 0, 0, &["ttyUSB10"]),
                iface(0xff, 0, 0, &["ttyUSB2"]),
                iface(2, 2, 1, &["ttyACM0"]),
            ],
        );
        assert_eq!(
            d.tty_paths(),
            vec!["/dev/ttyACM0", "/dev/ttyUSB2", "/dev/ttyUSB10"]
        );
    }

    /// Árvore sysfs falsa, construída dentro de `target/` do crate (nunca /tmp).
    struct FakeSysfs(PathBuf);

    impl FakeSysfs {
        fn new(name: &str) -> Self {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("test-sysfs")
                .join(name);
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            FakeSysfs(root)
        }
        fn attrs(&self, rel: &str, attrs: &[(&str, &str)]) -> PathBuf {
            let p = self.0.join(rel);
            fs::create_dir_all(&p).unwrap();
            for (k, v) in attrs {
                fs::write(p.join(k), format!("{v}\n")).unwrap();
            }
            p
        }
    }

    impl Drop for FakeSysfs {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn scan_reads_a_fake_sysfs_tree() {
        let fs_ = FakeSysfs::new("scan");
        // root hub
        fs_.attrs(
            "usb1",
            &[
                ("idVendor", "1d6b"),
                ("idProduct", "0002"),
                ("bDeviceClass", "09"),
            ],
        );
        // interface solta ao nível da raiz (como no sysfs real) — não é dispositivo
        fs_.attrs("1-0:1.0", &[("bInterfaceClass", "09")]);
        // pen Huawei em modo modem: usb-serial com ttyUSB0..2
        fs_.attrs(
            "1-2",
            &[
                ("idVendor", "12d1"),
                ("idProduct", "1506"),
                ("bDeviceClass", "00"),
                ("manufacturer", "HUAWEI"),
                ("product", "HUAWEI Mobile"),
            ],
        );
        fs_.attrs(
            "1-2/1-2:1.0",
            &[
                ("bInterfaceClass", "ff"),
                ("bInterfaceSubClass", "02"),
                ("bInterfaceProtocol", "01"),
            ],
        );
        fs_.attrs("1-2/1-2:1.0/ttyUSB0", &[]);
        fs_.attrs("1-2/1-2:1.2", &[("bInterfaceClass", "ff")]);
        fs_.attrs("1-2/1-2:1.2/ttyUSB2", &[]);
        // modem CDC-ACM: `<iface>/tty/ttyACM0`, com número de série
        fs_.attrs(
            "3-1",
            &[
                ("idVendor", "1E0E"),
                ("idProduct", "9001"),
                ("bDeviceClass", "ef"),
                ("serial", "SIM7600-XYZ"),
            ],
        );
        fs_.attrs("3-1/3-1:1.0", &[("bInterfaceClass", "02")]);
        fs_.attrs("3-1/3-1:1.0/tty/ttyACM0", &[]);
        // Android com ADB
        fs_.attrs(
            "3-2",
            &[("idVendor", "18d1"), ("idProduct", "4ee7"), ("serial", "")],
        );
        fs_.attrs(
            "3-2/3-2:1.0",
            &[
                ("bInterfaceClass", "ff"),
                ("bInterfaceSubClass", "ff"),
                ("bInterfaceProtocol", "00"),
                ("interface", "MTP"),
            ],
        );
        fs_.attrs(
            "3-2/3-2:1.1",
            &[
                ("bInterfaceClass", "ff"),
                ("bInterfaceSubClass", "42"),
                ("bInterfaceProtocol", "01"),
            ],
        );

        let devices = scan(&fs_.0).unwrap();
        let names: Vec<&str> = devices.iter().map(|d| d.dir_name.as_str()).collect();
        assert_eq!(names, vec!["1-2", "3-1", "3-2", "usb1"]);

        let huawei = &devices[0];
        assert_eq!(huawei.manufacturer.as_deref(), Some("HUAWEI"));
        assert_eq!(huawei.interfaces.len(), 2);
        assert_eq!(huawei.tty_paths(), vec!["/dev/ttyUSB0", "/dev/ttyUSB2"]);
        assert_eq!(classify(huawei).unwrap().kind, Kind::Modem);
        assert_eq!(device_key(huawei), "12d1:1506@1-2");

        let acm = &devices[1];
        assert_eq!(
            acm.vendor_id, "1e0e",
            "idVendor normalizado para minúsculas"
        );
        assert_eq!(acm.tty_paths(), vec!["/dev/ttyACM0"]);
        assert_eq!(device_key(acm), "1e0e:9001:SIM7600-XYZ");

        let phone = &devices[2];
        assert_eq!(phone.serial, None, "serial vazio conta como ausente");
        assert_eq!(classify(phone).unwrap().kind, Kind::AndroidAdb);

        assert_eq!(classify(&devices[3]), None);
    }

    #[test]
    fn scan_of_missing_root_is_an_error() {
        let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/não-existe-sysfs");
        let err = scan(&missing).unwrap_err();
        assert!(err.to_string().contains("não-existe-sysfs"));
    }
}
