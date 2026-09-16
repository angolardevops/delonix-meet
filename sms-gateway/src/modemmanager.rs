//! Transporte `modemmanager`: o modem é gerido pelo ModemManager e fala-se com
//! ele pelo `mmcli`.
//!
//! O `mmcli` é chamado SEMPRE com um vector de argumentos (`tokio::process::Command`),
//! nunca através de uma shell.
//!
//! **Como passa o texto da mensagem.** O `mmcli` aceita
//! `--messaging-create-sms="text='…',number='…'"`, mas esse formato chave=valor
//! não tem escape documentado para a plica. Por isso o texto vai por ficheiro —
//! `--messaging-create-sms-with-text=<ficheiro>` — escrito com permissões `0600`
//! em `$XDG_RUNTIME_DIR` (tmpfs privado do utilizador, nunca `/tmp`) e apagado a
//! seguir. Se `$XDG_RUNTIME_DIR` não existir, o texto vai em linha e **uma
//! mensagem com plica é recusada** com erro claro, em vez de chegar truncada.
//! O número só passa em linha depois de validado (dígitos e `+` inicial).

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use tokio::process::Command;

const MMCLI: &str = "mmcli";

/// Um modem tal como o `mmcli -m <idx> -J` o descreve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmModem {
    pub index: String,
    /// Caminho sysfs do dispositivo USB (`/sys/devices/…/1-2`).
    pub device: String,
    pub primary_port: Option<String>,
    pub state: String,
    pub operator_name: Option<String>,
    pub signal_percent: Option<u8>,
}

impl MmModem {
    /// O caminho sysfs aponta para esta pasta de `/sys/bus/usb/devices`?
    pub fn matches_usb_dir(&self, dir_name: &str) -> bool {
        Path::new(&self.device)
            .file_name()
            .is_some_and(|n| n == dir_name)
    }

    /// Estado em que o modem NÃO envia, com a razão.
    pub fn unusable_reason(&self) -> Option<String> {
        match self.state.as_str() {
            "failed" => Some("o ModemManager marca o modem como falhado (SIM ausente?)".into()),
            "locked" => Some("SIM bloqueado ou ausente (PIN pedido)".into()),
            _ => None,
        }
    }
}

/// `mmcli -L -J` → índices dos modems (`/org/freedesktop/ModemManager1/Modem/3` → `3`).
pub fn parse_modem_list(json: &str) -> Result<Vec<String>> {
    let v: Value = serde_json::from_str(json).context("mmcli -L -J devolveu JSON inválido")?;
    let list = v
        .get("modem-list")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("mmcli -L -J sem `modem-list`"))?;
    Ok(list
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|p| p.rsplit('/').next())
        .filter(|i| !i.is_empty() && i.bytes().all(|b| b.is_ascii_digit()))
        .map(str::to_string)
        .collect())
}

fn non_empty(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?.trim();
    (!s.is_empty() && s != "--").then(|| s.to_string())
}

/// `mmcli -m <idx> -J` → o que interessa ao inventário.
pub fn parse_modem_info(index: &str, json: &str) -> Result<MmModem> {
    let v: Value = serde_json::from_str(json).context("mmcli -m -J devolveu JSON inválido")?;
    let modem = v
        .get("modem")
        .ok_or_else(|| anyhow!("mmcli -m -J sem `modem`"))?;
    let generic = modem
        .get("generic")
        .ok_or_else(|| anyhow!("mmcli -m -J sem `modem.generic`"))?;
    let device = non_empty(generic.get("device"))
        .ok_or_else(|| anyhow!("mmcli -m -J sem `modem.generic.device`"))?;
    let signal_percent = generic
        .get("signal-quality")
        .and_then(|s| s.get("value"))
        .and_then(|v| match v {
            Value::String(s) => s.trim().parse::<u8>().ok(),
            Value::Number(n) => n.as_u64().and_then(|n| u8::try_from(n).ok()),
            _ => None,
        })
        .filter(|p| *p <= 100);
    Ok(MmModem {
        index: index.to_string(),
        device,
        primary_port: non_empty(generic.get("primary-port")),
        state: non_empty(generic.get("state")).unwrap_or_else(|| "unknown".into()),
        operator_name: non_empty(modem.get("3gpp").and_then(|g| g.get("operator-name"))),
        signal_percent,
    })
}

/// `Successfully created new SMS: /org/freedesktop/ModemManager1/SMS/7 (unknown)` → `7`.
pub fn parse_created_sms(stdout: &str) -> Option<String> {
    const MARK: &str = "/org/freedesktop/ModemManager1/SMS/";
    let at = stdout.find(MARK)? + MARK.len();
    let idx: String = stdout[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!idx.is_empty()).then_some(idx)
}

/// Destino aceitável para passar ao `mmcli`: `+` opcional e 6..=15 dígitos.
pub fn validate_number(to: &str) -> Result<()> {
    let digits = to.strip_prefix('+').unwrap_or(to);
    if (6..=15).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit()) {
        Ok(())
    } else {
        bail!("número de destino inválido para o ModemManager")
    }
}

/// Argumentos de criação do SMS. `text_file` = `Some` → texto por ficheiro.
pub fn create_args(
    index: &str,
    to: &str,
    body: &str,
    text_file: Option<&Path>,
) -> Result<Vec<String>> {
    validate_number(to)?;
    let mut args = vec!["-m".to_string(), index.to_string()];
    match text_file {
        Some(file) => {
            args.push(format!("--messaging-create-sms=number='{to}'"));
            args.push(format!(
                "--messaging-create-sms-with-text={}",
                file.display()
            ));
        }
        None => {
            if body.contains('\'') {
                bail!(
                    "este gateway não tem $XDG_RUNTIME_DIR e o texto em linha do mmcli \
                     não aceita plicas (') — reescreva a mensagem sem plica"
                );
            }
            args.push(format!(
                "--messaging-create-sms=text='{body}',number='{to}'"
            ));
        }
    }
    Ok(args)
}

async fn mmcli(args: &[String]) -> Result<String> {
    let out = Command::new(MMCLI)
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| format!("não foi possível correr {MMCLI}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        bail!(
            "{MMCLI} {} falhou: {err}",
            args.first().map(String::as_str).unwrap_or("")
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Lista os modems geridos. `None` = sem `mmcli` ou sem ModemManager a correr.
pub async fn list_modems() -> Option<Vec<MmModem>> {
    let json = mmcli(&["-L".into(), "-J".into()]).await.ok()?;
    let indices = match parse_modem_list(&json) {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!("resposta inesperada do mmcli: {e:#}");
            return Some(Vec::new());
        }
    };
    let mut modems = Vec::new();
    for idx in indices {
        match mmcli(&["-m".into(), idx.clone(), "-J".into()]).await {
            Ok(json) => match parse_modem_info(&idx, &json) {
                Ok(m) => modems.push(m),
                Err(e) => tracing::warn!("modem {idx}: {e:#}"),
            },
            Err(e) => tracing::warn!("modem {idx}: {e:#}"),
        }
    }
    Some(modems)
}

/// Ficheiro de texto privado para o `mmcli`; apaga-se ao sair de âmbito.
struct TextFile(PathBuf);

impl TextFile {
    async fn create(body: &str) -> Result<Option<Self>> {
        let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else {
            return Ok(None);
        };
        if !dir.is_dir() {
            return Ok(None);
        }
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = dir.join(format!("delonix-sms-{}-{n}.txt", std::process::id()));
        let mut opts = tokio::fs::OpenOptions::new();
        opts.write(true).create_new(true).mode(0o600);
        let mut f = opts
            .open(&path)
            .await
            .with_context(|| format!("não foi possível criar {}", path.display()))?;
        tokio::io::AsyncWriteExt::write_all(&mut f, body.as_bytes()).await?;
        tokio::io::AsyncWriteExt::flush(&mut f).await?;
        Ok(Some(TextFile(path)))
    }
}

impl Drop for TextFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Cria e envia um SMS pelo ModemManager. Devolve o índice do SMS criado.
pub async fn send(index: &str, to: &str, body: &str) -> Result<String> {
    let file = TextFile::create(body).await?;
    let args = create_args(index, to, body, file.as_ref().map(|f| f.0.as_path()))?;
    let out = mmcli(&args).await?;
    drop(file);
    let sms = parse_created_sms(&out)
        .ok_or_else(|| anyhow!("o mmcli não devolveu o SMS criado: {}", out.trim()))?;
    let sent = mmcli(&["-s".into(), sms.clone(), "--send".into()]).await;
    // O objecto SMS fica guardado no modem; limpa-se, com ou sem sucesso.
    let _ = mmcli(&[
        "-m".into(),
        index.to_string(),
        format!("--messaging-delete-sms={sms}"),
    ])
    .await;
    sent?;
    Ok(format!("mm-sms-{sms}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"{"modem-list":["/org/freedesktop/ModemManager1/Modem/0","/org/freedesktop/ModemManager1/Modem/12"]}"#;

    const INFO: &str = r#"{"modem":{"3gpp":{"enabled-locks":["fixed-dialing"],"imei":"867000000000000","operator-code":"63102","operator-name":"UNITEL","registration-state":"home"},"dbus-path":"/org/freedesktop/ModemManager1/Modem/0","generic":{"access-technologies":["umts"],"device":"/sys/devices/pci0000:00/0000:00:14.0/usb1/1-2","drivers":["option"],"manufacturer":"huawei","model":"E173","ports":["ttyUSB0 (at)","ttyUSB2 (at)"],"primary-port":"ttyUSB0","signal-quality":{"recent":"yes","value":"67"},"state":"registered","unlock-required":"--"}}}"#;

    #[test]
    fn lists_modem_indices() {
        assert_eq!(parse_modem_list(LIST).unwrap(), vec!["0", "12"]);
        assert!(parse_modem_list(r#"{"modem-list":[]}"#).unwrap().is_empty());
        assert!(parse_modem_list("não é json").is_err());
    }

    #[test]
    fn parses_modem_info() {
        let m = parse_modem_info("0", INFO).unwrap();
        assert_eq!(m.device, "/sys/devices/pci0000:00/0000:00:14.0/usb1/1-2");
        assert_eq!(m.operator_name.as_deref(), Some("UNITEL"));
        assert_eq!(m.signal_percent, Some(67));
        assert_eq!(m.state, "registered");
        assert_eq!(m.primary_port.as_deref(), Some("ttyUSB0"));
        assert!(m.matches_usb_dir("1-2"));
        assert!(!m.matches_usb_dir("1-2.1"));
        assert_eq!(m.unusable_reason(), None);
    }

    #[test]
    fn locked_modem_without_operator() {
        let json = r#"{"modem":{"3gpp":{"operator-name":"--"},"generic":{"device":"/sys/devices/x/3-1","state":"locked","signal-quality":{"value":"0"}}}}"#;
        let m = parse_modem_info("3", json).unwrap();
        assert_eq!(m.operator_name, None);
        assert!(m.unusable_reason().unwrap().contains("SIM"));
        let failed = MmModem {
            state: "failed".into(),
            ..m
        };
        assert!(failed.unusable_reason().is_some());
    }

    #[test]
    fn parses_created_sms_path() {
        assert_eq!(
            parse_created_sms(
                "Successfully created new SMS: /org/freedesktop/ModemManager1/SMS/7 (unknown)\n"
            ),
            Some("7".into())
        );
        assert_eq!(parse_created_sms("error"), None);
    }

    #[test]
    fn create_args_never_inline_text_when_file_is_available() {
        let file = Path::new("/run/user/1000/delonix-sms-1.txt");
        let args = create_args("0", "+244923000000", "olá, it's ok", Some(file)).unwrap();
        assert_eq!(
            args,
            vec![
                "-m",
                "0",
                "--messaging-create-sms=number='+244923000000'",
                "--messaging-create-sms-with-text=/run/user/1000/delonix-sms-1.txt",
            ]
        );
        assert!(!args.iter().any(|a| a.contains("it's")));
    }

    #[test]
    fn inline_text_rejects_single_quote() {
        let args = create_args("0", "+244923000000", "olá", None).unwrap();
        assert_eq!(
            args[2],
            "--messaging-create-sms=text='olá',number='+244923000000'"
        );
        let err = create_args("0", "+244923000000", "it's", None).unwrap_err();
        assert!(err.to_string().contains("plica"));
    }

    #[test]
    fn number_validation_blocks_injection() {
        for bad in ["+244923'000", "244,text='x'", "", "+", "12345"] {
            assert!(validate_number(bad).is_err(), "{bad}");
        }
        assert!(validate_number("+244923000000").is_ok());
        assert!(create_args("0", "1',text='x", "a", None).is_err());
    }
}
