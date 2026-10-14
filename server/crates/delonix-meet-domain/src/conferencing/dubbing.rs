//! Dobragem ao vivo de uma sala — as regras, puras (ADR-0030).
//!
//! Uma **pista por LÍNGUA**, nunca por ouvinte: dez pessoas a ouvir inglês são
//! UMA síntese. A tradução de texto já é por ouvinte (o browser pede a sua
//! língua a `POST /api/ai/translations`) e funciona porque custa uma frase de
//! LLM; a síntese de voz não escala assim.
//!
//! # As três decisões do dono, escritas aqui porque são regras
//!
//! 1. **No máximo [`MAX_LINGUAS`] línguas ACTIVAS por sala.** É a sala que paga
//!    a conta — é nela que as cadeias ASR→LLM→TTS correm em paralelo. A quarta
//!    é **recusada com um erro que diz porquê**, e nunca posta em fila: entrar
//!    em fila seria prometer uma dobragem que não começa, e quem espera não
//!    sabe disso.
//! 2. **A voz é sintética, de licença permissiva, e anunciada como tal.** Para
//!    português é o `pt_PT-tugão` — e o produto diz que é **de Portugal**
//!    ([`Voz::rotulo`]), nunca «Português» sozinho: num produto angolano,
//!    «Português» sem qualificação lê-se como o daqui, e esta voz não é daqui.
//! 3. **Não há voz pt-AO**, e isso diz-se em vez de se disfarçar com pt-BR. O
//!    catálogo não a lista como se estivesse a caminho; o desenho é por língua,
//!    por isso ela entra no dia em que houver voz angolana.
//!
//! # O que isto NÃO faz
//!
//! **Não clona a voz de ninguém.** Clonar a voz de um participante é tratá-la
//! como biometria: exige consentimento informado, prazo de retenção e uma
//! decisão de conformidade que não é desta frente.
//!
//! E **não é interpretação simultânea**: o caminho soma segundos, e o contrato
//! escrito é «dobragem com atraso», com o atraso medido e mostrado.

use delonix_meet_core::DomainError;

// ---------- Limites ----------

/// Tecto de línguas ACTIVAS numa sala (ADR-0030, decidido a 2026-10-14).
///
/// Activas, não «já pedidas alguma vez»: tirar uma língua liberta a vaga de
/// imediato. A língua de ORIGEM não conta — já está na sala e não é
/// sintetizada.
pub const MAX_LINGUAS: usize = 3;

// ---------- O catálogo ----------

/// Uma voz que a casa oferece. Cada uma é um artefacto de terceiros: fixada
/// por versão e por SHA-256 na imagem do `tts-server`, com o `MODEL_CARD`
/// registado ao lado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Voz {
    /// A etiqueta BCP-47 da língua que esta voz fala (`pt-PT`, não `pt`).
    pub lingua: &'static str,
    /// O nome da voz no catálogo do motor.
    pub voz: &'static str,
    /// A licença do conjunto de dados, como o `MODEL_CARD` a declara.
    pub licenca: &'static str,
}

impl Voz {
    /// O que a pessoa lê no selector — **sempre com o país**.
    ///
    /// «Português (Portugal)», nunca «Português». É a decisão 2 do cabeçalho,
    /// e está aqui em vez de na web porque é uma regra do produto e não uma
    /// escolha de ecrã: um ecrã novo não a pode perder.
    pub fn rotulo(&self) -> &'static str {
        match self.lingua {
            "pt-PT" => "Português (Portugal)",
            "en-US" => "English (US)",
            "fr-FR" => "Français (France)",
            _ => self.lingua,
        }
    }
}

/// As vozes que existem. **Não há `pt-AO`** — ver a decisão 3.
pub const CATALOGO: &[Voz] = &[Voz {
    lingua: "pt-PT",
    voz: "pt_PT-tugão-medium",
    licenca: "CC0",
}];

/// A voz de uma língua, ou nada se não a oferecemos.
pub fn voz_de(lingua: &str) -> Option<&'static Voz> {
    CATALOGO.iter().find(|v| v.lingua == lingua)
}

// ---------- As regras ----------

/// Pode-se acrescentar `nova` às `activas` de uma sala cuja origem é `origem`?
///
/// A ordem das recusas é a ordem pela qual elas interessam a quem pede: o que
/// não existe antes do que não cabe, porque dizer «não cabem mais» a quem pediu
/// uma língua que nunca vamos ter manda-o tirar outra por nada.
pub fn pode_acrescentar(activas: &[String], origem: &str, nova: &str) -> Result<(), DomainError> {
    if nova == origem {
        return Err(DomainError::invalid(
            "dubbing.same_as_source",
            "a língua de origem já se ouve na sala — não se dobra para si mesma",
        ));
    }
    if voz_de(nova).is_none() {
        return Err(DomainError::invalid(
            "dubbing.no_voice",
            format!("não há voz para «{nova}»"),
        ));
    }
    if activas.iter().any(|l| l == nova) {
        return Err(DomainError::invalid(
            "dubbing.already_on",
            "essa língua já está a ser dobrada nesta sala",
        ));
    }
    if activas.len() >= MAX_LINGUAS {
        return Err(DomainError::precondition(
            "dubbing.too_many_languages",
            format!(
                "uma sala dobra no máximo {MAX_LINGUAS} línguas ao mesmo tempo; \
                 tira uma para pôr outra"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn o_tecto_e_de_tres_e_a_quarta_e_recusada_com_o_porque() {
        // Três cabem (o catálogo só tem pt-PT, por isso o tecto prova-se com a
        // lista de activas, que é o que a sala guarda).
        let tres = v(&["en-US", "fr-FR", "es-ES"]);
        assert_eq!(tres.len(), MAX_LINGUAS);
        let e = pode_acrescentar(&tres, "pt-PT", "pt-PT").unwrap_err();
        // A origem bate primeiro — é a recusa mais barata e a mais clara.
        assert_eq!(e.code, "dubbing.same_as_source");

        // Com uma origem diferente, o tecto é que fala.
        let e = pode_acrescentar(&tres, "en-GB", "pt-PT").unwrap_err();
        assert_eq!(e.code, "dubbing.too_many_languages");
        assert!(e.message.contains('3'), "o erro tem de dizer o tecto: {}", e.message);
    }

    #[test]
    fn tirar_uma_lingua_liberta_a_vaga() {
        let cheia = v(&["en-US", "fr-FR", "es-ES"]);
        assert!(pode_acrescentar(&cheia, "en-GB", "pt-PT").is_err());
        // O tecto é de línguas ACTIVAS, não de línguas já pedidas na sessão.
        let depois = v(&["en-US", "fr-FR"]);
        assert!(pode_acrescentar(&depois, "en-GB", "pt-PT").is_ok());
    }

    #[test]
    fn a_lingua_de_origem_nao_conta_nem_se_dobra_para_si_mesma() {
        let e = pode_acrescentar(&[], "pt-PT", "pt-PT").unwrap_err();
        assert_eq!(e.code, "dubbing.same_as_source");
    }

    #[test]
    fn uma_lingua_sem_voz_e_recusada_ANTES_do_tecto() {
        // Quem pede uma língua que nunca vamos ter não pode ser mandado tirar
        // outra: a recusa tem de ser «não há voz», não «não cabem mais».
        let cheia = v(&["en-US", "fr-FR", "es-ES"]);
        let e = pode_acrescentar(&cheia, "en-GB", "ja-JP").unwrap_err();
        assert_eq!(e.code, "dubbing.no_voice");
    }

    #[test]
    fn nao_ha_voz_pt_ao_e_isso_e_a_decisao_escrita() {
        // Se um dia houver, este teste muda COM a voz — e não antes.
        assert!(voz_de("pt-AO").is_none(), "pt-AO não se oferece (ADR-0030)");
        assert!(voz_de("pt-BR").is_none(), "e não se disfarça com pt-BR");
        assert!(voz_de("pt-PT").is_some());
    }

    #[test]
    fn o_rotulo_diz_sempre_o_pais() {
        let pt = voz_de("pt-PT").unwrap();
        assert_eq!(pt.rotulo(), "Português (Portugal)");
        // Num produto angolano, «Português» sozinho lê-se como o daqui.
        assert_ne!(pt.rotulo(), "Português");
        assert_eq!(pt.voz, "pt_PT-tugão-medium");
        assert_eq!(pt.licenca, "CC0");
    }

    #[test]
    fn repetir_uma_lingua_activa_e_recusado() {
        let a = v(&["pt-PT"]);
        let e = pode_acrescentar(&a, "en-GB", "pt-PT").unwrap_err();
        assert_eq!(e.code, "dubbing.already_on");
    }
}
