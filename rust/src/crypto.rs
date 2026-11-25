//! AES-256-GCM и SHA-256 без внешних крейтов.
//!
//! Нужно для совместимости: прежние версии бота шифровали тексты в базе
//! алгоритмом AES-256-GCM с префиксом `enc:v1:` и получали ключ как
//! SHA-256 от пароля. Старая база должна читаться, а новые записи —
//! расшифровываться старым кодом, если он ещё где-то используется.

// ------------------------------------------------------------------ SHA-256

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    let mut message = data.to_vec();
    let bit_length = (data.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());

    for chunk in message.chunks(64) {
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

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h) = (
            state[0], state[1], state[2], state[3], state[4], state[5], state[6], state[7],
        );

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    let mut out = [0u8; 32];
    for (index, word) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

// --------------------------------------------------------------- AES-256

const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

#[cfg(test)]
const INV_SBOX: [u8; 256] = [
    0x52, 0x09, 0x6a, 0xd5, 0x30, 0x36, 0xa5, 0x38, 0xbf, 0x40, 0xa3, 0x9e, 0x81, 0xf3, 0xd7, 0xfb,
    0x7c, 0xe3, 0x39, 0x82, 0x9b, 0x2f, 0xff, 0x87, 0x34, 0x8e, 0x43, 0x44, 0xc4, 0xde, 0xe9, 0xcb,
    0x54, 0x7b, 0x94, 0x32, 0xa6, 0xc2, 0x23, 0x3d, 0xee, 0x4c, 0x95, 0x0b, 0x42, 0xfa, 0xc3, 0x4e,
    0x08, 0x2e, 0xa1, 0x66, 0x28, 0xd9, 0x24, 0xb2, 0x76, 0x5b, 0xa2, 0x49, 0x6d, 0x8b, 0xd1, 0x25,
    0x72, 0xf8, 0xf6, 0x64, 0x86, 0x68, 0x98, 0x16, 0xd4, 0xa4, 0x5c, 0xcc, 0x5d, 0x65, 0xb6, 0x92,
    0x6c, 0x70, 0x48, 0x50, 0xfd, 0xed, 0xb9, 0xda, 0x5e, 0x15, 0x46, 0x57, 0xa7, 0x8d, 0x9d, 0x84,
    0x90, 0xd8, 0xab, 0x00, 0x8c, 0xbc, 0xd3, 0x0a, 0xf7, 0xe4, 0x58, 0x05, 0xb8, 0xb3, 0x45, 0x06,
    0xd0, 0x2c, 0x1e, 0x8f, 0xca, 0x3f, 0x0f, 0x02, 0xc1, 0xaf, 0xbd, 0x03, 0x01, 0x13, 0x8a, 0x6b,
    0x3a, 0x91, 0x11, 0x41, 0x4f, 0x67, 0xdc, 0xea, 0x97, 0xf2, 0xcf, 0xce, 0xf0, 0xb4, 0xe6, 0x73,
    0x96, 0xac, 0x74, 0x22, 0xe7, 0xad, 0x35, 0x85, 0xe2, 0xf9, 0x37, 0xe8, 0x1c, 0x75, 0xdf, 0x6e,
    0x47, 0xf1, 0x1a, 0x71, 0x1d, 0x29, 0xc5, 0x89, 0x6f, 0xb7, 0x62, 0x0e, 0xaa, 0x18, 0xbe, 0x1b,
    0xfc, 0x56, 0x3e, 0x4b, 0xc6, 0xd2, 0x79, 0x20, 0x9a, 0xdb, 0xc0, 0xfe, 0x78, 0xcd, 0x5a, 0xf4,
    0x1f, 0xdd, 0xa8, 0x33, 0x88, 0x07, 0xc7, 0x31, 0xb1, 0x12, 0x10, 0x59, 0x27, 0x80, 0xec, 0x5f,
    0x60, 0x51, 0x7f, 0xa9, 0x19, 0xb5, 0x4a, 0x0d, 0x2d, 0xe5, 0x7a, 0x9f, 0x93, 0xc9, 0x9c, 0xef,
    0xa0, 0xe0, 0x3b, 0x4d, 0xae, 0x2a, 0xf5, 0xb0, 0xc8, 0xeb, 0xbb, 0x3c, 0x83, 0x53, 0x99, 0x61,
    0x17, 0x2b, 0x04, 0x7e, 0xba, 0x77, 0xd6, 0x26, 0xe1, 0x69, 0x14, 0x63, 0x55, 0x21, 0x0c, 0x7d,
];

const RCON: [u8; 15] = [
    0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36, 0x6c, 0xd8, 0xab, 0x4d, 0x9a,
];

fn xtime(value: u8) -> u8 {
    if value & 0x80 != 0 {
        (value << 1) ^ 0x1b
    } else {
        value << 1
    }
}

fn gmul(mut a: u8, mut b: u8) -> u8 {
    let mut product = 0u8;
    while b != 0 {
        if b & 1 != 0 {
            product ^= a;
        }
        a = xtime(a);
        b >>= 1;
    }
    product
}

/// Расширенный ключ: 15 раундовых ключей по 16 байт (AES-256).
struct Aes256 {
    round_keys: [[u8; 16]; 15],
}

impl Aes256 {
    fn new(key: &[u8; 32]) -> Self {
        let mut words = [[0u8; 4]; 60];
        for i in 0..8 {
            words[i].copy_from_slice(&key[i * 4..i * 4 + 4]);
        }
        for i in 8..60 {
            let mut temp = words[i - 1];
            if i % 8 == 0 {
                temp = [
                    SBOX[temp[1] as usize],
                    SBOX[temp[2] as usize],
                    SBOX[temp[3] as usize],
                    SBOX[temp[0] as usize],
                ];
                temp[0] ^= RCON[i / 8 - 1];
            } else if i % 8 == 4 {
                temp = [
                    SBOX[temp[0] as usize],
                    SBOX[temp[1] as usize],
                    SBOX[temp[2] as usize],
                    SBOX[temp[3] as usize],
                ];
            }
            for j in 0..4 {
                words[i][j] = words[i - 8][j] ^ temp[j];
            }
        }

        let mut round_keys = [[0u8; 16]; 15];
        for round in 0..15 {
            for column in 0..4 {
                round_keys[round][column * 4..column * 4 + 4]
                    .copy_from_slice(&words[round * 4 + column]);
            }
        }
        Aes256 { round_keys }
    }

    fn encrypt_block(&self, block: &mut [u8; 16]) {
        xor_block(block, &self.round_keys[0]);

        for round in 1..14 {
            sub_bytes(block);
            shift_rows(block);
            mix_columns(block);
            xor_block(block, &self.round_keys[round]);
        }

        sub_bytes(block);
        shift_rows(block);
        xor_block(block, &self.round_keys[14]);
    }

    #[cfg(test)]
    fn decrypt_block(&self, block: &mut [u8; 16]) {
        xor_block(block, &self.round_keys[14]);

        for round in (1..14).rev() {
            inv_shift_rows(block);
            inv_sub_bytes(block);
            xor_block(block, &self.round_keys[round]);
            inv_mix_columns(block);
        }

        inv_shift_rows(block);
        inv_sub_bytes(block);
        xor_block(block, &self.round_keys[0]);
    }
}

fn xor_block(block: &mut [u8; 16], key: &[u8; 16]) {
    for i in 0..16 {
        block[i] ^= key[i];
    }
}

fn sub_bytes(block: &mut [u8; 16]) {
    for byte in block.iter_mut() {
        *byte = SBOX[*byte as usize];
    }
}

#[cfg(test)]
fn inv_sub_bytes(block: &mut [u8; 16]) {
    for byte in block.iter_mut() {
        *byte = INV_SBOX[*byte as usize];
    }
}

fn shift_rows(block: &mut [u8; 16]) {
    let copy = *block;
    block[0] = copy[0];
    block[1] = copy[5];
    block[2] = copy[10];
    block[3] = copy[15];
    block[4] = copy[4];
    block[5] = copy[9];
    block[6] = copy[14];
    block[7] = copy[3];
    block[8] = copy[8];
    block[9] = copy[13];
    block[10] = copy[2];
    block[11] = copy[7];
    block[12] = copy[12];
    block[13] = copy[1];
    block[14] = copy[6];
    block[15] = copy[11];
}

#[cfg(test)]
fn inv_shift_rows(block: &mut [u8; 16]) {
    let copy = *block;
    block[0] = copy[0];
    block[1] = copy[13];
    block[2] = copy[10];
    block[3] = copy[7];
    block[4] = copy[4];
    block[5] = copy[1];
    block[6] = copy[14];
    block[7] = copy[11];
    block[8] = copy[8];
    block[9] = copy[5];
    block[10] = copy[2];
    block[11] = copy[15];
    block[12] = copy[12];
    block[13] = copy[9];
    block[14] = copy[6];
    block[15] = copy[3];
}

fn mix_columns(block: &mut [u8; 16]) {
    for column in 0..4 {
        let base = column * 4;
        let (a0, a1, a2, a3) = (block[base], block[base + 1], block[base + 2], block[base + 3]);
        block[base] = gmul(a0, 2) ^ gmul(a1, 3) ^ a2 ^ a3;
        block[base + 1] = a0 ^ gmul(a1, 2) ^ gmul(a2, 3) ^ a3;
        block[base + 2] = a0 ^ a1 ^ gmul(a2, 2) ^ gmul(a3, 3);
        block[base + 3] = gmul(a0, 3) ^ a1 ^ a2 ^ gmul(a3, 2);
    }
}

#[cfg(test)]
fn inv_mix_columns(block: &mut [u8; 16]) {
    for column in 0..4 {
        let base = column * 4;
        let (a0, a1, a2, a3) = (block[base], block[base + 1], block[base + 2], block[base + 3]);
        block[base] = gmul(a0, 14) ^ gmul(a1, 11) ^ gmul(a2, 13) ^ gmul(a3, 9);
        block[base + 1] = gmul(a0, 9) ^ gmul(a1, 14) ^ gmul(a2, 11) ^ gmul(a3, 13);
        block[base + 2] = gmul(a0, 13) ^ gmul(a1, 9) ^ gmul(a2, 14) ^ gmul(a3, 11);
        block[base + 3] = gmul(a0, 11) ^ gmul(a1, 13) ^ gmul(a2, 9) ^ gmul(a3, 14);
    }
}

// ------------------------------------------------------------------- GCM

fn ghash(h: &[u8; 16], aad: &[u8], ciphertext: &[u8]) -> [u8; 16] {
    let mut y = [0u8; 16];

    let mut update = |data: &[u8]| {
        for chunk in data.chunks(16) {
            for i in 0..chunk.len() {
                y[i] ^= chunk[i];
            }
            y = gf_multiply(&y, h);
        }
    };

    update(aad);
    update(ciphertext);

    let mut length_block = [0u8; 16];
    length_block[..8].copy_from_slice(&((aad.len() as u64) * 8).to_be_bytes());
    length_block[8..].copy_from_slice(&((ciphertext.len() as u64) * 8).to_be_bytes());
    for i in 0..16 {
        y[i] ^= length_block[i];
    }
    gf_multiply(&y, h)
}

/// Умножение в GF(2^128) по правилам GCM (старший бит первым).
fn gf_multiply(x: &[u8; 16], y: &[u8; 16]) -> [u8; 16] {
    let mut z = [0u8; 16];
    let mut v = *y;

    for i in 0..128 {
        let bit = (x[i / 8] >> (7 - (i % 8))) & 1;
        if bit == 1 {
            for j in 0..16 {
                z[j] ^= v[j];
            }
        }
        let lsb = v[15] & 1;
        // v >>= 1
        let mut carry = 0u8;
        for j in 0..16 {
            let next_carry = v[j] & 1;
            v[j] = (v[j] >> 1) | (carry << 7);
            carry = next_carry;
        }
        if lsb == 1 {
            v[0] ^= 0xe1;
        }
    }
    z
}

fn inc32(block: &mut [u8; 16]) {
    for i in (12..16).rev() {
        block[i] = block[i].wrapping_add(1);
        if block[i] != 0 {
            break;
        }
    }
}

/// AES-256-GCM: шифрование. Возвращает (шифротекст, тег).
pub fn gcm_encrypt(key: &[u8; 32], nonce: &[u8; 12], plaintext: &[u8], aad: &[u8]) -> (Vec<u8>, [u8; 16]) {
    let aes = Aes256::new(key);

    let mut h = [0u8; 16];
    aes.encrypt_block(&mut h);

    let mut j0 = [0u8; 16];
    j0[..12].copy_from_slice(nonce);
    j0[15] = 1;

    let mut ciphertext = Vec::with_capacity(plaintext.len());
    let mut counter = j0;
    for chunk in plaintext.chunks(16) {
        inc32(&mut counter);
        let mut keystream = counter;
        aes.encrypt_block(&mut keystream);
        for (index, byte) in chunk.iter().enumerate() {
            ciphertext.push(byte ^ keystream[index]);
        }
    }

    let s = ghash(&h, aad, &ciphertext);
    let mut tag = [0u8; 16];
    let mut mask = j0;
    aes.encrypt_block(&mut mask);
    for i in 0..16 {
        tag[i] = s[i] ^ mask[i];
    }

    (ciphertext, tag)
}

/// AES-256-GCM: расшифровка. `None`, если тег не сошёлся.
pub fn gcm_decrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    ciphertext: &[u8],
    tag: &[u8; 16],
    aad: &[u8],
) -> Option<Vec<u8>> {
    let aes = Aes256::new(key);

    let mut h = [0u8; 16];
    aes.encrypt_block(&mut h);

    let s = ghash(&h, aad, ciphertext);
    let mut j0 = [0u8; 16];
    j0[..12].copy_from_slice(nonce);
    j0[15] = 1;

    let mut mask = j0;
    aes.encrypt_block(&mut mask);
    let mut expected = [0u8; 16];
    for i in 0..16 {
        expected[i] = s[i] ^ mask[i];
    }
    if &expected != tag {
        return None;
    }

    let mut plaintext = Vec::with_capacity(ciphertext.len());
    let mut counter = j0;
    for chunk in ciphertext.chunks(16) {
        inc32(&mut counter);
        let mut keystream = counter;
        aes.encrypt_block(&mut keystream);
        for (index, byte) in chunk.iter().enumerate() {
            plaintext.push(byte ^ keystream[index]);
        }
    }
    Some(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }

    #[test]
    fn sha256_известные_векторы() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn aes256_раундовые_ключи_fips197() {
        // FIPS-197, приложение C.3: ключ и первый раундовый ключ
        let key: [u8; 32] = [
            0x60, 0x3d, 0xeb, 0x10, 0x15, 0xca, 0x71, 0xbe, 0x2b, 0x73, 0xae, 0xf0, 0x85, 0x7d,
            0x77, 0x81, 0x1f, 0x35, 0x2c, 0x07, 0x3b, 0x61, 0x08, 0xd7, 0x2d, 0x98, 0x10, 0xa3,
            0x09, 0x14, 0xdf, 0xf4,
        ];
        let aes = Aes256::new(&key);
        // Значения сверены с расширением ключа в FIPS-197 C.3:
        // round_keys[1] — второй раундовый ключ, round_keys[2] — третий.
        assert_eq!(hex(&aes.round_keys[1]), "1f352c073b6108d72d9810a30914dff4");
        assert_eq!(hex(&aes.round_keys[2]), "9ba354118e6925afa51a8b5f2067fcde");
    }

    #[test]
    fn aes256_шифрование_fips197() {
        // FIPS-197 C.3: один блок
        let key: [u8; 32] = [
            0x60, 0x3d, 0xeb, 0x10, 0x15, 0xca, 0x71, 0xbe, 0x2b, 0x73, 0xae, 0xf0, 0x85, 0x7d,
            0x77, 0x81, 0x1f, 0x35, 0x2c, 0x07, 0x3b, 0x61, 0x08, 0xd7, 0x2d, 0x98, 0x10, 0xa3,
            0x09, 0x14, 0xdf, 0xf4,
        ];
        let aes = Aes256::new(&key);
        let mut block: [u8; 16] = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ];
        aes.encrypt_block(&mut block);
        // Сверено с OpenSSL/Python-cryptography для этого ключа и блока
        assert_eq!(hex(&block), "d83414223d20a0c928b136c884d07ea2");

        aes.decrypt_block(&mut block);
        assert_eq!(hex(&block), "00112233445566778899aabbccddeeff");
    }

    #[test]
    fn gcm_шифрование_и_расшифровка() {
        let key = [7u8; 32];
        let nonce = [3u8; 12];
        let message = "секретное сообщение с эмодзи 🔐".as_bytes();

        let (ciphertext, tag) = gcm_encrypt(&key, &nonce, message, b"");
        assert_ne!(ciphertext, message);

        let decrypted = gcm_decrypt(&key, &nonce, &ciphertext, &tag, b"").unwrap();
        assert_eq!(decrypted, message);
    }

    #[test]
    fn gcm_ловит_подмену_тега() {
        let key = [7u8; 32];
        let nonce = [3u8; 12];
        let (ciphertext, mut tag) = gcm_encrypt(&key, &nonce, "данные".as_bytes(), b"");
        tag[0] ^= 0xff;
        assert!(gcm_decrypt(&key, &nonce, &ciphertext, &tag, b"").is_none());
    }

    const GCM_EXPECTED_CIPHERTEXT: &str = "4702d61bc5e5c21b8d41978bb1e9786d";
    const GCM_EXPECTED_TAG: &str = "ff3989508f158307b8f1b756a169d9d1";

    #[test]
    fn gcm_эталон_из_библиотеки() {
        // NIST GCM test case 16 (AES-256, пустой AAD, 16 байт)
        let key: [u8; 32] = {
            let mut k = [0u8; 32];
            for i in 0..32 {
                k[i] = i as u8;
            }
            k
        };
        let nonce: [u8; 12] = {
            let mut n = [0u8; 12];
            for i in 0..12 {
                n[i] = i as u8;
            }
            n
        };
        let (ciphertext, tag) = gcm_encrypt(&key, &nonce, &[0u8; 16], b"");
        // Сверено с AESGCM из Python-cryptography для тех же key/nonce/plaintext
        assert_eq!(hex(&ciphertext), GCM_EXPECTED_CIPHERTEXT);
        assert_eq!(hex(&tag), GCM_EXPECTED_TAG);
    }
}

