//! SIMD-based implementation of crc-32 checksums for x86 hardware.
//!
//! This module is based on Intel's paper, "Fast CRC Computation for Generic
//! Polynomials Using PCLMULQDQ Instruction". The code is quite analagous to the
//! paper itself and only largely differs in one area. More information in the
//! comments below!

#![allow(non_upper_case_globals)]

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;
#[cfg(target_arch = "x86")]
use std::arch::x86::*;

const K1: i64 = 0x154442bd4;
const K2: i64 = 0x1c6e41596;
const K3: i64 = 0x1751997d0;
const K4: i64 = 0x0ccaa009e;
const K5: i64 = 0x163cd6124;
const K6: i64 = 0x1db710640;

const P_x: i64 = 0x1DB710641;
const U_prime: i64 = 0x1F7011641;

pub fn detect() -> bool {
    is_x86_feature_detected!("pclmulqdq") &&
        is_x86_feature_detected!("sse2") &&
        is_x86_feature_detected!("sse4.1")
}

unsafe fn debug(s: &str, a: __m128i) -> __m128i {
    if false {
        union A { a: __m128i, b: [u8; 16] }
        let x = A { a }.b;
        print!(" {:20} | ", s);
        for x in x.iter() {
            print!("{:02x} ", x);
        }
        println!();
    }
    return a
}

#[target_feature(enable = "pclmulqdq", enable = "sse2", enable = "sse4.1")]
pub unsafe fn calculate(
    crc: u32,
    mut data: &[u8],
    fallback: fn(u32, &[u8]) -> u32,
) -> u32 {
    // In theory we can accelerate smaller chunks too, but for now just rely on
    // the fallback implementation as it's too much hassle and doesn't seem too
    // beneficial.
    if data.len() < 128 {
        return fallback(crc, data)
    }

    // Step 1: fold by 4 loop
    let mut x3 = get(&mut data);
    let mut x2 = get(&mut data);
    let mut x1 = get(&mut data);
    let mut x0 = get(&mut data);

    // fold in our initial value, part of the incremental crc checksum
    x3 = _mm_xor_si128(x3, _mm_cvtsi32_si128(!crc as i32));

    let k1k2 = _mm_set_epi64x(K2, K1);
    while data.len() >= 64 {
        x3 = reduce128(x3, get(&mut data), k1k2);
        x2 = reduce128(x2, get(&mut data), k1k2);
        x1 = reduce128(x1, get(&mut data), k1k2);
        x0 = reduce128(x0, get(&mut data), k1k2);
    }

    let k3k4 = _mm_set_epi64x(K4, K3);
    let mut x = reduce128(x3, x2, k3k4);
    x = reduce128(x, x1, k3k4);
    x = reduce128(x, x0, k3k4);

    // Step 2: fold by 1 loop
    while data.len() >= 16 {
        x = reduce128(x, get(&mut data), k3k4);
    }

    debug("128 > 64 init", x);

    // Perform step 3, reduction from 128 bits to 64 bits. This is
    // significantly different from the paper and basically doesn't follow it
    // at all. It's not really clear why, but implementations of this algorithm
    // in Chrome/Linux diverge in the same way. It is beyond me why this is
    // different than the paper, maybe the paper has like errata or something?
    // Unclear.
    //
    // It's also not clear to me what's actually happening here and/or why, but
    // algebraically what's happening is:
    //
    // x = (x[0:63] • K4) ^ x[64:127]           // 96 bit result
    // x = ((x[0:31] as u64) • K5) ^ x[32:95]   // 64 bit result
    //
    // It's... not clear to me what's going on here. The paper itself is pretty
    // vague on this part but definitely uses different constants at least.
    // It's not clear to me, reading the paper, where the xor operations are
    // happening or why things are shifting around. This implementation...
    // appears to work though!
    drop(K6);
    let x = _mm_xor_si128(
        _mm_clmulepi64_si128(x, k3k4, 0x10),
        _mm_srli_si128(x, 8),
    );
    let x = _mm_xor_si128(
        _mm_clmulepi64_si128(
            _mm_and_si128(x, _mm_set_epi32(0, 0, 0, !0)),
            _mm_set_epi64x(0, K5),
            0x00,
        ),
        _mm_srli_si128(x, 4),
    );
    debug("128 > 64 xx", x);

    // Perform a Barrett reduction from our now 64 bits to 32 bits. The
    // algorithm for this is described at the end of the paper, and note that
    // this also implements the "bit reflected input" variant.
    let pu = _mm_set_epi64x(U_prime, P_x);

    // T1(x) = ⌊(R(x) % x^32)⌋ • μ
    let t1 = _mm_clmulepi64_si128(
        _mm_and_si128(x, _mm_set_epi32(0, 0, 0, !0)),
        pu,
        0x10,
    );
    // T2(x) = ⌊(T1(x) % x^32)⌋ • P(x)
    let t2 = _mm_clmulepi64_si128(
        _mm_and_si128(t1, _mm_set_epi32(0, 0, 0, !0)),
        pu,
        0x00,
    );
    // We're doing the bit-reflected variant, so get the upper 32-bits of the
    // 64-bit result instead of the lower 32-bits.
    //
    // C(x) = R(x) ^ T2(x) / x^32
    let c = _mm_extract_epi32(_mm_xor_si128(x, t2), 1) as u32;

    if data.len() > 0 {
        fallback(!c, data)
    } else {
        !c
    }
}

unsafe fn reduce128(a: __m128i, b: __m128i, keys: __m128i) -> __m128i {
    let t1 = _mm_clmulepi64_si128(a, keys, 0x00);
    let t2 = _mm_clmulepi64_si128(a, keys, 0x11);
    _mm_xor_si128(_mm_xor_si128(b, t1), t2)
}

unsafe fn get(a: &mut &[u8]) -> __m128i {
    debug_assert!(a.len() >= 16);
    let r = _mm_loadu_si128(a.as_ptr() as *const __m128i);
    *a = &a[16..];
    return r
}
