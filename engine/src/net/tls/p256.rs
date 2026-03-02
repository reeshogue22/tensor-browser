/// P-256 (secp256r1) elliptic curve — ECDHE key exchange.
/// 256-bit modular arithmetic + point operations, from scratch.

/// 256-bit unsigned integer, stored as 4 x u64 limbs (little-endian).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct U256(pub [u64; 4]);

impl U256 {
    pub const ZERO: Self = Self([0; 4]);
    pub const ONE: Self = Self([1, 0, 0, 0]);

    pub fn from_be_bytes(b: &[u8; 32]) -> Self {
        Self([
            u64::from_be_bytes([b[24], b[25], b[26], b[27], b[28], b[29], b[30], b[31]]),
            u64::from_be_bytes([b[16], b[17], b[18], b[19], b[20], b[21], b[22], b[23]]),
            u64::from_be_bytes([b[8],  b[9],  b[10], b[11], b[12], b[13], b[14], b[15]]),
            u64::from_be_bytes([b[0],  b[1],  b[2],  b[3],  b[4],  b[5],  b[6],  b[7]]),
        ])
    }

    pub fn to_be_bytes(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        let b3 = self.0[3].to_be_bytes();
        let b2 = self.0[2].to_be_bytes();
        let b1 = self.0[1].to_be_bytes();
        let b0 = self.0[0].to_be_bytes();
        out[0..8].copy_from_slice(&b3);
        out[8..16].copy_from_slice(&b2);
        out[16..24].copy_from_slice(&b1);
        out[24..32].copy_from_slice(&b0);
        out
    }

    pub fn is_zero(&self) -> bool {
        self.0[0] == 0 && self.0[1] == 0 && self.0[2] == 0 && self.0[3] == 0
    }

    /// a + b, returns (result, carry)
    pub fn add(a: &Self, b: &Self) -> (Self, bool) {
        let mut r = [0u64; 4];
        let mut carry = false;
        for i in 0..4 {
            let (s1, c1) = a.0[i].overflowing_add(b.0[i]);
            let (s2, c2) = s1.overflowing_add(carry as u64);
            r[i] = s2;
            carry = c1 || c2;
        }
        (Self(r), carry)
    }

    /// a - b, returns (result, borrow)
    pub fn sub(a: &Self, b: &Self) -> (Self, bool) {
        let mut r = [0u64; 4];
        let mut borrow = false;
        for i in 0..4 {
            let (s1, b1) = a.0[i].overflowing_sub(b.0[i]);
            let (s2, b2) = s1.overflowing_sub(borrow as u64);
            r[i] = s2;
            borrow = b1 || b2;
        }
        (Self(r), borrow)
    }

    /// Compare: -1, 0, 1
    pub fn cmp(a: &Self, b: &Self) -> std::cmp::Ordering {
        for i in (0..4).rev() {
            if a.0[i] < b.0[i] { return std::cmp::Ordering::Less; }
            if a.0[i] > b.0[i] { return std::cmp::Ordering::Greater; }
        }
        std::cmp::Ordering::Equal
    }

    /// Full 512-bit multiply → stored as [u64; 8]
    pub fn mul_wide(a: &Self, b: &Self) -> [u64; 8] {
        let mut out = [0u64; 8];
        for i in 0..4 {
            let mut carry: u128 = 0;
            for j in 0..4 {
                let prod = (a.0[i] as u128) * (b.0[j] as u128) + (out[i + j] as u128) + carry;
                out[i + j] = prod as u64;
                carry = prod >> 64;
            }
            out[i + 4] = carry as u64;
        }
        out
    }
}

// ── P-256 curve parameters ──────────────────────────────────────────────────

/// p = 2^256 - 2^224 + 2^192 + 2^96 - 1
pub const P: U256 = U256([
    0xFFFFFFFFFFFFFFFF,
    0x00000000FFFFFFFF,
    0x0000000000000000,
    0xFFFFFFFF00000001,
]);

/// Order n of the generator point
pub const N: U256 = U256([
    0xF3B9CAC2FC632551,
    0xBCE6FAADA7179E84,
    0xFFFFFFFFFFFFFFFF,
    0xFFFFFFFF00000000,
]);

/// Generator point Gx
const GX: U256 = U256([
    0xF4A13945D898C296,
    0x77037D812DEB33A0,
    0xF8BCE6E563A440F2,
    0x6B17D1F2E12C4247,
]);

/// Generator point Gy
const GY: U256 = U256([
    0xCBB6406837BF51F5,
    0x2BCE33576B315ECE,
    0x8EE7EB4A7C0F9E16,
    0x4FE342E2FE1A7F9B,
]);

// ── Modular arithmetic mod p ────────────────────────────────────────────────

fn mod_add(a: &U256, b: &U256) -> U256 {
    let (sum, carry) = U256::add(a, b);
    if carry || U256::cmp(&sum, &P) != std::cmp::Ordering::Less {
        U256::sub(&sum, &P).0
    } else {
        sum
    }
}

fn mod_sub(a: &U256, b: &U256) -> U256 {
    let (diff, borrow) = U256::sub(a, b);
    if borrow {
        U256::add(&diff, &P).0
    } else {
        diff
    }
}

/// Modular reduction: 512-bit → 256-bit mod p.
/// Uses iterative reduction with R = 2^256 mod p.
/// Simple, correct, no tricky NIST formula.
fn mod_reduce(wide: &[u64; 8]) -> U256 {
    // R = 2^256 - p  (verified: p + R = 2^256)
    const R: U256 = U256([
        0x0000000000000001,
        0xFFFFFFFF00000000,
        0xFFFFFFFFFFFFFFFF,
        0x00000000FFFFFFFE,
    ]);

    let mut low = U256([wide[0], wide[1], wide[2], wide[3]]);
    let mut high = U256([wide[4], wide[5], wide[6], wide[7]]);

    // Iteratively reduce: value = high * 2^256 + low ≡ high * R + low (mod p)
    // Each iteration shrinks high by ~32 bits (since R < 2^224).
    // Converges in ~8 iterations for 256-bit high.
    while !high.is_zero() {
        let product = U256::mul_wide(&high, &R);
        let prod_low = U256([product[0], product[1], product[2], product[3]]);
        let prod_high = U256([product[4], product[5], product[6], product[7]]);

        let (sum, carry) = U256::add(&prod_low, &low);
        low = sum;
        high = prod_high;
        if carry {
            let (h2, _) = U256::add(&high, &U256::ONE);
            high = h2;
        }
    }

    // Final reduction: subtract p while >= p
    while U256::cmp(&low, &P) != std::cmp::Ordering::Less {
        low = U256::sub(&low, &P).0;
    }

    low
}

fn mod_mul(a: &U256, b: &U256) -> U256 {
    let wide = U256::mul_wide(a, b);
    mod_reduce(&wide)
}

fn mod_sqr(a: &U256) -> U256 {
    mod_mul(a, a)
}

/// Modular inverse using Fermat's little theorem: a^(p-2) mod p
fn mod_inv(a: &U256) -> U256 {
    // p - 2 = FFFFFFFF00000001 0000000000000000 00000000FFFFFFFF FFFFFFFFFFFFFFFD
    // Use square-and-multiply
    let mut result = U256::ONE;
    let exp = U256::sub(&P, &U256([2, 0, 0, 0])).0;

    let mut base = *a;
    for limb_idx in 0..4 {
        let mut limb = exp.0[limb_idx];
        for _ in 0..64 {
            if limb & 1 == 1 {
                result = mod_mul(&result, &base);
            }
            base = mod_sqr(&base);
            limb >>= 1;
        }
    }
    result
}

// ── Point operations (Jacobian coordinates) ─────────────────────────────────

/// Point in Jacobian coordinates: (X, Y, Z) where affine (x, y) = (X/Z², Y/Z³)
#[derive(Clone, Copy)]
pub struct Point {
    x: U256,
    y: U256,
    z: U256,
}

impl Point {
    pub fn identity() -> Self {
        Self { x: U256::ZERO, y: U256::ONE, z: U256::ZERO }
    }

    pub fn generator() -> Self {
        Self { x: GX, y: GY, z: U256::ONE }
    }

    pub fn from_affine(x: U256, y: U256) -> Self {
        Self { x, y, z: U256::ONE }
    }

    pub fn is_identity(&self) -> bool {
        self.z.is_zero()
    }

    /// Convert from Jacobian to affine coordinates
    pub fn to_affine(&self) -> (U256, U256) {
        if self.is_identity() {
            return (U256::ZERO, U256::ZERO);
        }
        let z_inv = mod_inv(&self.z);
        let z_inv2 = mod_sqr(&z_inv);
        let z_inv3 = mod_mul(&z_inv2, &z_inv);
        let x = mod_mul(&self.x, &z_inv2);
        let y = mod_mul(&self.y, &z_inv3);
        (x, y)
    }

    /// Encode uncompressed point: 0x04 || x || y
    pub fn to_uncompressed(&self) -> [u8; 65] {
        let (x, y) = self.to_affine();
        let mut out = [0u8; 65];
        out[0] = 0x04;
        out[1..33].copy_from_slice(&x.to_be_bytes());
        out[33..65].copy_from_slice(&y.to_be_bytes());
        out
    }

    /// Decode uncompressed point
    pub fn from_uncompressed(data: &[u8]) -> Option<Self> {
        if data.len() != 65 || data[0] != 0x04 { return None; }
        let mut xb = [0u8; 32];
        let mut yb = [0u8; 32];
        xb.copy_from_slice(&data[1..33]);
        yb.copy_from_slice(&data[33..65]);
        Some(Self::from_affine(U256::from_be_bytes(&xb), U256::from_be_bytes(&yb)))
    }

    /// Point doubling in Jacobian coords
    pub fn double(&self) -> Self {
        if self.is_identity() || self.y.is_zero() {
            return Self::identity();
        }

        let a = mod_sqr(&self.x);
        let b = mod_sqr(&self.y);
        let c = mod_sqr(&b);

        // d = 2*((x+b)^2 - a - c)
        let xb = mod_add(&self.x, &b);
        let xb2 = mod_sqr(&xb);
        let d = mod_sub(&mod_sub(&xb2, &a), &c);
        let d = mod_add(&d, &d);

        // e = 3*a  (note: for P-256, curve param a = -3, so 3*(X^2) + a*Z^4)
        // Actually for P-256: a = -3, so the formula is 3*X^2 + a*Z^4
        // But in Jacobian, if a = -3: M = 3*(X+Z^2)*(X-Z^2)
        let z2 = mod_sqr(&self.z);
        let xpz2 = mod_add(&self.x, &z2);
        let xmz2 = mod_sub(&self.x, &z2);
        let m = mod_mul(&xpz2, &xmz2);
        let e = mod_add(&mod_add(&m, &m), &m); // 3*m

        let f = mod_sqr(&e);

        // x3 = f - 2*d
        let x3 = mod_sub(&f, &mod_add(&d, &d));

        // y3 = e*(d - x3) - 8*c
        let y3 = mod_sub(&mod_mul(&e, &mod_sub(&d, &x3)),
                         &mod_add(&mod_add(&mod_add(&c, &c), &mod_add(&c, &c)),
                                  &mod_add(&mod_add(&c, &c), &mod_add(&c, &c))));

        // z3 = 2*y*z
        let z3 = mod_add(&mod_mul(&self.y, &self.z), &mod_mul(&self.y, &self.z));

        Self { x: x3, y: y3, z: z3 }
    }

    /// Point addition in Jacobian coords
    pub fn add(&self, other: &Self) -> Self {
        if self.is_identity() { return *other; }
        if other.is_identity() { return *self; }

        let z1z1 = mod_sqr(&self.z);
        let z2z2 = mod_sqr(&other.z);
        let u1 = mod_mul(&self.x, &z2z2);
        let u2 = mod_mul(&other.x, &z1z1);
        let s1 = mod_mul(&self.y, &mod_mul(&other.z, &z2z2));
        let s2 = mod_mul(&other.y, &mod_mul(&self.z, &z1z1));

        if u1 == u2 {
            if s1 == s2 {
                return self.double();
            } else {
                return Self::identity();
            }
        }

        let h = mod_sub(&u2, &u1);
        let h2 = mod_sqr(&h);
        let h3 = mod_mul(&h, &h2);
        let r = mod_sub(&s2, &s1);

        let x3 = mod_sub(&mod_sub(&mod_sqr(&r), &h3), &mod_add(&mod_mul(&u1, &h2), &mod_mul(&u1, &h2)));
        let y3 = mod_sub(&mod_mul(&r, &mod_sub(&mod_mul(&u1, &h2), &x3)), &mod_mul(&s1, &h3));
        let z3 = mod_mul(&mod_mul(&self.z, &other.z), &h);

        Self { x: x3, y: y3, z: z3 }
    }

    /// Scalar multiplication using double-and-add
    pub fn scalar_mul(&self, k: &U256) -> Self {
        let mut result = Self::identity();
        let mut base = *self;

        for limb_idx in 0..4 {
            let mut limb = k.0[limb_idx];
            for _ in 0..64 {
                if limb & 1 == 1 {
                    result = result.add(&base);
                }
                base = base.double();
                limb >>= 1;
            }
        }
        result
    }
}

// ── ECDHE ───────────────────────────────────────────────────────────────────

pub struct EcdhKeypair {
    pub private_key: U256,
    pub public_key: [u8; 65], // uncompressed point
}

impl EcdhKeypair {
    /// Generate a random keypair using /dev/urandom (or getrandom on Linux)
    pub fn generate() -> Self {
        let mut privkey_bytes = [0u8; 32];

        // Read from /dev/urandom — works on macOS + Linux
        use std::io::Read;
        let mut f = std::fs::File::open("/dev/urandom").expect("cannot open /dev/urandom");
        f.read_exact(&mut privkey_bytes).expect("cannot read random bytes");

        // Ensure private key is in [1, n-1]
        let mut private_key = U256::from_be_bytes(&privkey_bytes);
        // Simple reduction: if >= n, subtract n
        while U256::cmp(&private_key, &N) != std::cmp::Ordering::Less {
            private_key = U256::sub(&private_key, &N).0;
        }
        if private_key.is_zero() {
            private_key = U256::ONE;
        }

        let public_point = Point::generator().scalar_mul(&private_key);
        let public_key = public_point.to_uncompressed();

        Self { private_key, public_key }
    }

    /// Compute shared secret from peer's public key
    pub fn shared_secret(&self, peer_public: &[u8]) -> Option<[u8; 32]> {
        let peer_point = Point::from_uncompressed(peer_public)?;
        let shared = peer_point.scalar_mul(&self.private_key);
        let (x, _) = shared.to_affine();
        Some(x.to_be_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generator_on_curve() {
        let g = Point::generator();
        let (x, y) = g.to_affine();
        // y² = x³ + ax + b  (mod p)  where a = -3, b = ...
        let y2 = mod_sqr(&y);
        let x3 = mod_mul(&mod_sqr(&x), &x);
        let ax = mod_mul(&mod_sub(&P, &U256([3, 0, 0, 0])), &x); // a = -3
        let b = U256::from_be_bytes(&[
            0x5a, 0xc6, 0x35, 0xd8, 0xaa, 0x3a, 0x93, 0xe7,
            0xb3, 0xeb, 0xbd, 0x55, 0x76, 0x98, 0x86, 0xbc,
            0x65, 0x1d, 0x06, 0xb0, 0xcc, 0x53, 0xb0, 0xf6,
            0x3b, 0xce, 0x3c, 0x3e, 0x27, 0xd2, 0x60, 0x4b,
        ]);
        let rhs = mod_add(&mod_add(&x3, &ax), &b);
        assert_eq!(y2, rhs, "Generator point is not on curve!");
    }

    #[test]
    fn test_scalar_mul_identity() {
        let g = Point::generator();
        let ng = g.scalar_mul(&N); // n*G should be the identity
        assert!(ng.is_identity(), "n*G should be identity");
    }

    #[test]
    fn test_ecdh_roundtrip() {
        let alice = EcdhKeypair::generate();
        let bob = EcdhKeypair::generate();
        let secret_a = alice.shared_secret(&bob.public_key).unwrap();
        let secret_b = bob.shared_secret(&alice.public_key).unwrap();
        assert_eq!(secret_a, secret_b, "ECDH shared secrets must match");
    }
}
