//! `delonix-sms-gateway` — agente que corre onde o telefone/modem está ligado.
//!
//! Descobre os dispositivos USB, diz quais podem enviar SMS (e porque não, quando
//! não podem), reporta o inventário ao Delonix Meet e envia as mensagens que o
//! servidor lhe entrega. Liga-se sempre para fora: não abre portas. Ver ADR-0005.

mod at;
mod client;
mod inventory;
mod modemmanager;
mod usb;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::Parser;
use tokio::task::JoinSet;

use client::{ClaimedMessage, Client, Secret, SendResult};
use inventory::{Device, Inventory, ProbeOptions, Route, SendTarget};

#[derive(Parser, Debug)]
#[command(
    name = "delonix-sms-gateway",
    version,
    about = "Gateway de SMS do Delonix Meet: telefone ou modem GSM por USB"
)]
struct Args {
    /// URL do Delonix Meet (ex.: https://meet.exemplo.ao).
    #[arg(long, env = "DELONIX_SMS_SERVER")]
    server: Option<String>,

    /// Token do gateway (`dlxg_…`), criado na consola. Prefira a variável de ambiente.
    #[arg(long, env = "DELONIX_SMS_TOKEN", hide_env_values = true)]
    token: Option<String>,

    /// Raiz dos dispositivos USB no sysfs.
    #[arg(long, default_value = "/sys/bus/usb/devices")]
    sysfs_root: PathBuf,

    /// Diagnóstico: lê os dispositivos, sonda-os, imprime e sai. Não precisa de servidor.
    #[arg(long)]
    once: bool,
}

const DEFAULT_REPORT_EVERY: Duration = Duration::from_secs(5);
const CLAIM_EVERY: Duration = Duration::from_secs(3);
/// Quanto se espera pelos envios em curso ao receber SIGINT/SIGTERM
/// (um envio AT pode levar até 60 s à espera da rede).
const SHUTDOWN_GRACE: Duration = Duration::from_secs(70);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();
    if args.once {
        return run_once(args.sysfs_root).await;
    }
    let (Some(server), Some(token)) = (args.server.as_deref(), args.token.as_deref()) else {
        bail!(
            "faltam --server/DELONIX_SMS_SERVER e --token/DELONIX_SMS_TOKEN \
             (para só ver os dispositivos, use --once)"
        );
    };
    let client = Client::new(server, Secret::gateway_token(token)?)?;
    run(client, args.sysfs_root).await
}

async fn run_once(sysfs_root: PathBuf) -> Result<()> {
    // Sem espera pelo ModemManager: quem corre o diagnóstico quer a resposta já.
    let mut inv = Inventory::new(
        sysfs_root,
        ProbeOptions {
            mm_grace: Duration::ZERO,
            refresh_every: Duration::from_secs(60),
        },
    );
    inv.refresh().await?;
    print!("{}", render_table(inv.devices()));
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({ "devices": inv.devices() }))?
    );
    Ok(())
}

fn cell(v: Option<&str>) -> String {
    v.filter(|s| !s.is_empty()).unwrap_or("-").to_string()
}

/// Tabela legível para o diagnóstico `--once`.
fn render_table(devices: &[Device]) -> String {
    if devices.is_empty() {
        return "Nenhum dispositivo USB (além de hubs) encontrado.\n\n".to_string();
    }
    let header = [
        "CHAVE",
        "VID:PID",
        "PRODUTO",
        "TIPO",
        "TRANSPORTE",
        "PORTA",
        "CAPAZ",
        "OPERADOR",
        "SINAL",
    ];
    let rows: Vec<[String; 9]> = devices
        .iter()
        .map(|d| {
            let product = match (d.manufacturer.as_deref(), d.product.as_deref()) {
                (Some(m), Some(p)) if !p.starts_with(m) => format!("{m} {p}"),
                (_, Some(p)) => p.to_string(),
                (Some(m), None) => m.to_string(),
                (None, None) => "-".to_string(),
            };
            [
                d.device_key.clone(),
                format!("{}:{}", d.vendor_id, d.product_id),
                product,
                d.kind.as_str().to_string(),
                d.transport.as_str().to_string(),
                cell(d.port.as_deref()),
                if d.capable { "sim" } else { "não" }.to_string(),
                cell(d.operator_name.as_deref()),
                d.signal_percent
                    .map(|s| format!("{s}%"))
                    .unwrap_or_else(|| "-".into()),
            ]
        })
        .collect();
    let mut widths = header.map(|h| h.chars().count());
    for r in &rows {
        for (w, c) in widths.iter_mut().zip(r.iter()) {
            *w = (*w).max(c.chars().count());
        }
    }
    let line = |cells: &[String]| {
        let mut s = cells
            .iter()
            .zip(widths.iter())
            .map(|(c, w)| {
                let pad = w.saturating_sub(c.chars().count());
                format!("{c}{}", " ".repeat(pad))
            })
            .collect::<Vec<_>>()
            .join("  ");
        s.truncate(s.trim_end().len());
        s.push('\n');
        s
    };
    let mut out = line(&header.map(str::to_string));
    for (r, d) in rows.iter().zip(devices) {
        out.push_str(&line(r));
        if let Some(reason) = &d.reason {
            out.push_str(&format!("    ↳ {reason}\n"));
        }
    }
    let capable = devices.iter().filter(|d| d.capable).count();
    out.push_str(&format!(
        "\n{} dispositivo(s), {} capaz(es) de enviar SMS.\n\n",
        devices.len(),
        capable
    ));
    out
}

async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("sem tratamento de SIGTERM: {e}");
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

fn interval(every: Duration, start_now: bool) -> tokio::time::Interval {
    let start = if start_now {
        tokio::time::Instant::now()
    } else {
        tokio::time::Instant::now() + every
    };
    let mut i = tokio::time::interval_at(start, every);
    i.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    i
}

async fn run(client: Client, sysfs_root: PathBuf) -> Result<()> {
    let client = Arc::new(client);
    let mut inv = Inventory::new(
        sysfs_root,
        ProbeOptions {
            mm_grace: Duration::from_secs(20),
            refresh_every: Duration::from_secs(60),
        },
    );
    inv.refresh()
        .await
        .context("primeira leitura dos dispositivos")?;
    tracing::info!(
        dispositivos = inv.devices().len(),
        capazes = inv.devices().iter().filter(|d| d.capable).count(),
        "gateway de SMS a arrancar"
    );

    let mut report_every = DEFAULT_REPORT_EVERY;
    let mut report_tick = interval(report_every, true);
    let mut claim_tick = interval(CLAIM_EVERY, true);
    let mut sends: JoinSet<()> = JoinSet::new();
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    let mut last_report_ok = true;

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                tracing::info!("sinal de paragem recebido");
                break;
            }
            _ = report_tick.tick() => {
                if let Err(e) = inv.refresh().await {
                    tracing::warn!("falha a ler os dispositivos: {e:#}");
                    continue;
                }
                match client.put_devices(inv.devices()).await {
                    Ok(poll) => {
                        if !last_report_ok {
                            tracing::info!("ligação ao servidor restabelecida");
                        }
                        last_report_ok = true;
                        if let Some(secs) = poll {
                            let wanted = Duration::from_secs(secs.clamp(1, 300));
                            if wanted != report_every {
                                tracing::info!(
                                    segundos = wanted.as_secs(),
                                    "intervalo de reporte ajustado pelo servidor"
                                );
                                report_every = wanted;
                                report_tick = interval(wanted, false);
                            }
                        }
                    }
                    Err(e) => {
                        // Só a primeira falha de uma série vai a `warn`: o
                        // servidor em baixo não deve inundar o registo.
                        if last_report_ok {
                            tracing::warn!("falha a reportar o inventário: {e:#}");
                        }
                        last_report_ok = false;
                    }
                }
            }
            _ = claim_tick.tick() => {
                let messages = match client.claim().await {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::debug!("falha a pedir mensagens: {e:#}");
                        continue;
                    }
                };
                for msg in messages {
                    let target = inv.target(&msg.device_key);
                    let client = client.clone();
                    sends.spawn(async move { deliver(&client, msg, target).await });
                }
            }
            Some(res) = sends.join_next(), if !sends.is_empty() => {
                if let Err(e) = res {
                    tracing::error!("tarefa de envio terminou mal: {e}");
                }
            }
        }
    }

    if !sends.is_empty() {
        tracing::info!(
            em_curso = sends.len(),
            "à espera dos envios em curso antes de sair"
        );
        let wait = async { while sends.join_next().await.is_some() {} };
        if tokio::time::timeout(SHUTDOWN_GRACE, wait).await.is_err() {
            tracing::warn!(
                "envios ainda em curso ao fim de {} s; a sair — o servidor marca-os como falhados",
                SHUTDOWN_GRACE.as_secs()
            );
        }
    }
    Ok(())
}

/// Envia uma mensagem reclamada e reporta o resultado. Nunca repete: o
/// servidor garante «no máximo uma vez» e um reenvio aqui duplicava o SMS.
async fn deliver(
    client: &Client,
    msg: ClaimedMessage,
    target: std::result::Result<SendTarget, String>,
) {
    let outcome: Result<String> = match target {
        Err(reason) => Err(anyhow::anyhow!(reason)),
        Ok(SendTarget { route, lock }) => {
            let _guard = lock.lock_owned().await;
            match route {
                Route::AtSerial { port } => {
                    let pdus = msg.pdus.clone();
                    tokio::task::spawn_blocking(move || at::send_blocking(&port, &pdus))
                        .await
                        .unwrap_or_else(|e| Err(anyhow::anyhow!("tarefa de envio AT: {e}")))
                }
                Route::ModemManager { index } => {
                    modemmanager::send(&index, &msg.to, &msg.body).await
                }
                Route::None => Err(anyhow::anyhow!("dispositivo sem transporte de envio")),
            }
        }
    };
    let result = match outcome {
        Ok(provider_ref) => {
            tracing::info!(mensagem = %msg.id, "SMS enviado");
            SendResult {
                ok: true,
                error: None,
                provider_ref: Some(provider_ref),
            }
        }
        Err(e) => {
            let error = format!("{e:#}");
            tracing::warn!(mensagem = %msg.id, "SMS não enviado: {error}");
            SendResult {
                ok: false,
                error: Some(error),
                provider_ref: None,
            }
        }
    };
    if let Err(e) = client.report_result(&msg.id, &result).await {
        tracing::error!(mensagem = %msg.id, "falha a reportar o resultado: {e:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::Transport;
    use crate::usb::Kind;

    #[test]
    fn table_shows_reason_under_each_device() {
        let d = Device {
            device_key: "18d1:4ee7@3-2".into(),
            vendor_id: "18d1".into(),
            product_id: "4ee7".into(),
            manufacturer: Some("Google".into()),
            product: Some("Pixel 7".into()),
            serial: None,
            kind: Kind::AndroidAdb,
            transport: Transport::None,
            port: None,
            capable: false,
            reason: Some("sem modem".into()),
            operator_name: None,
            signal_percent: None,
        };
        let t = render_table(&[d]);
        assert!(t.contains("android_adb"));
        assert!(t.contains("Google Pixel 7"));
        assert!(t.contains("↳ sem modem"));
        assert!(t.contains("1 dispositivo(s), 0 capaz(es)"));
        assert!(render_table(&[]).contains("Nenhum dispositivo"));
    }

    #[test]
    fn args_parse_once_without_server() {
        let a = Args::try_parse_from(["delonix-sms-gateway", "--once"]).unwrap();
        assert!(a.once);
        assert_eq!(a.sysfs_root, PathBuf::from("/sys/bus/usb/devices"));
    }
}
