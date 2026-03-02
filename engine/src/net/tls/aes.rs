/// AES-128 block cipher — FIPS 197, implemented from scratch.
/// Then AES-128-GCM (AEAD) on top.

// ── AES-128 Core ────────────────────────────────────────────────────────────

const SBOX: [u8; 256] = [
    0x63,0x7c,0x77,0x7b,0xf2,0x6b,0x6f,0xc5,0x30,0x01,0x67,0x2b,0xfe,0xd7,0xab,0x76,
    0xca,0x82,0xc9,0x7d,0xfa,0x59,0x47,0xf0,0xad,0xd4,0xa2,0xaf,0x9c,0xa4,0x72,0xc0,
    0xb7,0xfd,0x93,0x26,0x36,0x3f,0xf7,0xcc,0x34,0xa5,0xe5,0xf1,0x71,0xd8,0x31,0x15,
    0x04,0xc7,0x23,0xc3,0x18,0x96,0x05,0x9a,0x07,0x12,0x80,0xe2,0xeb,0x27,0xb2,0x75,
    0x09,0x83,0x2c,0x1a,0x1b,0x6e,0x5a,0xa0,0x52,0x3b,0xd6,0xb3,0x29,0xe3,0x2f,0x84,
    0x53,0xd1,0x00,0xed,0x20,0xfc,0xb1,0x5b,0x6a,0xcb,0xbe,0x39,0x4a,0x4c,0x58,0xcf,
    0xd0,0xef,0xaa,0xfb,0x43,0x4d,0x33,0x85,0x45,0xf9,0x02,0x7f,0x50,0x3c,0x9f,0xa8,
    0x51,0xa3,0x40,0x8f,0x92,0x9d,0x38,0xf5,0xbc,0xb6,0xda,0x21,0x10,0xff,0xf3,0xd2,
    0xcd,0x0c,0x13,0xec,0x5f,0x97,0x44,0x17,0xc4,0xa7,0x7e,0x3d,0x64,0x5d,0x19,0x73,
    0x60,0x81,0x4f,0xdc,0x22,0x2a,0x90,0x88,0x46,0xee,0xb8,0x14,0xde,0x5e,0x0b,0xdb,
    0xe0,0x32,0x3a,0x0a,0x49,0x06,0x24,0x5c,0xc2,0xd3,0xac,0x62,0x91,0x95,0xe4,0x79,
    0xe7,0xc8,0x37,0x6d,0x8d,0xd5,0x4e,0xa9,0x6c,0x56,0xf4,0xea,0x65,0x7a,0xae,0x08,
    0xba,0x78,0x25,0x2e,0x1c,0xa6,0xb4,0xc6,0xe8,0xdd,0x74,0x1f,0x4b,0xbd,0x8b,0x8a,
    0x70,0x3e,0xb5,0x66,0x48,0x03,0xf6,0x0e,0x61,0x35,0x57,0xb9,0x86,0xc1,0x1d,0x9e,
    0xe1,0xf8,0x98,0x11,0x69,0xd9,0x8e,0x94,0x9b,0x1e,0x87,0xe9,0xce,0x55,0x28,0xdf,
    0x8c,0xa1,0x89,0x0d,0xbf,0xe6,0x42,0x68,0x41,0x99,0x2d,0x0f,0xb0,0x54,0xbb,0x16,
];

const RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

fn gf_mul(mut a: u8, mut b: u8) -> u8 {
    let mut p: u8 = 0;
    for _ in 0..8 {
        if b & 1 != 0 { p ^= a; }
        let hi = a & 0x80;
        a <<= 1;
        if hi != 0 { a ^= 0x1b; }
        b >>= 1;
    }
    p
}

pub struct Aes128 {
    round_keys: [[u8; 16]; 11],
}

impl Aes128 {
    pub fn new(key: &[u8; 16]) -> Self {
        let mut rk = [[0u8; 16]; 11];
        rk[0] = *key;

        for i in 1..11 {
            let prev = rk[i - 1];
            // RotWord + SubWord + Rcon
            let mut temp = [
                SBOX[prev[13] as usize] ^ RCON[i - 1],
                SBOX[prev[14] as usize],
                SBOX[prev[15] as usize],
                SBOX[prev[12] as usize],
            ];

            for j in 0..4 {
                rk[i][j] = prev[j] ^ temp[j];
            }
            for j in 4..16 {
                rk[i][j] = prev[j] ^ rk[i][j - 4];
            }
        }

        Self { round_keys: rk }
    }

    pub fn encrypt_block(&self, block: &mut [u8; 16]) {
        // Initial round key
        xor_block(block, &self.round_keys[0]);

        // Rounds 1-9
        for r in 1..10 {
            sub_bytes(block);
            shift_rows(block);
            mix_columns(block);
            xor_block(block, &self.round_keys[r]);
        }

        // Final round (no MixColumns)
        sub_bytes(block);
        shift_rows(block);
        xor_block(block, &self.round_keys[10]);
    }
}

fn xor_block(a: &mut [u8; 16], b: &[u8; 16]) {
    for i in 0..16 { a[i] ^= b[i]; }
}

fn sub_bytes(state: &mut [u8; 16]) {
    for b in state.iter_mut() { *b = SBOX[*b as usize]; }
}

fn shift_rows(s: &mut [u8; 16]) {
    // Row 1: shift left 1
    let t = s[1];
    s[1] = s[5]; s[5] = s[9]; s[9] = s[13]; s[13] = t;
    // Row 2: shift left 2
    let (t0, t1) = (s[2], s[6]);
    s[2] = s[10]; s[6] = s[14]; s[10] = t0; s[14] = t1;
    // Row 3: shift left 3 (= shift right 1)
    let t = s[15];
    s[15] = s[11]; s[11] = s[7]; s[7] = s[3]; s[3] = t;
}

fn mix_columns(s: &mut [u8; 16]) {
    for c in 0..4 {
        let i = c * 4;
        let (a0, a1, a2, a3) = (s[i], s[i+1], s[i+2], s[i+3]);
        s[i]   = gf_mul(a0, 2) ^ gf_mul(a1, 3) ^ a2 ^ a3;
        s[i+1] = a0 ^ gf_mul(a1, 2) ^ gf_mul(a2, 3) ^ a3;
        s[i+2] = a0 ^ a1 ^ gf_mul(a2, 2) ^ gf_mul(a3, 3);
        s[i+3] = gf_mul(a0, 3) ^ a1 ^ a2 ^ gf_mul(a3, 2);
    }
}

// ── AES-128-GCM ─────────────────────────────────────────────────────────────

pub struct AesGcm {
    cipher: Aes128,
    h: [u8; 16], // GHASH subkey
}

impl AesGcm {
    pub fn new(key: &[u8; 16]) -> Self {
        let cipher = Aes128::new(key);
        let mut h = [0u8; 16];
        cipher.encrypt_block(&mut h);
        Self { cipher, h }
    }

    /// Encrypt plaintext with AES-128-GCM.
    /// Returns (ciphertext, tag).
    pub fn encrypt(&self, iv: &[u8; 12], aad: &[u8], plaintext: &[u8]) -> (Vec<u8>, [u8; 16]) {
        // J0 = IV || 0x00000001
        let mut j0 = [0u8; 16];
        j0[..12].copy_from_slice(iv);
        j0[15] = 1;

        // Encrypt counter blocks
        let ciphertext = self.ctr_encrypt(&j0, plaintext);

        // GHASH
        let tag = self.compute_tag(&j0, aad, &ciphertext);

        (ciphertext, tag)
    }

    /// Decrypt ciphertext with AES-128-GCM.
    /// Returns None if tag doesn't match.
    pub fn decrypt(&self, iv: &[u8; 12], aad: &[u8], ciphertext: &[u8], tag: &[u8; 16]) -> Option<Vec<u8>> {
        let mut j0 = [0u8; 16];
        j0[..12].copy_from_slice(iv);
        j0[15] = 1;

        // Verify tag
        let computed_tag = self.compute_tag(&j0, aad, ciphertext);
        if !constant_time_eq(&computed_tag, tag) {
            return None;
        }

        // Decrypt (CTR mode is symmetric)
        Some(self.ctr_encrypt(&j0, ciphertext))
    }

    fn ctr_encrypt(&self, j0: &[u8; 16], data: &[u8]) -> Vec<u8> {
        let mut result = Vec::with_capacity(data.len());
        let mut counter = *j0;
        let mut pos = 0;

        while pos < data.len() {
            // Increment counter
            inc32(&mut counter);
            let mut keystream = counter;
            self.cipher.encrypt_block(&mut keystream);

            let chunk_len = std::cmp::min(16, data.len() - pos);
            for i in 0..chunk_len {
                result.push(data[pos + i] ^ keystream[i]);
            }
            pos += chunk_len;
        }

        result
    }

    fn compute_tag(&self, j0: &[u8; 16], aad: &[u8], ciphertext: &[u8]) -> [u8; 16] {
        let mut ghash_state = [0u8; 16];

        // Process AAD
        ghash_update(&mut ghash_state, &self.h, aad);

        // Process ciphertext
        ghash_update(&mut ghash_state, &self.h, ciphertext);

        // Length block: len(AAD) || len(C) in bits, as u64 big-endian
        let mut len_block = [0u8; 16];
        let aad_bits = (aad.len() as u64) * 8;
        let ct_bits = (ciphertext.len() as u64) * 8;
        len_block[..8].copy_from_slice(&aad_bits.to_be_bytes());
        len_block[8..].copy_from_slice(&ct_bits.to_be_bytes());
        ghash_block(&mut ghash_state, &self.h, &len_block);

        // Encrypt J0 to get final tag
        let mut enc_j0 = *j0;
        self.cipher.encrypt_block(&mut enc_j0);
        for i in 0..16 {
            ghash_state[i] ^= enc_j0[i];
        }

        ghash_state
    }
}

fn inc32(counter: &mut [u8; 16]) {
    for i in (12..16).rev() {
        counter[i] = counter[i].wrapping_add(1);
        if counter[i] != 0 { break; }
    }
}

/// GHASH: GF(2^128) multiplication
fn gf128_mul(x: &[u8; 16], y: &[u8; 16]) -> [u8; 16] {
    let mut z = [0u8; 16];
    let mut v = *y;

    for i in 0..128 {
        if (x[i / 8] >> (7 - (i % 8))) & 1 == 1 {
            for j in 0..16 { z[j] ^= v[j]; }
        }
        let carry = v[15] & 1;
        // Right shift V by 1
        for j in (1..16).rev() {
            v[j] = (v[j] >> 1) | (v[j-1] << 7);
        }
        v[0] >>= 1;
        if carry == 1 {
            v[0] ^= 0xe1; // reduction polynomial
        }
    }
    z
}

fn ghash_block(state: &mut [u8; 16], h: &[u8; 16], block: &[u8; 16]) {
    for i in 0..16 { state[i] ^= block[i]; }
    *state = gf128_mul(state, h);
}

fn ghash_update(state: &mut [u8; 16], h: &[u8; 16], data: &[u8]) {
    let mut pos = 0;
    while pos < data.len() {
        let mut block = [0u8; 16];
        let chunk_len = std::cmp::min(16, data.len() - pos);
        block[..chunk_len].copy_from_slice(&data[pos..pos + chunk_len]);
        ghash_block(state, h, &block);
        pos += 16;
    }
}

fn constant_time_eq(a: &[u8; 16], b: &[u8; 16]) -> bool {
    let mut diff = 0u8;
    for i in 0..16 { diff |= a[i] ^ b[i]; }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes128_known_vector() {
        // NIST FIPS 197 Appendix B
        let key = [0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6,
                    0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f, 0x3c];
        let mut block = [0x32, 0x43, 0xf6, 0xa8, 0x88, 0x5a, 0x30, 0x8d,
                         0x31, 0x31, 0x98, 0xa2, 0xe0, 0x37, 0x07, 0x34];
        let expected = [0x39, 0x25, 0x84, 0x1d, 0x02, 0xdc, 0x09, 0xfb,
                        0xdc, 0x11, 0x85, 0x97, 0x19, 0x6a, 0x0b, 0x32];

        let aes = Aes128::new(&key);
        aes.encrypt_block(&mut block);
        assert_eq!(block, expected);
    }

    #[test]
    fn test_aes_gcm_roundtrip() {
        let key = [0u8; 16];
        let iv = [0u8; 12];
        let aad = b"additional data";
        let plaintext = b"hello world from tensor-engine!";

        let gcm = AesGcm::new(&key);
        let (ct, tag) = gcm.encrypt(&iv, aad, plaintext);
        let pt = gcm.decrypt(&iv, aad, &ct, &tag).unwrap();
        assert_eq!(&pt, plaintext);
    }
}
