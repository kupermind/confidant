//! XRPL address canonicalization for the screening gate. A listed party must not slip past the
//! exit screen by spelling their address differently: classic `r…` addresses and X-addresses
//! (`X…` mainnet, `T…` testnet, with or without a destination tag) all decode to the same 20-byte
//! AccountID, and the canonical form used for every comparison is the classic address with the tag
//! dropped. Base58check with the XRPL alphabet and a double-SHA-256 checksum, verified on decode.

use sha2::{Digest, Sha256};

const ALPHABET: &[u8; 58] = b"rpshnaf39wBUDNEGHJKLM4PQRST7VWXYZ2bcdeCg65jkm8oFqi1tuvAxyz";
const X_MAINNET: [u8; 2] = [0x05, 0x44];
const X_TESTNET: [u8; 2] = [0x04, 0x93];

fn b58_decode(s: &str) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    for c in s.bytes() {
        let mut carry = ALPHABET.iter().position(|&a| a == c)? as u32;
        for b in out.iter_mut() {
            carry += (*b as u32) * 58;
            *b = (carry & 0xff) as u8;
            carry >>= 8;
        }
        while carry > 0 {
            out.push((carry & 0xff) as u8);
            carry >>= 8;
        }
    }
    let zeros = s.bytes().take_while(|&c| c == ALPHABET[0]).count();
    out.extend(std::iter::repeat_n(0u8, zeros));
    out.reverse();
    Some(out)
}

fn b58_encode(bytes: &[u8]) -> String {
    let mut digits: Vec<u8> = Vec::new();
    for &b in bytes {
        let mut carry = b as u32;
        for d in digits.iter_mut() {
            carry += (*d as u32) << 8;
            *d = (carry % 58) as u8;
            carry /= 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let zeros = bytes.iter().take_while(|&&b| b == 0).count();
    let mut s = String::with_capacity(zeros + digits.len());
    s.extend(std::iter::repeat_n(ALPHABET[0] as char, zeros));
    s.extend(digits.iter().rev().map(|&d| ALPHABET[d as usize] as char));
    s
}

fn checksum(payload: &[u8]) -> [u8; 4] {
    let first = Sha256::digest(payload);
    let second = Sha256::digest(first);
    [second[0], second[1], second[2], second[3]]
}

/// The 20-byte AccountID behind a classic or X-address, if `addr` is a well-formed one.
pub fn account_id(addr: &str) -> Option<[u8; 20]> {
    let raw = b58_decode(addr)?;
    if raw.len() < 5 {
        return None;
    }
    let (payload, sum) = raw.split_at(raw.len() - 4);
    if checksum(payload) != sum {
        return None;
    }
    let id = match payload.len() {
        // classic: version 0x00 ‖ AccountID
        21 if payload[0] == 0 => &payload[1..21],
        // X-address: prefix ‖ AccountID ‖ tag flag ‖ 8-byte tag
        31 if (payload[..2] == X_MAINNET || payload[..2] == X_TESTNET) && payload[22] <= 1 => {
            &payload[2..22]
        }
        _ => return None,
    };
    let mut out = [0u8; 20];
    out.copy_from_slice(id);
    Some(out)
}

/// Canonical classic address (tag dropped) for any well-formed classic or X-address.
pub fn canonical(addr: &str) -> Option<String> {
    let id = account_id(addr)?;
    let mut payload = Vec::with_capacity(25);
    payload.push(0u8);
    payload.extend_from_slice(&id);
    let sum = checksum(&payload);
    payload.extend_from_slice(&sum);
    Some(b58_encode(&payload))
}
