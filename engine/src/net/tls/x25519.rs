/// X25519 — Curve25519 Diffie-Hellman key exchange.
/// Zero dependencies. Implements RFC 7748 scalar multiplication.
/// Field: GF(2^255 - 19), 5 limbs of 51 bits.

#[derive(Clone, Copy)]
struct Fe([u64; 5]);

impl Fe {
    fn zero() -> Self { Fe([0; 5]) }
    fn one() -> Self { Fe([1, 0, 0, 0, 0]) }

    fn from_bytes(s: &[u8; 32]) -> Self {
        // Load 32 little-endian bytes as a 256-bit integer,
        // extract 5 limbs of 51 bits.
        let load64_le = |b: &[u8]| -> u64 {
            let mut v = 0u64;
            for i in 0..b.len().min(8) {
                v |= (b[i] as u64) << (8 * i);
            }
            v
        };

        // Limb boundaries: bits 0-50, 51-101, 102-152, 153-203, 204-254
        // Byte boundaries: 0-50 = bytes 0..6 (byte 6 has bits 48-55, we need bits 48-50)
        let h0 = load64_le(&s[0..7]) & 0x7FFFFFFFFFFFF;
        // bits 51-101: starts at byte 6 bit 3
        let h1 = (load64_le(&s[6..14]) >> 3) & 0x7FFFFFFFFFFFF;
        // bits 102-152: starts at byte 12 bit 6
        let h2 = (load64_le(&s[12..20]) >> 6) & 0x7FFFFFFFFFFFF;
        // bits 153-203: starts at byte 19 bit 1
        let h3 = (load64_le(&s[19..27]) >> 1) & 0x7FFFFFFFFFFFF;
        // bits 204-254: starts at byte 25 bit 4
        let h4 = (load64_le(&s[25..32]) >> 4) & 0x7FFFFFFFFFFFF;

        Fe([h0, h1, h2, h3, h4])
    }

    fn to_bytes(&self) -> [u8; 32] {
        // Fully reduce, then pack 5 limbs of 51 bits into 32 LE bytes.
        let h = self.full_reduce();

        let mut out = [0u8; 32];
        let mut acc: u128 = 0;
        let mut acc_bits: u32 = 0;
        let mut pos = 0;
        for &limb in h.0.iter() {
            acc |= (limb as u128) << acc_bits;
            acc_bits += 51;
            while acc_bits >= 8 && pos < 32 {
                out[pos] = acc as u8;
                acc >>= 8;
                acc_bits -= 8;
                pos += 1;
            }
        }
        if pos < 32 {
            out[pos] = acc as u8;
        }
        out
    }

    fn carry(&self) -> Self {
        let mut h = self.0;
        for i in 0..4 {
            let carry = h[i] >> 51;
            h[i] &= 0x7FFFFFFFFFFFF;
            h[i + 1] += carry;
        }
        let carry = h[4] >> 51;
        h[4] &= 0x7FFFFFFFFFFFF;
        h[0] += carry * 19;
        Fe(h)
    }

    fn full_reduce(&self) -> Self {
        let mut h = self.carry().carry();

        // Compute h - p. If h >= p, use the result; otherwise keep h.
        // p = 2^255 - 19, in limbs: [0x7FFFFFFFFFFED, 0x7FFFFFFFFFFFF, ...]
        let mut t = h.0;
        // Add 19 — if result >= 2^255 then h >= p
        t[0] += 19;
        for i in 0..4 {
            let carry = t[i] >> 51;
            t[i] &= 0x7FFFFFFFFFFFF;
            t[i + 1] += carry;
        }
        // If t[4] >= 2^51, then h + 19 >= 2^255, so h >= p
        let carry = t[4] >> 51;
        t[4] &= 0x7FFFFFFFFFFFF;

        // carry is 0 or 1. If 1, use t (reduced). If 0, use h.
        let mask = carry.wrapping_sub(1); // 0 → 0xFFFF..., 1 → 0
        for i in 0..5 {
            h.0[i] = (h.0[i] & mask) | (t[i] & !mask);
        }
        h
    }

    fn add(&self, other: &Fe) -> Fe {
        let r = Fe([
            self.0[0] + other.0[0],
            self.0[1] + other.0[1],
            self.0[2] + other.0[2],
            self.0[3] + other.0[3],
            self.0[4] + other.0[4],
        ]);
        r.carry()
    }

    fn sub(&self, other: &Fe) -> Fe {
        // Add 2*p to ensure no underflow
        let r = Fe([
            self.0[0] + 0xFFFFFFFFFFFDA - other.0[0],
            self.0[1] + 0xFFFFFFFFFFFFE - other.0[1],
            self.0[2] + 0xFFFFFFFFFFFFE - other.0[2],
            self.0[3] + 0xFFFFFFFFFFFFE - other.0[3],
            self.0[4] + 0xFFFFFFFFFFFFE - other.0[4],
        ]);
        r.carry()
    }

    fn mul(&self, other: &Fe) -> Fe {
        let a = self.0;
        let b = other.0;

        // When indices wrap (i+j >= 5), multiply by 19 for reduction mod 2^255-19.
        let r = |x: u128| -> u128 { x * 19 };

        let t0 = a[0] as u128 * b[0] as u128
            + r(a[1] as u128 * b[4] as u128)
            + r(a[2] as u128 * b[3] as u128)
            + r(a[3] as u128 * b[2] as u128)
            + r(a[4] as u128 * b[1] as u128);

        let t1 = a[0] as u128 * b[1] as u128
            + a[1] as u128 * b[0] as u128
            + r(a[2] as u128 * b[4] as u128)
            + r(a[3] as u128 * b[3] as u128)
            + r(a[4] as u128 * b[2] as u128);

        let t2 = a[0] as u128 * b[2] as u128
            + a[1] as u128 * b[1] as u128
            + a[2] as u128 * b[0] as u128
            + r(a[3] as u128 * b[4] as u128)
            + r(a[4] as u128 * b[3] as u128);

        let t3 = a[0] as u128 * b[3] as u128
            + a[1] as u128 * b[2] as u128
            + a[2] as u128 * b[1] as u128
            + a[3] as u128 * b[0] as u128
            + r(a[4] as u128 * b[4] as u128);

        let t4 = a[0] as u128 * b[4] as u128
            + a[1] as u128 * b[3] as u128
            + a[2] as u128 * b[2] as u128
            + a[3] as u128 * b[1] as u128
            + a[4] as u128 * b[0] as u128;

        let mut h = [0u64; 5];
        let c = t0 >> 51;
        h[0] = t0 as u64 & 0x7FFFFFFFFFFFF;
        let t1 = t1 + c;
        let c = t1 >> 51;
        h[1] = t1 as u64 & 0x7FFFFFFFFFFFF;
        let t2 = t2 + c;
        let c = t2 >> 51;
        h[2] = t2 as u64 & 0x7FFFFFFFFFFFF;
        let t3 = t3 + c;
        let c = t3 >> 51;
        h[3] = t3 as u64 & 0x7FFFFFFFFFFFF;
        let t4 = t4 + c;
        let c = t4 >> 51;
        h[4] = t4 as u64 & 0x7FFFFFFFFFFFF;
        h[0] += c as u64 * 19;
        let c = h[0] >> 51;
        h[0] &= 0x7FFFFFFFFFFFF;
        h[1] += c;

        Fe(h)
    }

    fn sq(&self) -> Fe {
        self.mul(self)
    }

    fn sq_n(&self, n: u32) -> Fe {
        let mut r = *self;
        for _ in 0..n { r = r.sq(); }
        r
    }

    /// Invert: self^(p-2) mod p where p = 2^255 - 19
    fn invert(&self) -> Fe {
        // p - 2 = 2^255 - 21
        let z1 = *self;
        let z2 = z1.sq();
        let z8 = z2.sq_n(2);
        let z9 = z8.mul(&z1);
        let z11 = z9.mul(&z2);
        let z22 = z11.sq();
        let z_5_0 = z22.mul(&z9);     // z^(2^5 - 1) = z^31
        let z_10_5 = z_5_0.sq_n(5);
        let z_10_0 = z_10_5.mul(&z_5_0); // z^(2^10 - 1)
        let z_20_0 = z_10_0.sq_n(10).mul(&z_10_0);
        let z_40_0 = z_20_0.sq_n(20).mul(&z_20_0);
        let z_50_0 = z_40_0.sq_n(10).mul(&z_10_0);
        let z_100_0 = z_50_0.sq_n(50).mul(&z_50_0);
        let z_200_0 = z_100_0.sq_n(100).mul(&z_100_0);
        let z_250_0 = z_200_0.sq_n(50).mul(&z_50_0);
        // 2^255 - 21 = 2^255 - 2^5 + 2^4 - 2^1 + 2^0 - ... actually:
        // p - 2 = 2^255 - 21
        // 21 = 10101 in binary
        // So p-2 = 111...11101011 (255 bits, with specific low bits)
        // We need z^(2^255 - 21)
        // = z_250_0^(2^5) * z^(2^5 - 21 ... hmm
        // Let me use the standard addition chain:
        // z^(2^255 - 21) = z_250_0^(2^5) * z^11
        z_250_0.sq_n(5).mul(&z11)
    }
}

/// Conditional swap (constant-time)
fn cswap(a: &mut Fe, b: &mut Fe, swap: u64) {
    let mask = 0u64.wrapping_sub(swap);
    for i in 0..5 {
        let t = mask & (a.0[i] ^ b.0[i]);
        a.0[i] ^= t;
        b.0[i] ^= t;
    }
}

/// X25519 scalar multiplication (RFC 7748)
pub fn x25519(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
    x25519_inner(scalar, point, false)
}

fn x25519_inner(scalar: &[u8; 32], point: &[u8; 32], debug: bool) -> [u8; 32] {
    let mut k = *scalar;
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;

    // Mask high bit of u-coordinate per RFC 7748
    let mut u_bytes = *point;
    u_bytes[31] &= 127;
    let u = Fe::from_bytes(&u_bytes);

    // Montgomery ladder
    let x_1 = u;
    let mut x_2 = Fe::one();
    let mut z_2 = Fe::zero();
    let mut x_3 = u;
    let mut z_3 = Fe::one();
    let mut swap: u64 = 0;

    for t in (0..255).rev() {
        let k_t = ((k[t / 8] >> (t % 8)) & 1) as u64;

        swap ^= k_t;
        cswap(&mut x_2, &mut x_3, swap);
        cswap(&mut z_2, &mut z_3, swap);
        swap = k_t;

        let a = x_2.add(&z_2);
        let aa = a.sq();
        let b = x_2.sub(&z_2);
        let bb = b.sq();
        let e = aa.sub(&bb);
        let c = x_3.add(&z_3);
        let d = x_3.sub(&z_3);
        let da = d.mul(&a);
        let cb = c.mul(&b);
        x_3 = da.add(&cb).sq();
        z_3 = x_1.mul(&da.sub(&cb).sq());
        x_2 = aa.mul(&bb);
        let a24 = Fe([121665, 0, 0, 0, 0]);
        z_2 = e.mul(&aa.add(&a24.mul(&e)));

        if debug && t == 254 {
            // Print x_2/z_2 ratio = affine x_2
            let ratio = x_2.mul(&z_2.invert());
            eprintln!("  step t={}: k_t={} x_2/z_2 = {:02x?}", t, k_t, ratio.to_bytes());
            let ratio3 = x_3.mul(&z_3.invert());
            eprintln!("  step t={}: x_3/z_3 = {:02x?}", t, ratio3.to_bytes());
        }
    }

    cswap(&mut x_2, &mut x_3, swap);
    cswap(&mut z_2, &mut z_3, swap);

    x_2.mul(&z_2.invert()).to_bytes()
}

#[cfg(test)]
pub fn x25519_debug(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
    x25519_inner(scalar, point, true)
}

/// X25519 base point (9)
pub const BASEPOINT: [u8; 32] = {
    let mut b = [0u8; 32];
    b[0] = 9;
    b
};

pub struct X25519Keypair {
    pub private_key: [u8; 32],
    pub public_key: [u8; 32],
}

impl X25519Keypair {
    pub fn generate() -> Self {
        let private_key = super::random_bytes::<32>();
        let public_key = x25519(&private_key, &BASEPOINT);
        Self { private_key, public_key }
    }

    pub fn shared_secret(&self, peer_public: &[u8; 32]) -> [u8; 32] {
        x25519(&self.private_key, peer_public)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ladder_one_step() {
        // Manually compute one step of the Montgomery ladder
        // with k_254=1, u=9
        let u = Fe::from_bytes(&BASEPOINT);
        let one = Fe::one();
        let zero = Fe::zero();

        // After cswap(1): x_2=9, z_2=1, x_3=1, z_3=0
        let x2 = u;
        let z2 = one;
        let x3 = one;
        let z3 = zero;

        let a = x2.add(&z2);  // 10
        let aa = a.sq();       // 100
        let b = x2.sub(&z2);  // 8
        let bb = b.sq();       // 64
        let e = aa.sub(&bb);   // 36

        let c = x3.add(&z3);  // 1
        let d = x3.sub(&z3);  // 1
        let da = d.mul(&a);    // 10
        let cb = c.mul(&b);    // 8

        let da_plus_cb = da.add(&cb); // 18
        let da_minus_cb = da.sub(&cb); // 2

        // Check intermediate values
        let mut buf18 = [0u8; 32]; buf18[0] = 18;
        assert_eq!(da_plus_cb.to_bytes(), buf18, "DA+CB should be 18");

        let mut buf2 = [0u8; 32]; buf2[0] = 2;
        assert_eq!(da_minus_cb.to_bytes(), buf2, "DA-CB should be 2");

        let new_x3 = da_plus_cb.sq(); // 324
        let mut buf324 = [0u8; 32]; buf324[0] = (324u16 & 0xff) as u8; buf324[1] = (324u16 >> 8) as u8;
        assert_eq!(new_x3.to_bytes(), buf324, "x3 should be 324");

        let dmcb_sq = da_minus_cb.sq(); // 4
        let new_z3 = u.mul(&dmcb_sq); // 9*4=36
        let mut buf36 = [0u8; 32]; buf36[0] = 36;
        assert_eq!(new_z3.to_bytes(), buf36, "z3 should be 36");
    }

    #[test]
    fn test_x25519_iter1() {
        // RFC 7748 Section 5.2: After one iteration
        let k = BASEPOINT;
        let u = BASEPOINT;
        let result = x25519(&k, &u);
        let expected: [u8; 32] = [
            0x42, 0x2c, 0x8e, 0x7a, 0x62, 0x27, 0xd7, 0xbc,
            0xa1, 0x35, 0x0b, 0x3e, 0x2b, 0xb7, 0x27, 0x9f,
            0x78, 0x97, 0xb8, 0x7b, 0xb6, 0x85, 0x4b, 0x78,
            0x3c, 0x60, 0xe8, 0x03, 0x11, 0xae, 0x30, 0x79,
        ];
        assert_eq!(result, expected, "x25519 iter1 failed");
    }

    #[test]
    fn test_sq() {
        // 9^2 = 81
        let nine = Fe::from_bytes(&BASEPOINT);
        let eightyone = nine.sq();
        let mut expected = [0u8; 32];
        expected[0] = 81;
        assert_eq!(eightyone.to_bytes(), expected, "9^2 != 81");

        // (2^51-1)^2 mod p - test with larger values
        // Test: (a+b)^2 = a^2 + 2ab + b^2
        let a = Fe::from_bytes(&[3,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]);
        let b = Fe::from_bytes(&[5,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]);
        let ab = a.add(&b);
        let ab_sq = ab.sq();
        let a_sq = a.sq();
        let b_sq = b.sq();
        let two_ab = a.mul(&b).add(&a.mul(&b));
        let rhs = a_sq.add(&two_ab).add(&b_sq);
        assert_eq!(ab_sq.to_bytes(), rhs.to_bytes(), "(a+b)^2 != a^2 + 2ab + b^2");
    }

    #[test]
    fn test_mul_one() {
        // a * 1 = a
        let a = Fe::from_bytes(&[
            0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d,
            0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2, 0x66, 0x45,
            0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a,
            0xb1, 0x77, 0xfb, 0xa5, 0x1d, 0xb9, 0x2c, 0x2a,
        ]);
        let one = Fe::one();
        let result = a.mul(&one).to_bytes();
        let expected = a.to_bytes();
        assert_eq!(result, expected, "a*1 != a");
    }

    #[test]
    fn test_add_sub() {
        let a = Fe::from_bytes(&BASEPOINT);
        let b = Fe::from_bytes(&[
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]);
        let c = a.add(&b);
        let d = c.sub(&b);
        assert_eq!(d.to_bytes(), a.to_bytes(), "a+b-b != a");
    }

    #[test]
    fn test_invert() {
        // a * a^(-1) = 1
        let a = Fe::from_bytes(&BASEPOINT); // 9
        let a_inv = a.invert();
        let product = a.mul(&a_inv).to_bytes();
        let one_bytes = Fe::one().to_bytes();
        assert_eq!(product, one_bytes, "a * a^(-1) != 1");
    }

    #[test]
    fn test_roundtrip() {
        let input: [u8; 32] = [
            0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d,
            0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2, 0x66, 0x45,
            0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a,
            0xb1, 0x77, 0xfb, 0xa5, 0x1d, 0xb9, 0x2c, 0x2a,
        ];
        let fe = Fe::from_bytes(&input);
        let output = fe.to_bytes();
        assert_eq!(input, output, "roundtrip failed");
    }

    #[test]
    fn test_roundtrip_basepoint() {
        let output = Fe::from_bytes(&BASEPOINT).to_bytes();
        assert_eq!(BASEPOINT, output, "basepoint roundtrip failed");
    }

    #[test]
    fn test_rfc7748_vector1() {
        let alice_sk: [u8; 32] = [
            0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d,
            0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2, 0x66, 0x45,
            0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a,
            0xb1, 0x77, 0xfb, 0xa5, 0x1d, 0xb9, 0x2c, 0x2a,
        ];
        let alice_pk = x25519(&alice_sk, &BASEPOINT);
        let expected_pk: [u8; 32] = [
            0x85, 0x20, 0xf0, 0x09, 0x89, 0x30, 0xa7, 0x54,
            0x74, 0x8b, 0x7d, 0xdc, 0xb4, 0x3e, 0xf7, 0x5a,
            0x0d, 0xbf, 0x3a, 0x0d, 0x26, 0x38, 0x1a, 0xf4,
            0xeb, 0xa4, 0xa9, 0x8e, 0xaa, 0x9b, 0x4e, 0x6a,
        ];
        assert_eq!(alice_pk, expected_pk);
    }

    #[test]
    fn test_rfc7748_vector2() {
        let bob_sk: [u8; 32] = [
            0x5d, 0xab, 0x08, 0x7e, 0x62, 0x4a, 0x8a, 0x4b,
            0x79, 0xe1, 0x7f, 0x8b, 0x83, 0x80, 0x0e, 0xe6,
            0x6f, 0x3b, 0xb1, 0x29, 0x26, 0x18, 0xb6, 0xfd,
            0x1c, 0x2f, 0x8b, 0x27, 0xff, 0x88, 0xe0, 0xeb,
        ];
        let bob_pk = x25519(&bob_sk, &BASEPOINT);
        let expected_pk: [u8; 32] = [
            0xde, 0x9e, 0xdb, 0x7d, 0x7b, 0x7d, 0xc1, 0xb4,
            0xd3, 0x5b, 0x61, 0xc2, 0xec, 0xe4, 0x35, 0x37,
            0x3f, 0x83, 0x43, 0xc8, 0x5b, 0x78, 0x67, 0x4d,
            0xad, 0xfc, 0x7e, 0x14, 0x6f, 0x88, 0x2b, 0x4f,
        ];
        assert_eq!(bob_pk, expected_pk);
    }

    #[test]
    fn test_rfc7748_shared_secret() {
        let alice_sk: [u8; 32] = [
            0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d,
            0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2, 0x66, 0x45,
            0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a,
            0xb1, 0x77, 0xfb, 0xa5, 0x1d, 0xb9, 0x2c, 0x2a,
        ];
        let bob_pk: [u8; 32] = [
            0xde, 0x9e, 0xdb, 0x7d, 0x7b, 0x7d, 0xc1, 0xb4,
            0xd3, 0x5b, 0x61, 0xc2, 0xec, 0xe4, 0x35, 0x37,
            0x3f, 0x83, 0x43, 0xc8, 0x5b, 0x78, 0x67, 0x4d,
            0xad, 0xfc, 0x7e, 0x14, 0x6f, 0x88, 0x2b, 0x4f,
        ];
        let shared = x25519(&alice_sk, &bob_pk);
        let expected: [u8; 32] = [
            0x4a, 0x5d, 0x9d, 0x5b, 0xa4, 0xce, 0x2d, 0xe1,
            0x72, 0x8e, 0x3b, 0xf4, 0x80, 0x35, 0x0f, 0x25,
            0xe0, 0x7e, 0x21, 0xc9, 0x47, 0xd1, 0x9e, 0x33,
            0x76, 0xf0, 0x9b, 0x3c, 0x1e, 0x16, 0x17, 0x42,
        ];
        assert_eq!(shared, expected);
    }
}
