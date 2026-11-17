//! Cliente em Sala Limpa do canal de controle JSON-lines (127.0.0.1) dos
//! aplicativos ArtCraft (PhotoCraft, VectorCraft, FilmCraft, EffectCraft e
//! SoundCraft), implementado exclusivamente a partir das especificações públicas
//! de `docs/control-protocol.md` (`AGENTS.md` §2.3).
//!
//! Suporta autenticação por desafio HMAC-SHA256 do PhotoCraft (RFC 2104 /
//! RFC 6234), token por requisição do FilmCraft e canal loopback direto para
//! VectorCraft, EffectCraft e SoundCraft. Toda leitura de socket é limitada em
//! tamanho e tempo limite (§0.4).

use super::discovery::{read_agent_connection, read_hex_token, AppDiscoveryStatus};
use super::ExternalApp;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Limite máximo de uma linha JSON recebida do socket de controle (4 MiB, §0.4).
pub const MAX_RPC_LINE_BYTES: usize = 4 * 1024 * 1024;

static NONCE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Modo de autenticação na conexão TCP loopback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlAuth {
    /// Sem autenticação explícita (VectorCraft, EffectCraft, SoundCraft).
    None,
    /// Handshake desafio-resposta HMAC-SHA256 do PhotoCraft.
    PhotoCraftChallenge { token_hex: String },
    /// Token enviado no corpo JSON de cada requisição (FilmCraft).
    PerRequestToken { token_hex: String },
}

/// Conexão TCP ativa (loopback apenas) com um aplicativo ArtCraft.
pub struct CraftClient {
    app: ExternalApp,
    auth: ControlAuth,
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    next_id: u64,
}

impl CraftClient {
    /// Conecta a `127.0.0.1:<port>`, aplica timeouts e autentica quando exigido.
    pub fn connect(
        app: ExternalApp,
        port: u16,
        auth: ControlAuth,
        timeout: Duration,
    ) -> Result<Self> {
        if port == 0 {
            bail!("porta de controle inválida (0)");
        }
        if !app.supports_loopback_rpc() {
            bail!(
                "{} não expõe canal de controle TCP direto; use a CLI dedicada",
                app.name()
            );
        }

        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
        let connect_timeout = timeout.min(Duration::from_secs(5));
        let stream = TcpStream::connect_timeout(&addr, connect_timeout).with_context(|| {
            format!(
                "não foi possível conectar ao canal de controle de {} em 127.0.0.1:{port}",
                app.name()
            )
        })?;
        let _ = stream.set_read_timeout(Some(timeout));
        let _ = stream.set_write_timeout(Some(timeout));
        let cloned = stream
            .try_clone()
            .context("falha ao clonar socket de controle")?;

        let mut client = Self {
            app,
            auth,
            stream,
            reader: BufReader::new(cloned),
            next_id: 1,
        };

        if let ControlAuth::PhotoCraftChallenge { ref token_hex } = client.auth.clone() {
            client.authenticate_photocraft_challenge(token_hex)?;
        }

        Ok(client)
    }

    /// Conecta a partir de uma descoberta ativa (`agent-connection.json` ou porta conhecida).
    pub fn connect_discovered(
        status: &AppDiscoveryStatus,
        override_token: Option<&str>,
        timeout: Duration,
    ) -> Result<Self> {
        let app = status.app;
        let port = status.live_port.ok_or_else(|| {
            anyhow!(
                "{} não possui canal de controle ativo anunciado",
                app.name()
            )
        })?;

        let auth = match app {
            ExternalApp::PhotoCraft => {
                let token = if let Some(raw) = override_token {
                    super::discovery::normalize_hex_token(raw)
                        .ok_or_else(|| anyhow!("token do PhotoCraft inválido (esperado 64 hex)"))?
                } else {
                    let conn = status
                        .settings_dir
                        .as_ref()
                        .and_then(|d| read_agent_connection(&d.join("agent-connection.json")));
                    let tok_path = conn
                        .as_ref()
                        .filter(|c| !c.token_file.trim().is_empty())
                        .map(|c| Path::new(&c.token_file).to_path_buf())
                        .or_else(|| {
                            status
                                .settings_dir
                                .as_ref()
                                .map(|d| d.join("agent-control.token"))
                        })
                        .ok_or_else(|| anyhow!("pasta de token do PhotoCraft não encontrada"))?;
                    read_hex_token(&tok_path).ok_or_else(|| {
                        anyhow!(
                            "token de controle do PhotoCraft ausente ou ilegível em {}",
                            tok_path.display()
                        )
                    })?
                };
                ControlAuth::PhotoCraftChallenge { token_hex: token }
            }
            ExternalApp::FilmCraft => {
                let token = override_token
                    .and_then(super::discovery::normalize_hex_token)
                    .or_else(|| {
                        std::env::var("FILMCRAFT_CONTROL_TOKEN")
                            .ok()
                            .and_then(|v| super::discovery::normalize_hex_token(&v))
                    })
                    .ok_or_else(|| {
                        anyhow!("FilmCraft exige FILMCRAFT_CONTROL_TOKEN (64 caracteres hex)")
                    })?;
                ControlAuth::PerRequestToken { token_hex: token }
            }
            ExternalApp::VectorCraft | ExternalApp::EffectCraft | ExternalApp::SoundCraft => {
                ControlAuth::None
            }
            ExternalApp::AniCraft | ExternalApp::Blender => {
                bail!("{} não usa canal TCP JSON-lines", app.name());
            }
        };

        Self::connect(app, port, auth, timeout)
    }

    fn authenticate_photocraft_challenge(&mut self, token_hex: &str) -> Result<()> {
        let client_nonce = generate_client_nonce_hex();
        let chal_res = self.send_raw(
            "auth.challenge",
            json!({ "clientNonce": client_nonce }),
            false,
        )?;
        let server_nonce = chal_res
            .get("serverNonce")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("resposta auth.challenge sem serverNonce"))?
            .to_ascii_lowercase();
        let server_proof = chal_res
            .get("serverProof")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("resposta auth.challenge sem serverProof"))?
            .to_ascii_lowercase();

        if server_nonce.len() != 64 || !server_nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("serverNonce inválido na autenticação PhotoCraft");
        }

        let key_bytes = token_hex.as_bytes();
        let server_msg = format!("photocraft-control-v1 server\n{client_nonce}\n{server_nonce}");
        let expected_server = hmac_sha256_hex(key_bytes, server_msg.as_bytes());
        if !constant_time_eq_ascii(&server_proof, &expected_server) {
            bail!("falha na verificação serverProof (porta pode pertencer a outro processo)");
        }

        let client_msg = format!("photocraft-control-v1 client\n{client_nonce}\n{server_nonce}");
        let client_proof = hmac_sha256_hex(key_bytes, client_msg.as_bytes());
        let auth_res = self.send_raw("auth", json!({ "proof": client_proof }), false)?;
        let ok = auth_res
            .get("authenticated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !ok {
            bail!("autenticação HMAC rejeitada pelo PhotoCraft");
        }
        Ok(())
    }

    /// Envia um método RPC genérico e devolve o campo `result`.
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.send_raw(method, params, true)
    }

    /// Executa um comando de motor via `engine.execute {command, params}`.
    pub fn engine_execute(&mut self, command: &str, params: Value) -> Result<Value> {
        let params_obj = if params.is_null() { json!({}) } else { params };
        self.request(
            "engine.execute",
            json!({
                "command": command,
                "params": params_obj,
            }),
        )
    }

    /// Solicita ao aplicativo em execução que abra um arquivo local no fluxo ativo.
    pub fn open_file(&mut self, path: &Path) -> Result<Value> {
        let path_str = path.to_string_lossy().to_string();
        match self.app {
            ExternalApp::VectorCraft => self.request("app.open", json!({ "path": path_str })),
            ExternalApp::PhotoCraft | ExternalApp::FilmCraft | ExternalApp::EffectCraft => {
                self.engine_execute("file.open", json!({ "path": path_str }))
            }
            ExternalApp::SoundCraft => {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if ext == "scraft" {
                    self.engine_execute("session.open", json!({ "path": path_str }))
                } else {
                    self.engine_execute("file.import_audio", json!({ "path": path_str }))
                }
            }
            ExternalApp::AniCraft | ExternalApp::Blender => {
                bail!("{} não abre arquivos via RPC loopback", self.app.name());
            }
        }
    }

    fn send_raw(&mut self, method: &str, params: Value, attach_token: bool) -> Result<Value> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let mut obj = Map::new();
        obj.insert("id".to_string(), Value::from(id));
        if attach_token {
            if let ControlAuth::PerRequestToken { ref token_hex } = self.auth {
                obj.insert("token".to_string(), Value::String(token_hex.clone()));
            }
        }
        obj.insert("method".to_string(), Value::String(method.to_string()));
        obj.insert("params".to_string(), params);

        let mut encoded = serde_json::to_string(&Value::Object(obj))
            .context("falha ao serializar requisição RPC")?;
        if encoded.len() >= MAX_RPC_LINE_BYTES {
            bail!(
                "requisição RPC excede o limite de {} bytes",
                MAX_RPC_LINE_BYTES
            );
        }
        encoded.push('\n');
        self.stream
            .write_all(encoded.as_bytes())
            .context("falha ao gravar no canal de controle")?;
        self.stream
            .flush()
            .context("falha ao enviar requisição RPC")?;

        let line = read_bounded_line(&mut self.reader, MAX_RPC_LINE_BYTES)?;
        let resp: Value = serde_json::from_str(&line)
            .with_context(|| format!("resposta JSON inválida de {}", self.app.name()))?;
        let ok = resp.get("ok").and_then(Value::as_bool).unwrap_or(false);
        if !ok {
            let err_msg = resp
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("erro desconhecido no canal de controle");
            bail!("{} respondeu erro: {err_msg}", self.app.name());
        }
        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
    }
}

fn read_bounded_line<R: BufRead>(reader: &mut R, max_bytes: usize) -> Result<String> {
    let mut buf = Vec::with_capacity(256);
    loop {
        let available = reader
            .fill_buf()
            .context("falha ao ler resposta do canal de controle")?;
        if available.is_empty() {
            if buf.is_empty() {
                bail!("conexão encerrada pelo aplicativo remoto antes da resposta");
            }
            break;
        }
        if let Some(pos) = available.iter().position(|&b| b == b'\n') {
            let take = pos + 1;
            if buf.len().saturating_add(take) > max_bytes {
                bail!("linha de resposta excedeu o limite seguro ({max_bytes} bytes)");
            }
            buf.extend_from_slice(&available[..take]);
            reader.consume(take);
            break;
        }
        if buf.len().saturating_add(available.len()) > max_bytes {
            bail!("linha de resposta excedeu o limite seguro ({max_bytes} bytes)");
        }
        let len = available.len();
        buf.extend_from_slice(available);
        reader.consume(len);
    }
    let text = String::from_utf8(buf).context("resposta RPC contém UTF-8 inválido")?;
    Ok(text.trim_end_matches(&['\r', '\n'][..]).to_string())
}

fn constant_time_eq_ascii(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in ab.iter().zip(bb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn generate_client_nonce_hex() -> String {
    let mut seed = [0u8; 64];
    for i in 0..4 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(NONCE_COUNTER.fetch_add(1, Ordering::Relaxed));
        h.write_u32(std::process::id());
        if let Ok(d) = SystemTime::now().duration_since(UNIX_EPOCH) {
            h.write_u128(d.as_nanos());
        }
        let val = h.finish().to_le_bytes();
        seed[i * 8..(i + 1) * 8].copy_from_slice(&val);
    }
    let digest = sha256(&seed);
    to_lower_hex(&digest)
}

pub fn to_lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// HMAC-SHA256 (RFC 2104) em Rust puro sem dependências externas.
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        let hashed = sha256(key);
        k_block[..32].copy_from_slice(&hashed);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = Vec::with_capacity(BLOCK + msg.len());
    let mut opad = Vec::with_capacity(BLOCK + 32);
    for &b in &k_block {
        ipad.push(b ^ 0x36);
        opad.push(b ^ 0x5c);
    }
    ipad.extend_from_slice(msg);
    let inner = sha256(&ipad);
    opad.extend_from_slice(&inner);
    sha256(&opad)
}

pub fn hmac_sha256_hex(key: &[u8], msg: &[u8]) -> String {
    to_lower_hex(&hmac_sha256(key, msg))
}

/// SHA-256 (FIPS 180-4 / RFC 6234) em Rust puro sem `unsafe`.
pub fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(input.len() + 64 + 8);
    padded.extend_from_slice(input);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    let (chunks, _) = padded.as_chunks::<64>();
    for chunk in chunks {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn vetores_rfc_sha256_e_hmac() {
        // FIPS 180-4 "abc"
        assert_eq!(
            to_lower_hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // RFC 4231 Case 2: key = "Jefe", data = "what do ya want for nothing?"
        assert_eq!(
            hmac_sha256_hex(b"Jefe", b"what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn photocraft_challenge_auth_valida_servidor_e_envia_comandos() {
        let token_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let tok_srv = token_hex.to_string();

        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("accept");
            let mut rd = BufReader::new(sock.try_clone().expect("clone"));
            let mut l1 = String::new();
            rd.read_line(&mut l1).expect("chal");
            let v1: Value = serde_json::from_str(&l1).expect("json1");
            assert_eq!(v1["method"], "auth.challenge");
            let cnonce = v1["params"]["clientNonce"].as_str().expect("cnonce");
            let snonce = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
            let sp = hmac_sha256_hex(
                tok_srv.as_bytes(),
                format!("photocraft-control-v1 server\n{cnonce}\n{snonce}").as_bytes(),
            );
            writeln!(
                sock,
                "{}",
                json!({"id": v1["id"], "ok": true, "result": {"serverNonce": snonce, "serverProof": sp}})
            )
            .expect("w1");

            let mut l2 = String::new();
            rd.read_line(&mut l2).expect("auth");
            let v2: Value = serde_json::from_str(&l2).expect("json2");
            let cproof = v2["params"]["proof"].as_str().expect("proof");
            let expected_cp = hmac_sha256_hex(
                tok_srv.as_bytes(),
                format!("photocraft-control-v1 client\n{cnonce}\n{snonce}").as_bytes(),
            );
            assert_eq!(cproof, expected_cp);
            writeln!(
                sock,
                "{}",
                json!({"id": v2["id"], "ok": true, "result": {"authenticated": true}})
            )
            .expect("w2");

            let mut l3 = String::new();
            rd.read_line(&mut l3).expect("exec");
            let v3: Value = serde_json::from_str(&l3).expect("json3");
            assert_eq!(v3["method"], "engine.execute");
            assert_eq!(v3["params"]["command"], "file.open");
            writeln!(
                sock,
                "{}",
                json!({"id": v3["id"], "ok": true, "result": {"opened": true}})
            )
            .expect("w3");
        });

        let mut client = CraftClient::connect(
            ExternalApp::PhotoCraft,
            port,
            ControlAuth::PhotoCraftChallenge {
                token_hex: token_hex.to_string(),
            },
            Duration::from_secs(2),
        )
        .expect("conecta e autentica");

        let res = client
            .open_file(Path::new("/tmp/cena.png"))
            .expect("file.open");
        assert_eq!(res["opened"], true);
        handle.join().expect("srv join");
    }

    #[test]
    fn photocraft_rejeita_server_proof_forjado() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();

        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("accept");
            let mut rd = BufReader::new(sock.try_clone().expect("clone"));
            let mut l1 = String::new();
            rd.read_line(&mut l1).expect("chal");
            let snonce = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
            let forged = "0000000000000000000000000000000000000000000000000000000000000000";
            writeln!(
                sock,
                "{}",
                json!({"id": 1, "ok": true, "result": {"serverNonce": snonce, "serverProof": forged}})
            )
            .expect("w1");
        });

        let err = CraftClient::connect(
            ExternalApp::PhotoCraft,
            port,
            ControlAuth::PhotoCraftChallenge {
                token_hex: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    .to_string(),
            },
            Duration::from_secs(2),
        );
        assert!(err.is_err());
        handle.join().expect("srv join");
    }

    #[test]
    fn filmcraft_envia_token_em_todas_as_requisicoes() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let token = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("accept");
            let mut rd = BufReader::new(sock.try_clone().expect("clone"));
            let mut line = String::new();
            rd.read_line(&mut line).expect("line");
            let v: Value = serde_json::from_str(&line).expect("json");
            assert_eq!(v["token"], token);
            assert_eq!(v["method"], "ui.inspect");
            writeln!(
                sock,
                "{}",
                json!({"id": v["id"], "ok": true, "result": {"fps": 24}})
            )
            .expect("reply");
        });

        let mut client = CraftClient::connect(
            ExternalApp::FilmCraft,
            port,
            ControlAuth::PerRequestToken {
                token_hex: token.to_string(),
            },
            Duration::from_secs(2),
        )
        .expect("connect");
        let res = client.request("ui.inspect", json!({})).expect("inspect");
        assert_eq!(res["fps"], 24);
        handle.join().expect("join");
    }
}
