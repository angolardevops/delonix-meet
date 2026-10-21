//! O cliente de objectos contra um MinIO A SÉRIO (ADR-0020, D2).
//!
//! **Porque é um teste à parte e não mais um `#[test]` no módulo.** Os três
//! testes de unidade do `objectos.rs` medem a chave, o ambiente incompleto e o
//! `Debug` sem o segredo — nenhum fala com um armazenamento. O que o
//! `aws-sdk-s3` faz com um endpoint de MinIO, com `force_path_style`, com um
//! `Range` e com um bucket que não existe **não se adivinha do tipo**.
//!
//! **Ele não finge.** Sem `OBJECT_STORE_*` no ambiente, imprime que não mediu e
//! sai — nunca passa por prova. É o `scripts/objectos-prova.sh` que lhe põe um
//! MinIO à frente, e esse script verifica também este aviso.

use delonix_server::objectos::{Definicoes, Objectos};

/// `None` quando não há onde medir, e di-lo em voz alta.
fn definicoes() -> Option<Definicoes> {
    match Definicoes::do_ambiente() {
        Some(d) => Some(d),
        None => {
            println!(
                "· sem OBJECT_STORE_ENDPOINT/ACCESS_KEY/SECRET_KEY: NÃO MEDI nada. \
                 Corre `bash scripts/objectos-prova.sh`."
            );
            None
        }
    }
}

#[tokio::test]
async fn escreve_le_por_intervalos_mede_e_apaga() {
    let Some(d) = definicoes() else { return };
    let o = Objectos::novo(&d);

    // 1. O teste de ligação do painel do operador. Escreve e apaga — é o que
    //    prova, porque ler um bucket vazio também responde.
    o.testar().await.expect("o teste de ligação devia passar");

    // 2. Um ficheiro do disco, pelo caminho (o `ByteStream::from_path` que
    //    existe para um `.webm` de uma hora não caber em memória).
    let dir = std::env::temp_dir().join(format!("objectos-prova-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let ficheiro = dir.join("gravacao.webm");
    // 300 KiB com um padrão previsível: é com ele que se verifica o `Range` —
    // um ficheiro de bytes iguais não distinguia um intervalo errado.
    let conteudo: Vec<u8> = (0..300 * 1024u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&ficheiro, &conteudo).unwrap();

    let id = uuid::Uuid::new_v4();
    let chave = Objectos::chave_da_gravacao(id, "webm");
    o.guardar(&chave, &ficheiro, "video/webm")
        .await
        .expect("guardar");

    // 3. O tamanho, sem puxar o conteúdo: é o que a resposta de `Range` precisa
    //    para dizer `Content-Range: bytes x-y/TOTAL`.
    let tamanho = o.tamanho(&chave).await.expect("tamanho");
    assert_eq!(tamanho, conteudo.len() as i64, "o tamanho não bate");

    // 4. Intervalos. É isto que torna o S3 utilizável para servir vídeo, e é o
    //    que o `recordings.rs` faz hoje com um ficheiro local.
    let meio = o
        .ler_intervalo(&chave, 1000, 1999)
        .await
        .expect("intervalo");
    assert_eq!(meio.len(), 1000, "o intervalo veio com outro tamanho");
    assert_eq!(
        &meio[..],
        &conteudo[1000..2000],
        "o intervalo veio deslocado"
    );

    // Os primeiros bytes — o caso do leitor a abrir o ficheiro.
    let inicio = o.ler_intervalo(&chave, 0, 15).await.expect("início");
    assert_eq!(&inicio[..], &conteudo[0..16]);

    // E o FIM, pedido com folga: um `Range` que passa do fim é o que um leitor
    // manda quando calcula mal o último pedaço. O S3 corta; não é erro.
    let fim = o
        .ler_intervalo(&chave, tamanho as u64 - 10, tamanho as u64 + 5_000)
        .await
        .expect("o fim pedido com folga devia responder, cortado");
    assert_eq!(fim.len(), 10, "o fim devia vir cortado em 10 bytes");
    assert_eq!(&fim[..], &conteudo[conteudo.len() - 10..]);

    // 5. Apagar, e o que vem depois: ler o que já não existe é ERRO, e é por
    //    isso que o servidor pode distinguir «não está lá» de «veio vazio».
    o.apagar(&chave).await.expect("apagar");
    assert!(
        o.tamanho(&chave).await.is_err(),
        "depois de apagar, o tamanho devia falhar — não devolver 0"
    );
    assert!(
        o.ler_intervalo(&chave, 0, 10).await.is_err(),
        "depois de apagar, ler devia falhar"
    );
    // Apagar duas vezes NÃO é erro no S3 (é idempotente): quem limpa uma
    // gravação já limpa não precisa de tratar o caso.
    o.apagar(&chave)
        .await
        .expect("apagar o que já não existe é idempotente no S3");

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn um_bucket_que_nao_existe_falha_e_a_mensagem_nao_traz_o_segredo() {
    let Some(base) = definicoes() else { return };
    let d = Definicoes {
        bucket: format!("nao-existe-{}", uuid::Uuid::new_v4()),
        ..base
    };
    let erro = Objectos::novo(&d)
        .testar()
        .await
        .expect_err("um bucket que não existe não pode passar o teste de ligação");
    let texto = format!("{erro}");

    // ISTO É O ASSERT QUE FALTAVA À PRIMEIRA VERSÃO deste teste, e é a razão de
    // ele existir: sem ele, o teste passava **pela razão errada**. Quando o SDK
    // estava sem cliente HTTP (R308) nenhum pedido chegava a sair, o erro era
    // `dispatch failure`, e um teste que só pedia «falha» dava verde com o
    // cliente completamente partido. Um bucket que não existe tem de ser uma
    // RESPOSTA do serviço, não uma falha de ligação.
    assert!(
        !texto.contains("dispatch failure"),
        "não cheguei a falar com o MinIO — isto não prova nada sobre o bucket: {texto}"
    );
    assert!(
        texto.contains("NoSuchBucket") || texto.contains("NoSuchKey"),
        "a mensagem devia trazer o CÓDIGO do serviço, que é o que diz ao \
         operador o que arranjar: {texto}"
    );

    // O painel do operador mostra esta mensagem. Se o SDK trouxesse a chave
    // secreta no texto, ela ia para o ecrã e para o log.
    assert!(
        !texto.contains(&d.secret_key),
        "a mensagem de erro traz a chave secreta: {texto}"
    );
    assert!(
        !texto.contains(&d.access_key),
        "a mensagem de erro traz a chave de acesso: {texto}"
    );
    println!("· erro de bucket inexistente, como o operador o vê: {texto}");
}
