//! A sala de espera vista pelo anfitrião (`GET /api/rooms/{code}/waiting`)
//! contra Postgres real.
//!
//! Guarda uma regressão do porte do convidado sem conta (R155): a sala ganhou
//! a coluna `allow_guests`, e este handler lia a sala com a lista de colunas
//! escrita à mão — sem ela. Resultado: `500` («no column found for name:
//! allow_guests») para o DONO da sala. Só o e2e de isolamento do CI o apanhou.
mod common;

use common::TestApp;

#[sqlx::test(migrations = "./migrations")]
async fn o_dono_espreita_a_sala_de_espera_e_um_estranho_nao(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dona = app.new_org("alfa.test").await;
    let estranho = app.new_org("beta.test").await;
    let sala = app.new_room(&dona, "Sala com espera").await;
    let code = sala["code"].as_str().unwrap();
    let path = format!("/api/rooms/{code}/waiting");

    let (st, body) = app.get(&path, Some(&dona.token)).await;
    assert_eq!(
        st, 200,
        "a dona tem de conseguir ver a sua sala de espera: {body}"
    );
    assert!(body.is_array(), "devolve a lista de quem espera: {body}");

    let (st, body) = app.get(&path, Some(&estranho.token)).await;
    assert!(
        matches!(st, 403 | 404),
        "outra organização não espreita a sala de espera: {st} {body}"
    );

    let (st, _) = app.get(&path, None).await;
    assert_eq!(st, 401, "sem sessão não há sala de espera");
}

/// A classe do defeito, não só o caso: nenhuma query lê uma `Room` com a lista
/// de colunas escrita à mão. Acrescentar uma coluna à sala tem de ser mexer em
/// `rooms::ROOM_COLUMNS` e mais nada.
#[test]
fn nenhuma_query_escreve_as_colunas_da_sala_a_mao() {
    fn rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                rs(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut ficheiros = Vec::new();
    rs(&src, &mut ficheiros);
    // O par de colunas que só a tabela `rooms` tem lado a lado.
    let marca = "waiting_room, e2ee";
    let mut onde = Vec::new();
    for f in ficheiros {
        let texto = std::fs::read_to_string(&f).unwrap();
        for (n, linha) in texto.lines().enumerate() {
            // Só leituras: um INSERT enumera as colunas que escreve, e isso é legítimo.
            if linha.contains("SELECT") && linha.contains(marca) {
                onde.push(format!(
                    "{}:{}",
                    f.strip_prefix(&src).unwrap().display(),
                    n + 1
                ));
            }
        }
    }
    // `application/recording_service.rs` é código por ligar (porta de
    // armazenamento, ADR-0004): fica listado aqui até entrar no grafo de módulos.
    onde.retain(|o| !o.starts_with("application/recording_service.rs"));
    assert!(
        onde.is_empty(),
        "colunas da sala escritas à mão (usa rooms::ROOM_COLUMNS): {onde:?}"
    );
}
