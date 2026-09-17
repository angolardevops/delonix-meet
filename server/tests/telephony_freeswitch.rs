//! Portas `CallOriginator` e `SipControl` contra um FreeSWITCH REAL (ADR-0009).
//!
//! Só corre com o ambiente da prova (ver `web/e2e/telefonia-freeswitch.mjs`,
//! que cria os gateways); sem ele, cada teste diz que NÃO correu e passa —
//! um verde aqui sem essas variáveis não prova nada, e a saída di-lo.
//!
//!   FS_ESL_ADDR=127.0.0.1:8121 FS_ESL_PASSWORD=… FS_GW_DOWN=dlx-<id> FS_GW_UP=dlx-<id> \
//!   cargo test --release --test telephony_freeswitch -- --test-threads=1 --nocapture
//!
//! `FS_GW_DOWN` responde 503 a tudo; `FS_GW_UP` atende (e dá 486 a `…000`);
//! `FS_BRIDGE=127.0.0.1:5190` é o UA que faz de ponte da sala.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use delonix_meet_domain::telephony::ports::{
    trunk_id_from_gateway, AfterAnswer, CallEvent, CallEventSink, CallOriginator, DialLeg,
    OriginateRequest, SipControl,
};
use delonix_server::telephony_esl::{
    EslConfig, EslConn, FreeswitchOriginator, FreeswitchSipControl,
};
use uuid::Uuid;

struct Env {
    esl: EslConfig,
    down: String,
    up: String,
    bridge: std::net::SocketAddr,
}

fn env() -> Option<Env> {
    let v = |k: &str| std::env::var(k).ok().filter(|s| !s.is_empty());
    match (
        v("FS_ESL_ADDR"),
        v("FS_ESL_PASSWORD"),
        v("FS_GW_DOWN"),
        v("FS_GW_UP"),
    ) {
        (Some(addr), Some(password), Some(down), Some(up)) => Some(Env {
            esl: EslConfig {
                addr,
                password,
                sofia_profile: v("FS_PROFILE").unwrap_or_else(|| "external".into()),
            },
            down,
            up,
            bridge: v("FS_BRIDGE")
                .unwrap_or_else(|| "127.0.0.1:5190".into())
                .parse()
                .unwrap(),
        }),
        _ => {
            eprintln!("NÃO CORREU: faltam FS_ESL_ADDR/FS_ESL_PASSWORD/FS_GW_DOWN/FS_GW_UP");
            None
        }
    }
}

#[derive(Default)]
struct Collect(Mutex<Vec<CallEvent>>);

impl CallEventSink for Collect {
    fn on_event(&self, _: Uuid, e: CallEvent) {
        eprintln!("  evento: {e:?}");
        self.0.lock().unwrap().push(e);
    }
}

impl Collect {
    async fn wait_end(&self, secs: u64) -> Vec<CallEvent> {
        for _ in 0..secs * 10 {
            let ev = self.0.lock().unwrap().clone();
            if ev.iter().any(|e| matches!(e, CallEvent::Ended { .. })) {
                return ev;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("sem Ended em {secs} s: {:?}", self.0.lock().unwrap());
    }
}

fn leg(gw: &str, number: &str) -> DialLeg {
    DialLeg {
        trunk_id: trunk_id_from_gateway(gw).expect("gateway dlx-<uuid>"),
        gateway_name: gw.to_string(),
        number: number.to_string(),
    }
}

fn request(legs: Vec<DialLeg>, after: AfterAnswer) -> OriginateRequest {
    OriginateRequest {
        call_id: Uuid::new_v4(),
        org_id: Uuid::nil(),
        legs,
        caller_id: None,
        record: false,
        emergency: false,
        rule_position: None,
        answer_timeout_secs: 15,
        after_answer: after,
    }
}

#[tokio::test]
async fn events_follow_failover_answer_and_end() {
    let Some(e) = env() else { return };
    let o = FreeswitchOriginator { esl: e.esl.clone() };
    let sink = Arc::new(Collect::default());
    let req = request(
        vec![leg(&e.down, "244923447108"), leg(&e.up, "244923447108")],
        AfterAnswer::TestTone { secs: 2 },
    );
    let t0 = std::time::Instant::now();
    let out = o.originate(&req, sink.clone()).await.unwrap();
    eprintln!("resultado em {:?}: {out:?}", t0.elapsed());
    assert!(out.answered, "{out:?}");
    let ev = sink.wait_end(20).await;
    let down = trunk_id_from_gateway(&e.down);
    let up = trunk_id_from_gateway(&e.up);
    let pos = |f: &dyn Fn(&CallEvent) -> bool| ev.iter().position(|x| f(x));
    let failed =
        pos(&|x| matches!(x, CallEvent::AttemptFailed { trunk_id, .. } if *trunk_id == down))
            .expect("tentativa falhada no tronco em baixo");
    let answered = pos(&|x| matches!(x, CallEvent::Answered { trunk_id, .. } if *trunk_id == up))
        .expect("atendida no segundo tronco");
    let ringing = pos(&|x| matches!(x, CallEvent::Ringing { trunk_id, .. } if *trunk_id == up))
        .expect("a tocar antes de atender");
    assert!(failed < answered && ringing < answered, "{ev:?}");
    match ev.last().unwrap() {
        CallEvent::Ended {
            answered,
            cause,
            billsec,
        } => {
            assert!(*answered);
            assert_eq!(cause, "NORMAL_CLEARING");
            assert!(billsec.is_some());
        }
        other => panic!("o último evento não é Ended: {other:?}"),
    }
    assert_eq!(
        ev.iter()
            .filter(|x| matches!(x, CallEvent::Ended { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn busy_ends_unanswered_with_the_cause() {
    let Some(e) = env() else { return };
    let o = FreeswitchOriginator { esl: e.esl.clone() };
    let sink = Arc::new(Collect::default());
    let req = request(
        vec![leg(&e.up, "244923447000")],
        AfterAnswer::TestTone { secs: 1 },
    );
    let out = o.originate(&req, sink.clone()).await.unwrap();
    assert!(!out.answered);
    assert_eq!(out.hangup_cause.as_deref(), Some("USER_BUSY"), "{out:?}");
    let ev = sink.wait_end(10).await;
    assert!(
        matches!(
            ev.last(),
            Some(CallEvent::Ended {
                answered: false,
                ..
            })
        ),
        "{ev:?}"
    );
    assert!(!ev.iter().any(|x| matches!(x, CallEvent::Answered { .. })));
}

#[tokio::test]
async fn room_bridge_reaches_the_bridge_ua_and_hangup_ends_it() {
    let Some(e) = env() else { return };
    let o = FreeswitchOriginator { esl: e.esl.clone() };
    let sink = Arc::new(Collect::default());
    let room = format!("sala-{}", &Uuid::new_v4().simple().to_string()[..8]);
    let req = request(
        vec![leg(&e.up, "244923447108")],
        AfterAnswer::RoomBridge {
            room_code: room.clone(),
            bridge_host: e.bridge.ip(),
            bridge_port: e.bridge.port(),
            codec: Some("PCMA".into()),
        },
    );
    let out = o.originate(&req, sink.clone()).await.unwrap();
    assert!(out.answered, "{out:?}");
    // A perna para a ponte existe, com o nome da sala no Request-URI.
    let mut seen = String::new();
    for _ in 0..30 {
        let mut c = EslConn::connect(&e.esl).await.unwrap();
        seen = c
            .api(
                &format!("show channels like room-{room}"),
                Duration::from_secs(4),
            )
            .await
            .unwrap();
        if seen.contains(&format!("room-{room}")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    eprintln!("canais da ponte: {seen}");
    assert!(seen.contains(&format!("room-{room}")), "{seen}");
    assert!(!seen.contains("conference"), "não é a conferência local");
    // Desligar pela porta termina a chamada e fecha os eventos.
    o.hangup(req.call_id).await.unwrap();
    let ev = sink.wait_end(10).await;
    assert!(
        matches!(ev.last(), Some(CallEvent::Ended { answered: true, .. })),
        "{ev:?}"
    );
}

#[tokio::test]
async fn sip_control_reads_real_gateways() {
    let Some(e) = env() else { return };
    let sip = FreeswitchSipControl {
        esl: Some(e.esl.clone()),
        kamailio: None,
    };
    let snap = sip
        .snapshot(&[
            e.up.clone(),
            e.down.clone(),
            "dlx-00000000-0000-0000-0000-000000000000".into(),
        ])
        .await
        .unwrap();
    eprintln!("{snap:#?}");
    let media = snap.media.expect("media server");
    assert!(media.version.unwrap().starts_with("1.11"));
    use delonix_meet_domain::telephony::cost::RegistrationState::*;
    assert_eq!(snap.gateways[0].registration, NotRequired);
    assert_eq!(snap.gateways[0].channels_in_use, Some(0));
    assert_eq!(snap.gateways[2].registration, Unknown);
    assert_eq!(
        snap.gateways[2].channels_in_use, None,
        "gateway desconhecido: sem medida"
    );
    assert_eq!(snap.sbc_error.as_deref(), Some("not_configured"));
}
