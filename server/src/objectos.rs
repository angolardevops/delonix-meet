//! Armazenamento de OBJECTOS (MinIO ou S3) — ADR-0020, decisão D2.
//!
//! **Porque existe.** As gravações viviam só em disco local, e o
//! `values-production.yaml` pedia um volume `ReadWriteMany` para três réplicas
//! as poderem servir. Isso amarrava o produto a armazenamento de blocos
//! partilhado — que é caro, frágil e não sai do cluster. Com objectos, qualquer
//! réplica serve qualquer gravação e o armazenamento deixa de ser do cluster.
//!
//! **O que este módulo NÃO faz, hoje:** não está ligado ao gravador. O
//! `recorder` continua a escrever em disco e o `recordings` continua a servir
//! de lá. Ligar as duas pontas sem um MinIO real onde medir seria deixar o
//! produto num estado híbrido que ninguém exercitou — e é assim que se perdem
//! gravações. O que está aqui é o cliente, a configuração e o teste de ligação,
//! que é o que se pode provar sem o serviço de pé.
//!
//! **Compatível com MinIO:** `force_path_style`. O estilo por domínio
//! (`bucket.host`) exige DNS por bucket, que um MinIO dentro do cluster não
//! tem.

use aws_credential_types::Credentials;
use aws_sdk_s3::config::{BehaviorVersion, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;

/// O que é preciso para falar com o armazenamento. Tudo vem da configuração da
/// plataforma ou do ambiente — nada tem valor por omissão que funcione, de
/// propósito: um cliente de objectos que arranca com valores inventados escreve
/// em sítio nenhum e só se dá por isso quando se procura a gravação.
#[derive(Debug, Clone)]
pub struct Definicoes {
    /// `http://minio.minio.svc.cluster.local:9000` dentro do cluster.
    pub endpoint: String,
    pub bucket: String,
    pub regiao: String,
    pub access_key: String,
    pub secret_key: String,
}

impl Definicoes {
    /// Lê do ambiente. `None` se faltar o essencial — e faltar é o caso normal
    /// de quem corre o Meet sem objectos.
    pub fn do_ambiente() -> Option<Self> {
        let v = |k: &str| std::env::var(k).ok().filter(|s| !s.trim().is_empty());
        Some(Self {
            endpoint: v("OBJECT_STORE_ENDPOINT")?,
            bucket: v("OBJECT_STORE_BUCKET").unwrap_or_else(|| "meet-gravacoes".into()),
            regiao: v("OBJECT_STORE_REGION").unwrap_or_else(|| "us-east-1".into()),
            access_key: v("OBJECT_STORE_ACCESS_KEY")?,
            secret_key: v("OBJECT_STORE_SECRET_KEY")?,
        })
    }
}

/// O cliente. Barato de clonar (o SDK partilha a ligação por dentro).
#[derive(Clone)]
pub struct Objectos {
    cliente: Client,
    bucket: String,
}

impl std::fmt::Debug for Objectos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Sem o cliente: ele imprime a configuração, e a configuração tem a
        // chave secreta.
        f.debug_struct("Objectos")
            .field("bucket", &self.bucket)
            .finish()
    }
}

impl Objectos {
    pub fn novo(d: &Definicoes) -> Self {
        let cfg = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(d.regiao.clone()))
            .endpoint_url(&d.endpoint)
            // MinIO não tem DNS por bucket: sem isto, o SDK pede a
            // `bucket.minio.…` e não há quem responda.
            .force_path_style(true)
            .credentials_provider(Credentials::new(
                d.access_key.clone(),
                d.secret_key.clone(),
                None,
                None,
                "delonix",
            ))
            .build();
        Self {
            cliente: Client::from_conf(cfg),
            bucket: d.bucket.clone(),
        }
    }

    /// A chave de uma gravação. Num sítio só: uma chave escrita em dois sítios
    /// é uma gravação que se escreve num e se procura noutro.
    pub fn chave_da_gravacao(id: uuid::Uuid, extensao: &str) -> String {
        format!("gravacoes/{id}.{extensao}")
    }

    /// Carrega um ficheiro do disco. Usa `ByteStream::from_path`, que lê em
    /// pedaços — um `.webm` de uma hora não cabe em memória, e carregá-lo para
    /// `Vec<u8>` mataria o pod a meio do upload.
    pub async fn guardar(
        &self,
        chave: &str,
        caminho: &std::path::Path,
        tipo: &str,
    ) -> Result<(), Erro> {
        let corpo = ByteStream::from_path(caminho)
            .await
            .map_err(Erro::leitura)?;
        self.cliente
            .put_object()
            .bucket(&self.bucket)
            .key(chave)
            .content_type(tipo)
            .body(corpo)
            .send()
            .await
            .map_err(Erro::envio)?;
        Ok(())
    }

    /// Lê um intervalo de bytes. É isto que torna o S3 utilizável para servir
    /// vídeo: o leitor do browser pede intervalos (`Range`) para saltar na
    /// linha do tempo, e sem isso cada salto puxaria o ficheiro inteiro.
    pub async fn ler_intervalo(&self, chave: &str, de: u64, ate: u64) -> Result<Vec<u8>, Erro> {
        let r = self
            .cliente
            .get_object()
            .bucket(&self.bucket)
            .key(chave)
            .range(format!("bytes={de}-{ate}"))
            .send()
            .await
            .map_err(Erro::envio)?;
        let dados = r.body.collect().await.map_err(Erro::leitura)?;
        Ok(dados.into_bytes().to_vec())
    }

    /// O tamanho, sem puxar o conteúdo. É o que a resposta de `Range` precisa
    /// para dizer `Content-Range: bytes x-y/TOTAL`.
    pub async fn tamanho(&self, chave: &str) -> Result<i64, Erro> {
        let r = self
            .cliente
            .head_object()
            .bucket(&self.bucket)
            .key(chave)
            .send()
            .await
            .map_err(Erro::envio)?;
        Ok(r.content_length().unwrap_or(0))
    }

    pub async fn apagar(&self, chave: &str) -> Result<(), Erro> {
        self.cliente
            .delete_object()
            .bucket(&self.bucket)
            .key(chave)
            .send()
            .await
            .map_err(Erro::envio)?;
        Ok(())
    }

    /// Escreve e apaga um objecto de prova. É o que o ecrã do operador chama
    /// para dizer «isto funciona» — e escrever é o que prova, porque ler um
    /// bucket vazio também responde.
    pub async fn testar(&self) -> Result<(), Erro> {
        let chave = format!("teste-de-ligacao/{}", uuid::Uuid::new_v4());
        self.cliente
            .put_object()
            .bucket(&self.bucket)
            .key(&chave)
            .body(ByteStream::from_static(b"delonix"))
            .send()
            .await
            .map_err(Erro::envio)?;
        self.apagar(&chave).await
    }
}

/// O erro, sem o detalhe do SDK a sair para o cliente. A mensagem do SDK pode
/// trazer o endpoint e o nome do bucket, e isso é topologia que não se conta a
/// quem faz um pedido.
#[derive(Debug)]
pub enum Erro {
    Leitura(String),
    Envio(String),
}

impl Erro {
    fn leitura(e: impl std::fmt::Display) -> Self {
        Self::Leitura(e.to_string())
    }

    /// O erro do SDK reduzido ao que se pode mostrar: o **código do serviço**
    /// (`NoSuchBucket`, `InvalidAccessKeyId`, `AccessDenied`…), que é uma
    /// palavra e não topologia.
    ///
    /// Sem isto a mensagem era «service error», que não diz a um operador se o
    /// bucket não existe ou se a chave está errada — e os dois arranjam-se de
    /// maneiras opostas. Quando **não há código**, o erro não chegou a ser uma
    /// resposta do serviço (não ligou, DNS, TLS): aí vale o texto do SDK, que
    /// diz «dispatch failure», e esse é o sinal mais importante dos dois.
    fn envio<E>(e: E) -> Self
    where
        E: aws_sdk_s3::error::ProvideErrorMetadata + std::fmt::Display,
    {
        match e.code() {
            Some(c) => Self::Envio(c.to_string()),
            None => Self::Envio(e.to_string()),
        }
    }
}

impl std::fmt::Display for Erro {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Leitura(e) => write!(f, "não foi possível ler o ficheiro: {e}"),
            Self::Envio(e) => write!(f, "o armazenamento de objectos recusou: {e}"),
        }
    }
}

impl std::error::Error for Erro {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chave_de_uma_gravacao_escreve_se_num_so_sitio() {
        let id = uuid::uuid!("3f2b1c4d-5e6f-7a8b-9c0d-ddddeeeeffff");
        assert_eq!(
            Objectos::chave_da_gravacao(id, "webm"),
            "gravacoes/3f2b1c4d-5e6f-7a8b-9c0d-ddddeeeeffff.webm"
        );
        // A miniatura vive ao lado, com a MESMA raiz: procurar uma gravação e a
        // miniatura dela é procurar no mesmo prefixo.
        assert_eq!(
            Objectos::chave_da_gravacao(id, "jpg"),
            "gravacoes/3f2b1c4d-5e6f-7a8b-9c0d-ddddeeeeffff.jpg"
        );
    }

    #[test]
    fn sem_o_essencial_no_ambiente_nao_ha_definicoes() {
        // Guarda o que estiver e repõe no fim: as variáveis são do processo e
        // outro teste a correr ao lado veria o que este pusesse.
        let antes: Vec<_> = [
            "OBJECT_STORE_ENDPOINT",
            "OBJECT_STORE_ACCESS_KEY",
            "OBJECT_STORE_SECRET_KEY",
            "OBJECT_STORE_BUCKET",
        ]
        .iter()
        .map(|k| (*k, std::env::var(k).ok()))
        .collect();
        for (k, _) in &antes {
            std::env::remove_var(k);
        }

        assert!(
            Definicoes::do_ambiente().is_none(),
            "sem nada, não há cliente"
        );

        std::env::set_var("OBJECT_STORE_ENDPOINT", "http://minio:9000");
        assert!(
            Definicoes::do_ambiente().is_none(),
            "só com endpoint ainda não: faltam as credenciais"
        );

        std::env::set_var("OBJECT_STORE_ACCESS_KEY", "k");
        std::env::set_var("OBJECT_STORE_SECRET_KEY", "s");
        let d = Definicoes::do_ambiente().expect("com o essencial, há cliente");
        assert_eq!(d.endpoint, "http://minio:9000");
        // O bucket tem valor por omissão; o endpoint e as chaves não.
        assert_eq!(d.bucket, "meet-gravacoes");
        assert_eq!(d.regiao, "us-east-1");

        // Um valor só com espaços é o mesmo que não estar: é o que acontece a
        // um `value: ""` de um chart.
        std::env::set_var("OBJECT_STORE_ENDPOINT", "   ");
        assert!(
            Definicoes::do_ambiente().is_none(),
            "espaços não são endereço"
        );

        for (k, v) in antes {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }

    #[test]
    fn o_debug_do_cliente_nao_mostra_a_chave_secreta() {
        let d = Definicoes {
            endpoint: "http://minio:9000".into(),
            bucket: "b".into(),
            regiao: "us-east-1".into(),
            access_key: "ACESSO-VISIVEL".into(),
            secret_key: "SEGREDO-QUE-NAO-PODE-SAIR".into(),
        };
        let texto = format!("{:?}", Objectos::novo(&d));
        assert!(!texto.contains("SEGREDO-QUE-NAO-PODE-SAIR"), "{texto}");
        assert!(!texto.contains("ACESSO-VISIVEL"), "{texto}");
    }
}
