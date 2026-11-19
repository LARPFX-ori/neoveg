//! Pacote do programa anexado ao fim do `setup.exe`.
//!
//! Formato (montado por `tools/build_installer.py`):
//!
//! ```text
//! [executável do instalador][ZIP com a pasta dist/][tamanho do ZIP: u64 LE][b"NVSETUP1"]
//! ```
//!
//! O instalador lê o próprio executável, acha o trailer de 16 bytes no fim e
//! abre o ZIP como uma região do arquivo, sem carregá-lo na memória (§3.6).
//! O arquivo é tratado como entrada hostil (§0.4): o trailer é validado, o
//! número de entradas e o total descompactado têm limite, e nenhum caminho
//! escapa da pasta de destino.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Assinatura no fim do instalador.
pub const MAGIC: &[u8; 8] = b"NVSETUP1";
/// Tamanho do trailer: tamanho do ZIP (u64 LE) + assinatura.
pub const TRAILER_LEN: u64 = 16;
/// Maior pacote aceito.
pub const MAX_PAYLOAD: u64 = 4 << 30;
/// Mais entradas que um pacote pode ter.
pub const MAX_ENTRIES: usize = 20_000;
/// Mais bytes descompactados no total.
pub const MAX_UNPACKED: u64 = 8 << 30;

/// Onde está o pacote dentro do executável.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PayloadSpan {
    /// Início do ZIP (também o tamanho do executável sem o pacote).
    pub start: u64,
    /// Tamanho do ZIP.
    pub len: u64,
}

/// Lê o trailer de um arquivo de `file_len` bytes.
pub fn parse_trailer(file_len: u64, trailer: &[u8; 16]) -> Result<PayloadSpan, String> {
    let (len_bytes, magic) = trailer.split_at(8);
    if magic != MAGIC {
        return Err("o instalador não traz o pacote do programa".into());
    }
    let mut raw = [0u8; 8];
    raw.copy_from_slice(len_bytes);
    let len = u64::from_le_bytes(raw);
    if len == 0 || len > MAX_PAYLOAD {
        return Err(format!("tamanho de pacote inválido: {len}"));
    }
    let start = file_len
        .checked_sub(TRAILER_LEN)
        .and_then(|v| v.checked_sub(len))
        .ok_or_else(|| "pacote maior que o instalador".to_string())?;
    Ok(PayloadSpan { start, len })
}

/// Acha o pacote num arquivo aberto.
pub fn find_payload<R: Read + Seek>(file: &mut R) -> Result<PayloadSpan, String> {
    let file_len = file.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
    if file_len < TRAILER_LEN {
        return Err("instalador truncado".into());
    }
    file.seek(SeekFrom::Start(file_len - TRAILER_LEN))
        .map_err(|e| e.to_string())?;
    let mut trailer = [0u8; 16];
    file.read_exact(&mut trailer).map_err(|e| e.to_string())?;
    parse_trailer(file_len, &trailer)
}

/// Uma região `[start, start + len)` de um arquivo vista como arquivo próprio.
pub struct Region<R> {
    inner: R,
    start: u64,
    len: u64,
    pos: u64,
}

impl<R: Read + Seek> Region<R> {
    pub fn new(inner: R, span: PayloadSpan) -> Self {
        Region {
            inner,
            start: span.start,
            len: span.len,
            pos: 0,
        }
    }
}

impl<R: Read + Seek> Read for Region<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.len.saturating_sub(self.pos);
        if left == 0 || buf.is_empty() {
            return Ok(0);
        }
        let want = usize::try_from(left).map_or(buf.len(), |l| l.min(buf.len()));
        let abs = self
            .start
            .checked_add(self.pos)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "posição inválida"))?;
        self.inner.seek(SeekFrom::Start(abs))?;
        let slice = buf
            .get_mut(..want)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "buffer"))?;
        let n = self.inner.read(slice)?;
        self.pos = self.pos.saturating_add(n as u64);
        Ok(n)
    }
}

impl<R: Read + Seek> Seek for Region<R> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let bad = || io::Error::new(io::ErrorKind::InvalidInput, "busca fora do pacote");
        let next = match to {
            SeekFrom::Start(p) => Some(p),
            SeekFrom::End(d) => i128::from(self.len)
                .checked_add(i128::from(d))
                .and_then(|v| u64::try_from(v).ok()),
            SeekFrom::Current(d) => i128::from(self.pos)
                .checked_add(i128::from(d))
                .and_then(|v| u64::try_from(v).ok()),
        }
        .ok_or_else(bad)?;
        self.pos = next;
        Ok(next)
    }
}

/// Abre o pacote do executável em `exe`.
pub fn open_payload(exe: &Path) -> Result<(Region<File>, PayloadSpan), String> {
    let mut f = File::open(exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    let span = find_payload(&mut f)?;
    Ok((Region::new(f, span), span))
}

/// Caminho relativo seguro de uma entrada: sem raiz, sem `..`, sem prefixo
/// de unidade. `None` para caminhos que escapariam da pasta de destino.
pub fn safe_relative(name: &str) -> Option<PathBuf> {
    use std::path::Component;
    let normalized = name.replace('\\', "/");
    if normalized.is_empty() || normalized.starts_with('/') || normalized.contains(':') {
        return None;
    }
    let mut out = PathBuf::new();
    for c in Path::new(&normalized).components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Total descompactado e número de arquivos do pacote (para a barra de
/// progresso e para conferir o espaço), com os limites aplicados.
pub fn payload_summary<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<(u64, usize), String> {
    if archive.len() > MAX_ENTRIES {
        return Err(format!("pacote com entradas demais ({})", archive.len()));
    }
    let mut total: u64 = 0;
    let mut files = 0usize;
    for i in 0..archive.len() {
        let e = archive.by_index_raw(i).map_err(|e| e.to_string())?;
        if !e.is_dir() {
            files += 1;
            total = total
                .checked_add(e.size())
                .filter(|t| *t <= MAX_UNPACKED)
                .ok_or_else(|| "pacote grande demais".to_string())?;
        }
    }
    Ok((total, files))
}

/// Extrai o pacote em `dest`. Devolve os caminhos relativos dos arquivos
/// gravados (o manifesto da desinstalação). `progress(feito, total)` é
/// chamado a cada arquivo.
pub fn extract_all<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    dest: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Vec<PathBuf>, String> {
    let (total, _) = payload_summary(archive)?;
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut done: u64 = 0;
    let mut written = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let rel = safe_relative(entry.name())
            .ok_or_else(|| format!("caminho inválido no pacote: {}", entry.name()))?;
        let out = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let declared = entry.size();
        let mut f = File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        // nunca grava mais que o declarado (bomba de descompressão)
        let n = io::copy(&mut (&mut entry).take(declared), &mut f)
            .map_err(|e| format!("{}: {e}", out.display()))?;
        if n != declared {
            return Err(format!("{}: pacote corrompido", rel.display()));
        }
        done = done.saturating_add(n);
        written.push(rel);
        progress(done, total);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in files {
            w.start_file(*name, opts).expect("entrada");
            w.write_all(data).expect("dados");
        }
        w.finish().expect("zip").into_inner()
    }

    fn with_payload(stub: &[u8], zip: &[u8]) -> Vec<u8> {
        let mut out = stub.to_vec();
        out.extend_from_slice(zip);
        out.extend_from_slice(&(zip.len() as u64).to_le_bytes());
        out.extend_from_slice(MAGIC);
        out
    }

    #[test]
    fn acha_e_extrai_o_pacote_anexado() {
        let zip = zip_bytes(&[("neoveg.exe", b"exe"), ("i18n/pt-BR.json", b"{}")]);
        let exe = with_payload(b"STUB-DO-INSTALADOR", &zip);
        let mut cur = Cursor::new(exe);
        let span = find_payload(&mut cur).expect("trailer");
        assert_eq!(span.start, b"STUB-DO-INSTALADOR".len() as u64);
        assert_eq!(span.len, zip.len() as u64);
        let mut archive = zip::ZipArchive::new(Region::new(cur, span)).expect("zip na região");
        let dir = std::env::temp_dir().join(format!("neoveg-setup-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut calls = 0;
        let files = extract_all(&mut archive, &dir, &mut |_, _| calls += 1).expect("extrai");
        assert_eq!(files.len(), 2);
        assert_eq!(calls, 2);
        assert_eq!(std::fs::read(dir.join("neoveg.exe")).expect("lê"), b"exe");
        assert_eq!(
            std::fs::read(dir.join("i18n").join("pt-BR.json")).expect("lê"),
            b"{}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trailer_hostil_e_erro_sem_panico() {
        // sem assinatura
        assert!(parse_trailer(100, &[0u8; 16]).is_err());
        // tamanho zero, maior que o arquivo e gigante
        let t = |len: u64| {
            let mut t = [0u8; 16];
            t[..8].copy_from_slice(&len.to_le_bytes());
            t[8..].copy_from_slice(MAGIC);
            t
        };
        assert!(parse_trailer(100, &t(0)).is_err());
        assert!(parse_trailer(100, &t(90)).is_err());
        assert!(parse_trailer(u64::MAX, &t(u64::MAX)).is_err());
        assert_eq!(
            parse_trailer(100, &t(84)),
            Ok(PayloadSpan { start: 0, len: 84 })
        );
        // arquivo menor que o trailer, e vazio
        assert!(find_payload(&mut Cursor::new(vec![1u8; 5])).is_err());
        assert!(find_payload(&mut Cursor::new(Vec::new())).is_err());
        // pacote que não é ZIP
        let exe = with_payload(b"stub", b"isto nao e zip");
        let mut cur = Cursor::new(exe);
        let span = find_payload(&mut cur).expect("trailer válido");
        assert!(zip::ZipArchive::new(Region::new(cur, span)).is_err());
    }

    #[test]
    fn caminhos_do_pacote_nao_escapam_da_pasta() {
        assert_eq!(
            safe_relative("a/b.txt"),
            Some(PathBuf::from("a").join("b.txt"))
        );
        assert_eq!(safe_relative("./a.txt"), Some(PathBuf::from("a.txt")));
        for bad in [
            "",
            "/etc/passwd",
            "../fora.txt",
            "a/../../fora",
            "C:/Windows/x",
            "\\\\srv\\x",
            "a\\..\\..\\x",
        ] {
            assert_eq!(safe_relative(bad), None, "{bad}");
        }
    }

    #[test]
    fn regiao_nao_le_alem_do_pacote() {
        let data = b"0123456789".to_vec();
        let mut r = Region::new(Cursor::new(data), PayloadSpan { start: 2, len: 5 });
        let mut out = Vec::new();
        r.read_to_end(&mut out).expect("lê");
        assert_eq!(out, b"23456");
        assert_eq!(r.seek(SeekFrom::End(-1)).expect("busca"), 4);
        assert!(r.seek(SeekFrom::Current(-100)).is_err());
    }
}
