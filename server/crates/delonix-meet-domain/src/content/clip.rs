//! Recortes de uma gravação, e o *short* vertical — as regras, puras.
//!
//! Um **recorte** é um pedaço de uma gravação com princípio e fim. Um
//! **short** é um recorte com enquadramento vertical (9:16) e, se se pedir,
//! as legendas queimadas. São a mesma tabela e a mesma fila: a diferença está
//! no que o render faz, não no que o pedido é.
//!
//! # Três decisões que se escrevem aqui porque são regras, não configuração
//!
//! 1. **Recodifica-se sempre.** Cortar «em cópia» (`-c copy`) só é exacto num
//!    keyframe, e a R299 já mostrou o que sai de uma gravação arrancada a meio:
//!    o leitor mostra preto até ao próximo keyframe. Um recorte que começa
//!    quando se pediu custa CPU; um recorte que começa «por volta de» não serve
//!    para nada.
//! 2. **O short é 9:16 por CORTE, não por esticamento**, e nunca se amplia
//!    acima da resolução da origem. A gravação é uma grelha já composta: não há
//!    como «seguir o orador» sem compor outra vez a partir das pistas, e
//!    inventar um zoom seria entregar uma coisa pior a dizer que é melhor.
//! 3. **A saída é WebM (VP9 + Opus)**, porque o ffmpeg da imagem do servidor é
//!    LGPL e **não tem libx264** (`scripts/check-ffmpeg-licenca.sh`, decisão
//!    D3). O YouTube aceita WebM; uma plataforma que só aceite H.264 exige uma
//!    conversão fora desta imagem, e isso está dito no plano em vez de sair um
//!    ficheiro que ninguém consegue publicar.
//!
//! # E uma que se mediu e mudou o desenho
//!
//! **As legendas não se queimam na imagem — saem ao lado, num `.vtt`.** O
//! `Dockerfile.server` compila o ffmpeg com `--disable-autodetect` e só
//! `libvpx`, `libopus` e `zlib`: **não há libass nem libfreetype**, e sem elas
//! os filtros `subtitles` e `drawtext` não existem no binário. Queimar exigiria
//! mudar a imagem (e decidir a licença da FreeType), que é decisão do dono e
//! não de quem escreve a tool. O que sai é um WebVTT com as deixas da janela
//! recortada, deslocadas para começar em zero — util de imediato, e honesto
//! quanto ao que é ([`cues_in_window`]).

use delonix_meet_core::{jobs::Retry, DomainError};
use std::time::Duration;

// ---------- Limites ----------

/// Um recorte não passa de quinze minutos. Acima disso não é um recorte: é
/// outra gravação, e já há uma biblioteca para gravações.
pub const MAX_CLIP_MS: i64 = 15 * 60 * 1000;

/// Um *short* não passa de três minutos — é o tecto das plataformas que o
/// nome descreve, e passar dele é produzir um ficheiro que elas recusam.
pub const MAX_SHORT_MS: i64 = 3 * 60 * 1000;

/// Menos de um segundo não é um recorte.
pub const MIN_MS: i64 = 1_000;

/// Lado maior do *short*. Não se amplia acima da origem (ver `vertical_output`).
pub const SHORT_MAX_HEIGHT: u32 = 1920;

/// Prazo da posse de um render. Tem de ser maior do que o tecto do ffmpeg,
/// senão outro nó reivindica um recorte que ainda está a ser composto.
pub const LEASE: Duration = Duration::from_secs(1_800);

/// Duas tentativas, cinco minutos de intervalo. A causa provável de falha é
/// CPU ocupado ou um input que o ffmpeg não gostou — a primeira passa, a
/// segunda não, e queimar dez tentativas num input mau não ajuda ninguém.
pub const RETRY: Retry = Retry {
    max_attempts: 2,
    delays: &[300],
    jitter: 0.2,
};

// ---------- O que se pede ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Clip,
    Short,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Clip => "clip",
            Kind::Short => "short",
        }
    }

    pub fn parse(raw: &str) -> Option<Kind> {
        match raw {
            "clip" => Some(Kind::Clip),
            "short" => Some(Kind::Short),
            _ => None,
        }
    }

    pub fn max_duration_ms(self) -> i64 {
        match self {
            Kind::Clip => MAX_CLIP_MS,
            Kind::Short => MAX_SHORT_MS,
        }
    }
}

/// Que parte da largura fica no *short*.
///
/// A gravação é uma grelha: quem fala pode estar à esquerda. Sem seguimento de
/// rosto, a escolha é de quem recorta — e é por isso um parâmetro, não um
/// palpite nosso.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Center,
    Left,
    Right,
}

impl Focus {
    pub fn as_str(self) -> &'static str {
        match self {
            Focus::Center => "center",
            Focus::Left => "left",
            Focus::Right => "right",
        }
    }

    pub fn parse(raw: &str) -> Option<Focus> {
        match raw {
            "center" => Some(Focus::Center),
            "left" => Some(Focus::Left),
            "right" => Some(Focus::Right),
            _ => None,
        }
    }
}

/// Um pedido já validado.
#[derive(Debug, Clone)]
pub struct Request {
    pub kind: Kind,
    pub label: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub focus: Focus,
    /// Produzir um WebVTT ao lado do recorte, nesta língua. **Não queima** nada
    /// na imagem (ver o cabeçalho do módulo): escreve um ficheiro de legendas
    /// com as deixas da janela, já deslocadas para começar em zero.
    pub captions_lang: Option<String>,
}

impl Request {
    pub fn duration_ms(&self) -> i64 {
        self.end_ms - self.start_ms
    }
}

fn invalido(code: &'static str, msg: impl Into<String>, campo: &str, espera: &str) -> DomainError {
    DomainError::invalid(code, msg).with_field(campo, espera.to_string())
}

/// Valida um pedido de recorte.
///
/// `recording_duration_ms` é `None` numa gravação cuja duração ainda não se
/// mediu: aí só se valida a forma do intervalo, e não se recusa um fim que
/// pode estar certo. O ffmpeg corta pelo que existe.
pub fn validate(
    kind: &str,
    label: &str,
    start_ms: i64,
    end_ms: i64,
    focus: Option<&str>,
    captions_lang: Option<&str>,
    recording_duration_ms: Option<i64>,
) -> Result<Request, DomainError> {
    let kind = Kind::parse(kind.trim()).ok_or_else(|| {
        invalido(
            "clip.unknown_kind",
            format!("tipo desconhecido: {kind}"),
            "kind",
            "clip | short",
        )
    })?;

    let label = label.trim();
    if label.is_empty() || label.chars().count() > 120 {
        return Err(invalido(
            "clip.invalid_label",
            "o recorte tem de ter um nome",
            "label",
            "1 a 120 caracteres",
        ));
    }
    if label.chars().any(|c| c.is_control()) {
        return Err(invalido(
            "clip.invalid_label",
            "o nome não pode ter caracteres de controlo",
            "label",
            "sem caracteres de controlo",
        ));
    }

    if start_ms < 0 {
        return Err(invalido(
            "clip.invalid_range",
            "o início não pode ser negativo",
            "start_ms",
            "≥ 0",
        ));
    }
    let duracao = end_ms - start_ms;
    if duracao < MIN_MS {
        return Err(invalido(
            "clip.invalid_range",
            format!("um recorte tem de ter pelo menos {} ms", MIN_MS),
            "end_ms",
            "end_ms − start_ms ≥ 1000",
        ));
    }
    if duracao > kind.max_duration_ms() {
        return Err(invalido(
            "clip.too_long",
            format!(
                "um {} não passa de {} ms (pediu {duracao})",
                kind.as_str(),
                kind.max_duration_ms()
            ),
            "end_ms",
            &format!("no máximo {} ms de duração", kind.max_duration_ms()),
        ));
    }
    if let Some(total) = recording_duration_ms {
        if total > 0 && start_ms >= total {
            return Err(invalido(
                "clip.invalid_range",
                format!("a gravação tem {total} ms e o início pedido é {start_ms}"),
                "start_ms",
                "dentro da gravação",
            ));
        }
    }

    let focus = match focus {
        None => Focus::default(),
        Some(f) => Focus::parse(f.trim()).ok_or_else(|| {
            invalido(
                "clip.unknown_focus",
                format!("enquadramento desconhecido: {f}"),
                "focus",
                "center | left | right",
            )
        })?,
    };

    let captions = match captions_lang {
        None => None,
        Some(lang) => {
            let lang = lang.trim();
            // Mesma forma que as legendas já exigem: um código curto, nada que
            // se possa meter num comando como argumento.
            let ok = (2..=12).contains(&lang.len())
                && lang.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
            if !ok {
                return Err(invalido(
                    "clip.invalid_language",
                    format!("língua inválida: {lang}"),
                    "captions_lang",
                    "um código BCP-47 curto (pt, en, pt-PT)",
                ));
            }
            Some(lang.to_ascii_lowercase())
        }
    };

    Ok(Request {
        kind,
        label: label.to_string(),
        start_ms,
        end_ms,
        focus,
        captions_lang: captions,
    })
}

// ---------- As legendas do recorte ----------

/// Uma deixa de legenda: início, fim (ms) e texto.
pub type Cue = (i64, i64, String);

/// As deixas que caem na janela `[start_ms, end_ms)`, cortadas à janela e
/// **deslocadas** para que a primeira comece em zero.
///
/// Uma deixa que atravessa o início fica, com o princípio cortado: quem vê o
/// recorte está a ouvir aquela frase, e deixá-la fora é perder a primeira
/// linha. Uma deixa vazia depois do corte sai.
pub fn cues_in_window(cues: &[Cue], start_ms: i64, end_ms: i64) -> Vec<Cue> {
    cues.iter()
        .filter(|(a, b, _)| *b > start_ms && *a < end_ms)
        .filter_map(|(a, b, texto)| {
            let ini = (*a).max(start_ms) - start_ms;
            let fim = (*b).min(end_ms) - start_ms;
            if fim <= ini || texto.trim().is_empty() {
                return None;
            }
            Some((ini, fim, texto.clone()))
        })
        .collect()
}

// ---------- Enquadramento vertical ----------

/// Um rectângulo de corte, em pixels da origem. Lados pares: o VP9 recusa
/// dimensões ímpares em croma subamostrado, e um render que falha por um pixel
/// é das avarias mais difíceis de ler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

fn par(v: u32) -> u32 {
    v & !1
}

/// O corte 9:16 de uma imagem `w`×`h`, com o foco pedido.
///
/// Corta — nunca estica. Numa origem mais larga do que 9:16 (o caso normal,
/// 16:9) fica a altura inteira e corta-se a largura; numa origem mais alta do
/// que 9:16 fica a largura inteira e corta-se a altura pelo meio (aí o foco
/// não se aplica: é vertical em cima e em baixo, e não há «esquerda»).
pub fn vertical_crop(w: u32, h: u32, focus: Focus) -> Crop {
    if w == 0 || h == 0 {
        return Crop {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        };
    }
    // 9:16 ⇒ w/h = 9/16. Compara-se com multiplicação para não arredondar.
    if (w as u64) * 16 > (h as u64) * 9 {
        let cw = par(((h as u64) * 9 / 16) as u32).max(2).min(par(w));
        let sobra = w.saturating_sub(cw);
        let x = match focus {
            Focus::Left => 0,
            Focus::Right => sobra,
            Focus::Center => sobra / 2,
        };
        Crop {
            x: par(x),
            y: 0,
            w: cw,
            h: par(h),
        }
    } else {
        let ch = par(((w as u64) * 16 / 9) as u32).max(2).min(par(h));
        let sobra = h.saturating_sub(ch);
        Crop {
            x: 0,
            y: par(sobra / 2),
            w: par(w),
            h: ch,
        }
    }
}

/// A dimensão de saída do *short*: o corte reduzido a `SHORT_MAX_HEIGHT` de
/// altura quando é maior, e **tal como está** quando é menor.
///
/// Não amplia. Esticar 405×720 para 1080×1920 não acrescenta imagem nenhuma —
/// só peso e a aparência de qualidade que não existe.
pub fn vertical_output(crop: Crop) -> (u32, u32) {
    if crop.h <= SHORT_MAX_HEIGHT || crop.h == 0 {
        return (crop.w, crop.h);
    }
    let h = SHORT_MAX_HEIGHT;
    let w = par(((crop.w as u64) * (h as u64) / (crop.h as u64)) as u32).max(2);
    (w, h)
}

// ---------- Estado ----------

/// O estado de um render, tal como está na base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Queued,
    Processing,
    Ready,
    Failed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::Processing => "processing",
            Status::Ready => "ready",
            Status::Failed => "failed",
        }
    }

    pub fn parse(raw: &str) -> Option<Status> {
        match raw {
            "queued" => Some(Status::Queued),
            "processing" => Some(Status::Processing),
            "ready" => Some(Status::Ready),
            "failed" => Some(Status::Failed),
            _ => None,
        }
    }
}

/// Só um recorte `ready` tem ficheiro para descarregar. Serve à rota do
/// conteúdo, que de outra forma devolvia um 404 sem explicação ou um ficheiro
/// a meio.
pub fn ensure_downloadable(status: Status) -> Result<(), DomainError> {
    match status {
        Status::Ready => Ok(()),
        Status::Failed => Err(DomainError::new(
            delonix_meet_core::ErrorKind::FailedPrecondition,
            "clip.failed",
            "este recorte falhou a compor — vê o erro no recorte e pede outro",
        )),
        _ => Err(DomainError::new(
            delonix_meet_core::ErrorKind::FailedPrecondition,
            "clip.not_ready",
            "o recorte ainda está a compor",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pedido(kind: &str, start: i64, end: i64) -> Result<Request, DomainError> {
        validate(kind, "Um recorte", start, end, None, None, Some(600_000))
    }

    #[test]
    fn o_intervalo_tem_de_fazer_sentido() {
        assert!(pedido("clip", 0, 10_000).is_ok());
        assert_eq!(
            pedido("clip", 0, 500).unwrap_err().code,
            "clip.invalid_range"
        );
        assert_eq!(
            pedido("clip", 10_000, 10_000).unwrap_err().code,
            "clip.invalid_range"
        );
        assert_eq!(
            pedido("clip", 10_000, 5_000).unwrap_err().code,
            "clip.invalid_range"
        );
        assert_eq!(
            pedido("clip", -1, 10_000).unwrap_err().code,
            "clip.invalid_range"
        );
        // Começar depois do fim da gravação não dá recorte nenhum.
        assert_eq!(
            pedido("clip", 700_000, 710_000).unwrap_err().code,
            "clip.invalid_range"
        );
        // Sem duração conhecida, a forma basta — o ffmpeg corta pelo que existe.
        assert!(validate("clip", "x", 700_000, 710_000, None, None, None).is_ok());
    }

    #[test]
    fn cada_tipo_tem_o_seu_tecto() {
        assert!(pedido("clip", 0, MAX_CLIP_MS).is_ok());
        assert_eq!(
            pedido("clip", 0, MAX_CLIP_MS + 1).unwrap_err().code,
            "clip.too_long"
        );
        assert!(validate("short", "x", 0, MAX_SHORT_MS, None, None, None).is_ok());
        assert_eq!(
            validate("short", "x", 0, MAX_SHORT_MS + 1, None, None, None)
                .unwrap_err()
                .code,
            "clip.too_long"
        );
        assert_eq!(
            pedido("filme", 0, 10_000).unwrap_err().code,
            "clip.unknown_kind"
        );
    }

    #[test]
    fn as_legendas_pedem_se_por_lingua_nos_dois_tipos() {
        // Vale no recorte E no short: o que sai é um `.vtt` ao lado, não tinta
        // na imagem (o ffmpeg desta imagem não tem libass).
        let r = validate("clip", "x", 0, 10_000, None, Some("PT"), None).unwrap();
        assert_eq!(
            r.captions_lang.as_deref(),
            Some("pt"),
            "a língua normaliza-se"
        );
        let r = validate("short", "x", 0, 10_000, Some("left"), Some("en"), None).unwrap();
        assert_eq!(r.focus, Focus::Left);
        assert_eq!(r.captions_lang.as_deref(), Some("en"));
        // Nada que se possa meter num comando como argumento.
        assert_eq!(
            validate("short", "x", 0, 10_000, None, Some("pt; rm -rf /"), None)
                .unwrap_err()
                .code,
            "clip.invalid_language"
        );
        assert_eq!(
            validate("short", "x", 0, 10_000, Some("cima"), None, None)
                .unwrap_err()
                .code,
            "clip.unknown_focus"
        );
    }

    #[test]
    fn as_deixas_cortam_se_a_janela_e_comecam_em_zero() {
        let cues: Vec<Cue> = vec![
            (0, 5_000, "antes".into()),
            (9_000, 12_000, "atravessa o início".into()),
            (13_000, 15_000, "dentro".into()),
            (19_000, 25_000, "atravessa o fim".into()),
            (30_000, 31_000, "depois".into()),
            (14_000, 14_500, "   ".into()),
        ];
        let out = cues_in_window(&cues, 10_000, 20_000);
        assert_eq!(
            out,
            vec![
                // Cortada no início, e o tempo recomeça em zero.
                (0, 2_000, "atravessa o início".to_string()),
                (3_000, 5_000, "dentro".to_string()),
                // Cortada no fim.
                (9_000, 10_000, "atravessa o fim".to_string()),
            ],
            "deixa vazia sai, e o que está fora da janela não entra"
        );
        assert!(cues_in_window(&cues, 26_000, 29_000).is_empty());
        assert!(cues_in_window(&[], 0, 1_000).is_empty());
    }

    #[test]
    fn o_nome_e_obrigatorio_e_sem_controlo() {
        assert_eq!(pedido_com_nome("").unwrap_err().code, "clip.invalid_label");
        assert_eq!(
            pedido_com_nome("   ").unwrap_err().code,
            "clip.invalid_label"
        );
        assert_eq!(
            pedido_com_nome("a\nb").unwrap_err().code,
            "clip.invalid_label"
        );
        assert_eq!(
            pedido_com_nome(&"x".repeat(121)).unwrap_err().code,
            "clip.invalid_label"
        );
        assert_eq!(
            pedido_com_nome("  Troço do Kilamba  ").unwrap().label,
            "Troço do Kilamba"
        );
    }

    fn pedido_com_nome(label: &str) -> Result<Request, DomainError> {
        validate("clip", label, 0, 10_000, None, None, None)
    }

    #[test]
    fn o_corte_vertical_corta_e_nunca_estica() {
        // 16:9 → fica a altura inteira, corta-se a largura.
        let c = vertical_crop(1280, 720, Focus::Center);
        assert_eq!((c.w, c.h), (404, 720));
        assert_eq!(c.y, 0);
        // Centrado: a mesma sobra dos dois lados.
        assert_eq!(c.x, (1280 - 404) / 2);
        assert_eq!(vertical_crop(1280, 720, Focus::Left).x, 0);
        assert_eq!(vertical_crop(1280, 720, Focus::Right).x, 1280 - 404);
        // A razão de aspecto do corte é 9:16 a menos de um pixel par.
        assert!(((c.w as f64) / (c.h as f64) - 9.0 / 16.0).abs() < 0.01);
    }

    #[test]
    fn os_lados_do_corte_sao_sempre_pares() {
        for (w, h) in [(1281, 721), (1920, 1080), (641, 361), (100, 100)] {
            for f in [Focus::Center, Focus::Left, Focus::Right] {
                let c = vertical_crop(w, h, f);
                assert_eq!(c.w % 2, 0, "{w}x{h} {f:?}: largura ímpar");
                assert_eq!(c.h % 2, 0, "{w}x{h} {f:?}: altura ímpar");
                assert_eq!(c.x % 2, 0, "{w}x{h} {f:?}: x ímpar");
                assert!(c.x + c.w <= w, "{w}x{h} {f:?}: o corte sai da imagem");
                assert!(c.y + c.h <= h, "{w}x{h} {f:?}: o corte sai da imagem");
            }
        }
    }

    #[test]
    fn uma_origem_mais_alta_do_que_9_16_corta_em_altura() {
        // 1080×2400 é mais alto do que 9:16.
        let c = vertical_crop(1080, 2400, Focus::Left);
        assert_eq!(c.w, 1080, "a largura fica inteira");
        assert_eq!(c.h, 1920);
        assert_eq!(c.y, (2400 - 1920) / 2, "centra-se na vertical");
        assert_eq!(c.x, 0, "o foco não se aplica: não há sobra horizontal");
    }

    #[test]
    fn a_saida_reduz_mas_nao_amplia() {
        // Um corte pequeno sai como está.
        assert_eq!(
            vertical_output(Crop {
                x: 0,
                y: 0,
                w: 404,
                h: 720
            }),
            (404, 720)
        );
        // Um corte grande reduz-se a 1920 de altura, mantendo a proporção.
        let (w, h) = vertical_output(Crop {
            x: 0,
            y: 0,
            w: 1440,
            h: 2560,
        });
        assert_eq!(h, 1920);
        assert_eq!(w, 1080);
        assert_eq!(
            vertical_output(Crop {
                x: 0,
                y: 0,
                w: 0,
                h: 0
            }),
            (0, 0)
        );
    }

    #[test]
    fn so_um_recorte_pronto_se_descarrega() {
        assert!(ensure_downloadable(Status::Ready).is_ok());
        assert_eq!(
            ensure_downloadable(Status::Queued).unwrap_err().code,
            "clip.not_ready"
        );
        assert_eq!(
            ensure_downloadable(Status::Processing).unwrap_err().code,
            "clip.not_ready"
        );
        assert_eq!(
            ensure_downloadable(Status::Failed).unwrap_err().code,
            "clip.failed"
        );
        for s in [
            Status::Queued,
            Status::Processing,
            Status::Ready,
            Status::Failed,
        ] {
            assert_eq!(Status::parse(s.as_str()), Some(s));
        }
        assert!(Status::parse("inventado").is_none());
    }

    #[test]
    fn a_posse_dura_mais_do_que_um_render() {
        // Se a posse expirasse antes do tecto do ffmpeg, outro nó reivindicava
        // um recorte que ainda está a ser composto — e dois ffmpeg escreviam o
        // mesmo ficheiro.
        assert!(
            LEASE.as_secs() > 900,
            "a posse tem de cobrir um render longo"
        );
        assert_eq!(RETRY.max_attempts, 2);
    }
}
