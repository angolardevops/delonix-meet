//! O pouco de Matroska que a emissão precisa de saber ler, sem desmultiplexar.
//!
//! - Onde se pode (re)entrar no fluxo: um Cluster (ADR-0013 §3 — um destino
//!   reposto retoma no presente, no próximo Cluster, com o cabeçalho guardado).
//! - A resolução, lida do cabeçalho (`Tracks/TrackEntry/Video`), para a
//!   gravação dizer «2160p» com o que o browser mandou e não com o que se supõe.
//! - O Timestamp de cada Cluster, para a duração gravada.
//!
//! A detecção de Cluster com CRC-32 vem da linha `frontend/l2-*` (`ea6fc31`),
//! onde foi medida contra um RTMP real: o muxer do ffmpeg põe um CRC-32 antes
//! do Timestamp, o MediaRecorder do Chromium não.

const CLUSTER_ID: [u8; 4] = [0x1F, 0x43, 0xB6, 0x75];
const TIMESTAMP_ID: u8 = 0xE7;
/// Elemento CRC-32 (id 0xBF, tamanho 0x84 = 4 bytes).
const CRC32_HEAD: [u8; 2] = [0xBF, 0x84];
/// Bytes que um início de Cluster ocupa até ao Timestamp: 4 (id) + até 8
/// (tamanho) + 6 (CRC-32 opcional) + 1 (id do Timestamp).
pub const CLUSTER_PROBE: usize = 19;
/// Tecto do cabeçalho guardado. O do MediaRecorder tem centenas de bytes.
pub const HEADER_MAX: usize = 1024 * 1024;

const SEGMENT_ID: u32 = 0x1853_8067;
const TRACKS_ID: u32 = 0x1654_AE6B;
const TRACK_ENTRY_ID: u32 = 0xAE;
const VIDEO_ID: u32 = 0xE0;
const PIXEL_WIDTH_ID: u32 = 0xB0;
const PIXEL_HEIGHT_ID: u32 = 0xBA;

/// Posição do primeiro início de Cluster em `buf`.
///
/// Procurar só os 4 bytes do id daria falsos positivos dentro dos dados de
/// vídeo. Exige-se também um tamanho EBML bem formado e, logo a seguir, o
/// elemento Timestamp — ou um CRC-32 e depois o Timestamp.
pub fn cluster_start(buf: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + CLUSTER_ID.len() <= buf.len() {
        let p = i + buf[i..].windows(4).position(|w| w == CLUSTER_ID)?;
        if timestamp_offset(&buf[p..]).is_some() {
            return Some(p);
        }
        i = p + 1;
    }
    None
}

/// Com `buf` a começar num Cluster: a posição do id do Timestamp.
fn timestamp_offset(buf: &[u8]) -> Option<usize> {
    let &b = buf.get(4)?;
    if b == 0 {
        return None;
    }
    let first_child = 4 + b.leading_zeros() as usize + 1;
    if buf.get(first_child) == Some(&TIMESTAMP_ID) {
        return Some(first_child);
    }
    if buf.get(first_child..first_child + 2) == Some(&CRC32_HEAD[..])
        && buf.get(first_child + 6) == Some(&TIMESTAMP_ID)
    {
        return Some(first_child + 6);
    }
    None
}

/// O Timestamp (na escala do segmento; milissegundos por omissão) do Cluster
/// que começa em `buf`. `None` se o elemento ainda não chegou todo.
pub fn cluster_timestamp(buf: &[u8]) -> Option<u64> {
    let ts = timestamp_offset(buf)?;
    let (size, n) = read_size(buf.get(ts + 1..)?)?;
    let size = size? as usize;
    if size == 0 || size > 8 {
        return None;
    }
    let start = ts + 1 + n;
    let bytes = buf.get(start..start + size)?;
    Some(bytes.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b)))
}

/// Largura e altura do primeiro vídeo declarado no cabeçalho.
pub fn video_dimensions(header: &[u8]) -> Option<(u32, u32)> {
    let mut width = None;
    let mut height = None;
    walk(header, 0, &mut |id, payload| match id {
        PIXEL_WIDTH_ID if width.is_none() => width = read_uint(payload),
        PIXEL_HEIGHT_ID if height.is_none() => height = read_uint(payload),
        _ => {}
    });
    Some((width? as u32, height? as u32))
}

/// Percorre os elementos, descendo só nos contentores que levam ao vídeo.
fn walk(buf: &[u8], depth: usize, visit: &mut dyn FnMut(u32, &[u8])) {
    if depth > 6 {
        return;
    }
    let mut i = 0;
    while i < buf.len() {
        let Some((id, id_len)) = read_id(&buf[i..]) else {
            return;
        };
        let Some((size, size_len)) = read_size(&buf[i + id_len..]) else {
            return;
        };
        let start = i + id_len + size_len;
        // Tamanho desconhecido (o Segment de um fluxo ao vivo): vai até ao fim.
        let end = match size {
            Some(s) => start.saturating_add(s as usize).min(buf.len()),
            None => buf.len(),
        };
        if start > buf.len() {
            return;
        }
        let payload = &buf[start..end];
        match id {
            SEGMENT_ID | TRACKS_ID | TRACK_ENTRY_ID | VIDEO_ID => walk(payload, depth + 1, visit),
            _ => visit(id, payload),
        }
        if size.is_none() {
            return;
        }
        i = end;
    }
}

/// Id EBML (com o marcador de comprimento incluído, como a especificação o escreve).
fn read_id(buf: &[u8]) -> Option<(u32, usize)> {
    let &first = buf.first()?;
    let len = first.leading_zeros() as usize + 1;
    if len > 4 {
        return None;
    }
    let bytes = buf.get(..len)?;
    Some((bytes.iter().fold(0u32, |a, &b| (a << 8) | u32::from(b)), len))
}

/// Tamanho EBML. `Some(None)` = tamanho desconhecido (todos os bits a 1).
fn read_size(buf: &[u8]) -> Option<(Option<u64>, usize)> {
    let &first = buf.first()?;
    if first == 0 {
        return None;
    }
    let len = first.leading_zeros() as usize + 1;
    let bytes = buf.get(..len)?;
    let mask = if len >= 8 { 0 } else { 0xFFu8 >> len };
    let mut value = u64::from(first & mask);
    let mut all_ones = first & mask == mask;
    for &b in &bytes[1..] {
        value = (value << 8) | u64::from(b);
        all_ones &= b == 0xFF;
    }
    Some((if all_ones { None } else { Some(value) }, len))
}

fn read_uint(payload: &[u8]) -> Option<u64> {
    if payload.is_empty() || payload.len() > 8 {
        return None;
    }
    Some(payload.iter().fold(0u64, |a, &b| (a << 8) | u64::from(b)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Um elemento EBML com tamanho de 1 byte (para payloads < 127).
    fn el(id: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut v = id.to_vec();
        v.push(0x80 | payload.len() as u8);
        v.extend_from_slice(payload);
        v
    }

    /// Um cabeçalho como o do MediaRecorder: EBML, Segment de tamanho
    /// desconhecido, Info, Tracks com um vídeo e um áudio.
    pub(crate) fn header(width: u16, height: u16) -> Vec<u8> {
        let mut h = el(&[0x1A, 0x45, 0xDF, 0xA3], &el(&[0x42, 0x82], b"webm"));
        h.extend_from_slice(&[0x18, 0x53, 0x80, 0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        h.extend(el(&[0x15, 0x49, 0xA9, 0x66], &el(&[0x2A, 0xD7, 0xB1], &[0x0F, 0x42, 0x40])));
        let video = el(
            &[0xE0],
            &[
                el(&[0xB0], &width.to_be_bytes()),
                el(&[0xBA], &height.to_be_bytes()),
            ]
            .concat(),
        );
        let v_entry = el(&[0xAE], &[el(&[0xD7], &[1]), el(&[0x86], b"V_MPEG4/ISO/AVC"), video].concat());
        let a_entry = el(&[0xAE], &[el(&[0xD7], &[2]), el(&[0x86], b"A_OPUS")].concat());
        h.extend(el(&[0x16, 0x54, 0xAE, 0x6B], &[v_entry, a_entry].concat()));
        h
    }

    /// Um Cluster com Timestamp `ts` e um bloco de `n` bytes; `crc` põe o
    /// CRC-32 primeiro, como o muxer do ffmpeg.
    pub(crate) fn cluster(ts: u32, n: usize, crc: bool) -> Vec<u8> {
        let mut body = Vec::new();
        if crc {
            body.extend_from_slice(&[0xBF, 0x84, 1, 2, 3, 4]);
        }
        body.extend(el(&[0xE7], &ts.to_be_bytes()));
        body.push(0xA3); // SimpleBlock
        body.extend_from_slice(&[0x40 | ((n >> 8) as u8 & 0x3F), n as u8]);
        body.extend(std::iter::repeat_n(0x1F, n)); // dados que contêm meio id de Cluster
        let mut c = CLUSTER_ID.to_vec();
        c.push(0x01);
        c.extend_from_slice(&(body.len() as u64).to_be_bytes()[1..]);
        c.extend(body);
        c
    }

    #[test]
    fn encontra_o_cluster_nos_dois_formatos_e_nao_nos_dados() {
        for crc in [false, true] {
            let mut buf = header(3840, 2160);
            let h = buf.len();
            buf.extend(cluster(0, 300, crc));
            assert_eq!(cluster_start(&buf), Some(h), "crc={crc}");
        }
        // Um id de Cluster perdido no meio dos dados, sem Timestamp a seguir.
        let mut falso = vec![0u8; 50];
        falso.extend_from_slice(&CLUSTER_ID);
        falso.extend_from_slice(&[0x81, 0x00, 0x00]);
        assert_eq!(cluster_start(&falso), None);
    }

    #[test]
    fn le_a_resolucao_do_cabecalho() {
        assert_eq!(video_dimensions(&header(3840, 2160)), Some((3840, 2160)));
        assert_eq!(video_dimensions(&header(1920, 1080)), Some((1920, 1080)));
        assert_eq!(video_dimensions(b"lixo sem ebml"), None);
        assert_eq!(video_dimensions(&[]), None);
    }

    #[test]
    fn le_o_timestamp_do_cluster() {
        for crc in [false, true] {
            let c = cluster(4_326_000, 10, crc);
            assert_eq!(cluster_timestamp(&c), Some(4_326_000), "crc={crc}");
            // Cortado a meio do Timestamp: ainda não se sabe.
            assert_eq!(cluster_timestamp(&c[..8]), None);
        }
    }

    #[test]
    fn tamanhos_ebml() {
        assert_eq!(read_size(&[0x81]), Some((Some(1), 1)));
        assert_eq!(read_size(&[0x40, 0x02]), Some((Some(2), 2)));
        assert_eq!(read_size(&[0xFF]), Some((None, 1)));
        assert_eq!(
            read_size(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
            Some((None, 8))
        );
        assert_eq!(read_size(&[0x00]), None);
    }
}
