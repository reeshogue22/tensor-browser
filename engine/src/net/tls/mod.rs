/// TLS 1.2 — full handshake, record layer, encrypted stream.
/// Zero dependencies. Cipher suite: TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256

pub mod sha256;
pub mod aes;
pub mod p256;
pub mod x25519;

use std::io::{self, Read, Write};
use sha256::{sha256, tls_prf};
use aes::AesGcm;
use p256::EcdhKeypair;
use x25519::X25519Keypair;

// ── TLS Record Layer ────────────────────────────────────────────────────────

const TLS_CHANGE_CIPHER_SPEC: u8 = 20;
const TLS_ALERT: u8 = 21;
const TLS_HANDSHAKE: u8 = 22;
const TLS_APPLICATION_DATA: u8 = 23;

const TLS_12: [u8; 2] = [0x03, 0x03]; // TLS 1.2

// Handshake message types
const HS_CLIENT_HELLO: u8 = 1;
const HS_SERVER_HELLO: u8 = 2;
const HS_CERTIFICATE: u8 = 11;
const HS_SERVER_KEY_EXCHANGE: u8 = 12;
const HS_SERVER_HELLO_DONE: u8 = 14;
const HS_CLIENT_KEY_EXCHANGE: u8 = 16;
const HS_FINISHED: u8 = 20;

// TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256
const CIPHER_SUITE: [u8; 2] = [0xc0, 0x2f];

// ── Handshake message buffer ────────────────────────────────────────────────
// Servers often pack multiple handshake messages into one TLS record.
// This splits them out by reading the 4-byte handshake header (type + 3-byte length).

struct HandshakeReader {
    buf: Vec<u8>,
    pos: usize,
}

impl HandshakeReader {
    fn new() -> Self {
        Self { buf: Vec::new(), pos: 0 }
    }

    fn feed(&mut self, data: &[u8]) {
        // Append new data, compacting if needed
        if self.pos > 0 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        self.buf.extend_from_slice(data);
    }

    fn next_message(&mut self) -> Option<Vec<u8>> {
        let remaining = &self.buf[self.pos..];
        if remaining.len() < 4 { return None; }

        let msg_len = ((remaining[1] as usize) << 16)
                    | ((remaining[2] as usize) << 8)
                    | (remaining[3] as usize);
        let total = 4 + msg_len;
        if remaining.len() < total { return None; }

        let msg = remaining[..total].to_vec();
        self.pos += total;
        Some(msg)
    }

    fn has_data(&self) -> bool {
        self.pos < self.buf.len()
    }
}

// ── TLS Stream ──────────────────────────────────────────────────────────────

pub struct TlsStream<S: Read + Write> {
    stream: S,
    client_write_iv: [u8; 4],
    server_write_iv: [u8; 4],
    client_seq: u64,
    server_seq: u64,
    client_gcm: AesGcm,
    server_gcm: AesGcm,
    read_buf: Vec<u8>,
    read_pos: usize,
}

impl<S: Read + Write> TlsStream<S> {
    /// Perform TLS 1.2 handshake and return encrypted stream
    pub fn connect(mut stream: S, hostname: &str) -> io::Result<Self> {
        let mut handshake_messages = Vec::new();
        let mut hs_reader = HandshakeReader::new();

        let ecdh_p256 = EcdhKeypair::generate();
        let ecdh_x25519 = X25519Keypair::generate();

        // ── ClientHello ─────────────────────────────────────────────────
        let client_random = random_bytes::<32>();
        let client_hello = build_client_hello(&client_random, hostname);
        handshake_messages.extend_from_slice(&client_hello);
        send_record(&mut stream, TLS_HANDSHAKE, &client_hello)?;

        // ── Read server handshake messages ──────────────────────────────
        let mut server_random = [0u8; 32];
        let mut server_pubkey: Option<Vec<u8>> = None;
        let mut server_curve: u16 = 0;
        let mut got_hello = false;
        let mut got_done = false;
        let mut use_ems = false;

        while !got_done {
            let rec = read_record(&mut stream)?;
            if rec.content_type == TLS_ALERT {
                return Err(io::Error::new(io::ErrorKind::ConnectionRefused,
                    format!("TLS alert: level={} desc={}", rec.data.get(0).copied().unwrap_or(0), rec.data.get(1).copied().unwrap_or(0))));
            }
            if rec.content_type != TLS_HANDSHAKE { continue; }

            hs_reader.feed(&rec.data);
            while let Some(msg) = hs_reader.next_message() {
                handshake_messages.extend_from_slice(&msg);
                match msg[0] {
                    HS_SERVER_HELLO => {
                        server_random.copy_from_slice(&parse_server_hello(&msg)?);
                        // Check for extended_master_secret in server hello extensions
                        if let Some(ext_start) = find_extensions_in_server_hello(&msg) {
                            let ext_data = &msg[ext_start..];
                            if ext_data.len() >= 2 {
                                let ext_len = ((ext_data[0] as usize) << 8) | ext_data[1] as usize;
                                let mut pos = 2;
                                while pos + 4 <= ext_data.len().min(2 + ext_len) {
                                    let etype = ((ext_data[pos] as u16) << 8) | ext_data[pos+1] as u16;
                                    let elen = ((ext_data[pos+2] as usize) << 8) | ext_data[pos+3] as usize;
                                    if etype == 0x0017 { use_ems = true; }
                                    pos += 4 + elen;
                                }
                            }
                        }
                        // Remove debug output from x25519 inner

                        got_hello = true;
                    }
                    HS_CERTIFICATE => {}
                    HS_SERVER_KEY_EXCHANGE => {
                        let (curve, pubkey) = parse_server_key_exchange(&msg)?;
                        server_curve = curve;
                        server_pubkey = Some(pubkey);
                    }
                    HS_SERVER_HELLO_DONE => {
                        got_done = true;
                        break;
                    }
                    _ => {}
                }
            }
        }

        if !got_hello {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "never got ServerHello"));
        }
        let server_pubkey = server_pubkey
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no ServerKeyExchange"))?;

        // ── ClientKeyExchange + ECDH shared secret ──────────────────────
        let (cke, premaster) = if server_curve == 0x001d {
            // x25519 — 32-byte keys
            let cke = build_client_key_exchange_raw(&ecdh_x25519.public_key);
            let mut peer = [0u8; 32];
            if server_pubkey.len() != 32 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "x25519 pubkey not 32 bytes"));
            }
            peer.copy_from_slice(&server_pubkey);
            let secret = ecdh_x25519.shared_secret(&peer);
            (cke, secret[..].to_vec())
        } else {
            // P-256 — 65-byte uncompressed point
            let cke = build_client_key_exchange(&ecdh_p256.public_key);
            let secret = ecdh_p256.shared_secret(&server_pubkey)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "P-256 ECDH failed"))?;
            (cke, secret[..].to_vec())
        };
        handshake_messages.extend_from_slice(&cke);
        send_record(&mut stream, TLS_HANDSHAKE, &cke)?;

        let master_secret = if use_ems {
            // Extended Master Secret (RFC 7627): PRF(premaster, "extended master secret", SHA-256(handshake_messages))
            let session_hash = sha256(&handshake_messages);
            tls_prf(&premaster, b"extended master secret", &session_hash, 48)
        } else {
            let mut seed = [0u8; 64];
            seed[..32].copy_from_slice(&client_random);
            seed[32..].copy_from_slice(&server_random);
            tls_prf(&premaster, b"master secret", &seed, 48)
        };

        let mut key_seed = [0u8; 64];
        key_seed[..32].copy_from_slice(&server_random);
        key_seed[32..].copy_from_slice(&client_random);
        let key_block = tls_prf(&master_secret, b"key expansion", &key_seed, 40);

        let mut client_write_key = [0u8; 16];
        let mut server_write_key = [0u8; 16];
        let mut client_write_iv = [0u8; 4];
        let mut server_write_iv = [0u8; 4];
        client_write_key.copy_from_slice(&key_block[0..16]);
        server_write_key.copy_from_slice(&key_block[16..32]);
        client_write_iv.copy_from_slice(&key_block[32..36]);
        server_write_iv.copy_from_slice(&key_block[36..40]);

        let client_gcm = AesGcm::new(&client_write_key);
        let server_gcm = AesGcm::new(&server_write_key);

        // ── ChangeCipherSpec ────────────────────────────────────────────
        send_record(&mut stream, TLS_CHANGE_CIPHER_SPEC, &[1])?;

        // ── Client Finished ─────────────────────────────────────────────
        let handshake_hash = sha256(&handshake_messages);
        let verify_data = tls_prf(&master_secret, b"client finished", &handshake_hash, 12);

        let mut finished_msg = Vec::with_capacity(16);
        finished_msg.push(HS_FINISHED);
        let len = verify_data.len() as u32;
        finished_msg.push((len >> 16) as u8);
        finished_msg.push((len >> 8) as u8);
        finished_msg.push(len as u8);
        finished_msg.extend_from_slice(&verify_data);

        // Encrypt finished with seq 0
        let nonce = build_nonce(&client_write_iv, 0);
        let aad = build_aad(0, TLS_HANDSHAKE, finished_msg.len());
        let (ct, tag) = client_gcm.encrypt(&nonce, &aad, &finished_msg);

        let mut enc = Vec::new();
        enc.extend_from_slice(&nonce[4..]);
        enc.extend_from_slice(&ct);
        enc.extend_from_slice(&tag);
        send_record(&mut stream, TLS_HANDSHAKE, &enc)?;

        // ── Read server CCS + Finished ──────────────────────────────────
        let mut got_ccs = false;
        let mut got_finished = false;

        while !got_finished {
            let rec = read_record(&mut stream)?;
            // eprintln!("  tls: post-CKE record type={} len={}", rec.content_type, rec.data.len());
            match rec.content_type {
                TLS_CHANGE_CIPHER_SPEC => {
                    got_ccs = true;
                }
                TLS_HANDSHAKE if got_ccs => {
                    got_finished = true;
                }
                TLS_ALERT => {
                    let level = rec.data.get(0).copied().unwrap_or(0);
                    let desc = rec.data.get(1).copied().unwrap_or(0);
                    return Err(io::Error::new(io::ErrorKind::ConnectionRefused,
                        format!("TLS alert after CCS: level={} desc={}", level, desc)));
                }
                _ => {}
            }
        }
        // eprintln!("  tls: handshake complete!");

        Ok(Self {
            stream,
            client_write_iv,
            server_write_iv,
            client_seq: 1,
            server_seq: 1,
            client_gcm,
            server_gcm,
            read_buf: Vec::new(),
            read_pos: 0,
        })
    }

    fn encrypt_and_send(&mut self, content_type: u8, data: &[u8]) -> io::Result<()> {
        let nonce = build_nonce(&self.client_write_iv, self.client_seq);
        let aad = build_aad(self.client_seq, content_type, data.len());
        let (ct, tag) = self.client_gcm.encrypt(&nonce, &aad, data);

        let mut payload = Vec::with_capacity(8 + ct.len() + 16);
        payload.extend_from_slice(&nonce[4..]);
        payload.extend_from_slice(&ct);
        payload.extend_from_slice(&tag);

        send_record(&mut self.stream, content_type, &payload)?;
        self.client_seq += 1;
        Ok(())
    }

    fn decrypt_record(&mut self, rec: &TlsRecord) -> io::Result<Vec<u8>> {
        if rec.data.len() < 24 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "record too short"));
        }

        let mut nonce = [0u8; 12];
        nonce[..4].copy_from_slice(&self.server_write_iv);
        nonce[4..12].copy_from_slice(&rec.data[..8]);

        let ct_end = rec.data.len() - 16;
        let ciphertext = &rec.data[8..ct_end];
        let mut tag = [0u8; 16];
        tag.copy_from_slice(&rec.data[ct_end..]);

        let aad = build_aad(self.server_seq, rec.content_type, ciphertext.len());

        let plaintext = self.server_gcm.decrypt(&nonce, &aad, ciphertext, &tag)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "GCM tag verification failed"))?;

        self.server_seq += 1;
        Ok(plaintext)
    }
}

impl<S: Read + Write> Read for TlsStream<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.read_pos < self.read_buf.len() {
            let available = &self.read_buf[self.read_pos..];
            let n = std::cmp::min(buf.len(), available.len());
            buf[..n].copy_from_slice(&available[..n]);
            self.read_pos += n;
            if self.read_pos >= self.read_buf.len() {
                self.read_buf.clear();
                self.read_pos = 0;
            }
            return Ok(n);
        }

        loop {
            let rec = match read_record(&mut self.stream) {
                Ok(rec) => rec,
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(0),
                Err(e) => {
                    // Connection closed — treat as EOF
                    if e.to_string().contains("failed to fill whole buffer") {
                        return Ok(0);
                    }
                    return Err(e);
                }
            };
            match rec.content_type {
                TLS_APPLICATION_DATA => {
                    let plaintext = self.decrypt_record(&rec)?;
                    if plaintext.is_empty() { continue; }
                    let n = std::cmp::min(buf.len(), plaintext.len());
                    buf[..n].copy_from_slice(&plaintext[..n]);
                    if n < plaintext.len() {
                        self.read_buf = plaintext;
                        self.read_pos = n;
                    }
                    return Ok(n);
                }
                TLS_ALERT => {
                    return Ok(0);
                }
                _ => continue,
            }
        }
    }
}

impl<S: Read + Write> Write for TlsStream<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = std::cmp::min(buf.len(), 16384);
        self.encrypt_and_send(TLS_APPLICATION_DATA, &buf[..n])?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

// ── Record I/O ──────────────────────────────────────────────────────────────

struct TlsRecord {
    content_type: u8,
    data: Vec<u8>,
}

fn send_record(stream: &mut impl Write, content_type: u8, data: &[u8]) -> io::Result<()> {
    let mut header = [0u8; 5];
    header[0] = content_type;
    header[1..3].copy_from_slice(&TLS_12);
    let len = data.len() as u16;
    header[3] = (len >> 8) as u8;
    header[4] = len as u8;
    stream.write_all(&header)?;
    stream.write_all(data)?;
    stream.flush()
}

fn read_record(stream: &mut impl Read) -> io::Result<TlsRecord> {
    let mut header = [0u8; 5];
    stream.read_exact(&mut header).map_err(|e| {
        io::Error::new(e.kind(), format!("read_record header: {}", e))
    })?;

    let content_type = header[0];
    let version = ((header[1] as u16) << 8) | header[2] as u16;
    let length = ((header[3] as usize) << 8) | (header[4] as usize);


    if content_type == TLS_ALERT && length >= 2 {
        let mut data = vec![0u8; length];
        stream.read_exact(&mut data)?;
        return Ok(TlsRecord { content_type, data });
    }

    if length > 18432 {
        return Err(io::Error::new(io::ErrorKind::InvalidData,
            format!("record too large: {} bytes, type={}", length, content_type)));
    }

    let mut data = vec![0u8; length];
    stream.read_exact(&mut data)?;

    Ok(TlsRecord { content_type, data })
}

// ── Handshake message builders ──────────────────────────────────────────────

fn build_client_hello(client_random: &[u8; 32], hostname: &str) -> Vec<u8> {
    let mut msg = Vec::new();

    msg.extend_from_slice(&TLS_12);
    msg.extend_from_slice(client_random);

    // Session ID — 32 random bytes (Chrome always sends one)
    let session_id = random_bytes::<32>();
    msg.push(32);
    msg.extend_from_slice(&session_id);

    // Cipher suites — GREASE + real suites (mimics Chrome ordering)
    let suites: &[[u8; 2]] = &[
        [0x0a, 0x0a], // GREASE
        [0xc0, 0x2b], // TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256
        [0xc0, 0x2f], // TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256
        [0xc0, 0x2c], // TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384
        [0xc0, 0x30], // TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384
        [0xcc, 0xa9], // TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256
        [0xcc, 0xa8], // TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256
        [0x00, 0x9e], // TLS_DHE_RSA_WITH_AES_128_GCM_SHA256
        [0x00, 0x9f], // TLS_DHE_RSA_WITH_AES_256_GCM_SHA384
    ];
    let suite_len = (suites.len() * 2) as u16;
    msg.push((suite_len >> 8) as u8);
    msg.push(suite_len as u8);
    for s in suites { msg.extend_from_slice(s); }

    // Compression
    msg.push(1); msg.push(0);

    // Extensions
    let mut exts = Vec::new();

    // GREASE extension
    push_ext(&mut exts, 0x0a0a, &[0x00]);

    // SNI
    let sni = build_sni_extension(hostname);
    exts.extend_from_slice(&sni);

    // Extended Master Secret (0x0017)
    push_ext(&mut exts, 0x0017, &[]);

    // Renegotiation Info (0xff01) — empty
    push_ext(&mut exts, 0xff01, &[0x00]);

    // Supported Groups — GREASE + real curves
    let groups: &[[u8; 2]] = &[
        [0x0a, 0x0a], // GREASE
        [0x00, 0x1d], // x25519
        [0x00, 0x17], // secp256r1
        [0x00, 0x18], // secp384r1
    ];
    let gl = (groups.len() * 2) as u16;
    let mut gdata = Vec::new();
    gdata.push((gl >> 8) as u8);
    gdata.push(gl as u8);
    for g in groups { gdata.extend_from_slice(g); }
    push_ext(&mut exts, 0x000a, &gdata);

    // EC Point Formats
    push_ext(&mut exts, 0x000b, &[0x01, 0x00]);

    // Session Ticket (0x0023) — empty (we support it but have no ticket)
    push_ext(&mut exts, 0x0023, &[]);

    // ALPN (0x0010) — http/1.1
    let alpn_proto = b"http/1.1";
    let mut alpn_data = Vec::new();
    let alpn_list_len = (1 + alpn_proto.len()) as u16;
    alpn_data.push((alpn_list_len >> 8) as u8);
    alpn_data.push(alpn_list_len as u8);
    alpn_data.push(alpn_proto.len() as u8);
    alpn_data.extend_from_slice(alpn_proto);
    push_ext(&mut exts, 0x0010, &alpn_data);

    // Signature Algorithms (0x000d)
    let sig_algs: &[[u8; 2]] = &[
        [0x04, 0x03], // ecdsa_secp256r1_sha256
        [0x08, 0x04], // rsa_pss_rsae_sha256
        [0x04, 0x01], // rsa_pkcs1_sha256
        [0x08, 0x05], // rsa_pss_rsae_sha384
        [0x08, 0x06], // rsa_pss_rsae_sha512
        [0x05, 0x01], // rsa_pkcs1_sha384
        [0x06, 0x01], // rsa_pkcs1_sha512
        [0x02, 0x01], // rsa_pkcs1_sha1
    ];
    let sig_len = (sig_algs.len() * 2) as u16;
    let mut sig_data = Vec::new();
    sig_data.push((sig_len >> 8) as u8);
    sig_data.push(sig_len as u8);
    for sa in sig_algs { sig_data.extend_from_slice(sa); }
    push_ext(&mut exts, 0x000d, &sig_data);

    // Status Request / OCSP Stapling (0x0005)
    push_ext(&mut exts, 0x0005, &[0x01, 0x00, 0x00, 0x00, 0x00]);

    // Signed Certificate Timestamps (0x0012)
    push_ext(&mut exts, 0x0012, &[]);

    // GREASE extension (another one, Chrome sends two)
    push_ext(&mut exts, 0x3a3a, &[0x00]);

    let ext_total = exts.len() as u16;
    msg.push((ext_total >> 8) as u8);
    msg.push(ext_total as u8);
    msg.extend_from_slice(&exts);

    // Wrap in handshake header
    let mut hs = Vec::new();
    hs.push(HS_CLIENT_HELLO);
    let len = msg.len() as u32;
    hs.push((len >> 16) as u8);
    hs.push((len >> 8) as u8);
    hs.push(len as u8);
    hs.extend_from_slice(&msg);
    hs
}

fn push_ext(buf: &mut Vec<u8>, ext_type: u16, data: &[u8]) {
    buf.push((ext_type >> 8) as u8);
    buf.push(ext_type as u8);
    let len = data.len() as u16;
    buf.push((len >> 8) as u8);
    buf.push(len as u8);
    buf.extend_from_slice(data);
}

fn build_sni_extension(hostname: &str) -> Vec<u8> {
    let name = hostname.as_bytes();
    let mut ext = Vec::new();
    ext.extend_from_slice(&[0x00, 0x00]);

    let entry_len = 3 + name.len();
    let list_len = entry_len;
    let ext_data_len = 2 + list_len;

    ext.push((ext_data_len >> 8) as u8);
    ext.push(ext_data_len as u8);
    ext.push((list_len >> 8) as u8);
    ext.push(list_len as u8);
    ext.push(0);
    ext.push((name.len() >> 8) as u8);
    ext.push(name.len() as u8);
    ext.extend_from_slice(name);
    ext
}

fn build_client_key_exchange(public_key: &[u8; 65]) -> Vec<u8> {
    build_client_key_exchange_raw(public_key)
}

fn build_client_key_exchange_raw(public_key: &[u8]) -> Vec<u8> {
    let mut msg = Vec::new();
    msg.push(public_key.len() as u8);
    msg.extend_from_slice(public_key);

    let mut hs = Vec::new();
    hs.push(HS_CLIENT_KEY_EXCHANGE);
    let len = msg.len() as u32;
    hs.push((len >> 16) as u8);
    hs.push((len >> 8) as u8);
    hs.push(len as u8);
    hs.extend_from_slice(&msg);
    hs
}

// ── Handshake message parsers ───────────────────────────────────────────────

fn find_extensions_in_server_hello(data: &[u8]) -> Option<usize> {
    // type(1) + len(3) + version(2) + random(32) + session_id_len(1) + session_id + cipher(2) + comp(1)
    if data.len() < 39 { return None; }
    let sid_len = data[38] as usize;
    let after_sid = 39 + sid_len;
    // cipher suite (2) + compression (1)
    let ext_start = after_sid + 3;
    if ext_start < data.len() { Some(ext_start) } else { None }
}

fn parse_server_hello(data: &[u8]) -> io::Result<[u8; 32]> {
    // type(1) + length(3) + version(2) + random(32)
    if data.len() < 38 || data[0] != HS_SERVER_HELLO {
        return Err(io::Error::new(io::ErrorKind::InvalidData,
            format!("bad ServerHello: len={}, type={}", data.len(), data.get(0).copied().unwrap_or(0))));
    }
    let mut server_random = [0u8; 32];
    server_random.copy_from_slice(&data[6..38]);
    Ok(server_random)
}

fn parse_server_key_exchange(data: &[u8]) -> io::Result<(u16, Vec<u8>)> {
    if data.len() < 5 || data[0] != HS_SERVER_KEY_EXCHANGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad ServerKeyExchange"));
    }
    let body = &data[4..];
    // body[0] = curve_type (3 = named_curve)
    // body[1..3] = named_curve
    // body[3] = pubkey_len
    // body[4..4+pubkey_len] = pubkey
    if body.len() < 4 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "SKE too short"));
    }
    let named_curve = ((body[1] as u16) << 8) | body[2] as u16;
    let pubkey_len = body[3] as usize;
    if body.len() < 4 + pubkey_len {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "SKE pubkey truncated"));
    }
    Ok((named_curve, body[4..4 + pubkey_len].to_vec()))
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn build_nonce(implicit: &[u8; 4], seq: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[..4].copy_from_slice(implicit);
    nonce[4..].copy_from_slice(&seq.to_be_bytes());
    nonce
}

fn build_aad(seq: u64, content_type: u8, length: usize) -> Vec<u8> {
    let mut aad = Vec::with_capacity(13);
    aad.extend_from_slice(&seq.to_be_bytes());
    aad.push(content_type);
    aad.extend_from_slice(&TLS_12);
    aad.push((length >> 8) as u8);
    aad.push(length as u8);
    aad
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    use std::io::Read;
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    }
    buf
}
