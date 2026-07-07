//! Variable-length integer encoding for all Cord integer types (u8–u128, i8–i128).
//!
//! Encoding scheme:
//! - **Unsigned**: LEB128 (7 data bits per byte, MSB = continuation bit)
//! - **Signed**: zigzag to unsigned, then LEB128
//!
//! This is wire-compatible with the `integer-encoding` crate for u8–u64 / i8–i64
//! and extends the same scheme to 128-bit integers.

pub trait VarIntEncoding: Sized + Copy {
    fn required_space(self) -> usize;
    fn decode_var(src: &[u8]) -> Option<(Self, usize)>;
    fn encode_var(self, dst: &mut [u8]) -> usize;

    #[cfg(test)]
    fn encode_var_vec(self) -> Vec<u8> {
        let mut v = vec![0u8; self.required_space()];
        self.encode_var(&mut v);
        v
    }
}

// ---------------------------------------------------------------------------
// Core: u128 unsigned LEB128
// ---------------------------------------------------------------------------

fn encode_unsigned(mut v: u128, dst: &mut [u8]) -> usize {
    let mut i = 0;
    while v >= 0x80 {
        dst[i] = (v as u8) | 0x80;
        v >>= 7;
        i += 1;
    }
    dst[i] = v as u8;
    i + 1
}

fn decode_unsigned(src: &[u8]) -> Option<(u128, usize)> {
    let mut result: u128 = 0;
    let mut shift: u32 = 0;
    for (i, &byte) in src.iter().enumerate() {
        if shift >= 128 {
            return None;
        }
        let low7 = (byte & 0x7F) as u128;
        if shift > 0 && low7 >= (1u128 << (128 - shift)) {
            return None;
        }
        result |= low7 << shift;
        if byte & 0x80 == 0 {
            let consumed = i + 1;
            // Reject non-minimal encodings: the number of bytes used must
            // equal the minimum required. This ensures each value has exactly
            // one valid wire representation, which is critical for a canonical
            // serialization format.
            if consumed != required_space_unsigned(result) {
                return None;
            }
            return Some((result, consumed));
        }
        shift += 7;
    }
    None
}

fn required_space_unsigned(v: u128) -> usize {
    if v == 0 {
        return 1;
    }
    let bits = 128 - v.leading_zeros() as usize;
    (bits + 6) / 7
}

// ---------------------------------------------------------------------------
// Zigzag helpers (128-bit)
// ---------------------------------------------------------------------------

fn zigzag_encode(v: i128) -> u128 {
    ((v << 1) ^ (v >> 127)) as u128
}

fn zigzag_decode(v: u128) -> i128 {
    ((v >> 1) as i128) ^ (-((v & 1) as i128))
}

// ---------------------------------------------------------------------------
// Impl: u128
// ---------------------------------------------------------------------------

impl VarIntEncoding for u128 {
    fn required_space(self) -> usize {
        required_space_unsigned(self)
    }
    fn decode_var(src: &[u8]) -> Option<(Self, usize)> {
        decode_unsigned(src)
    }
    fn encode_var(self, dst: &mut [u8]) -> usize {
        encode_unsigned(self, dst)
    }
}

// ---------------------------------------------------------------------------
// Impl: i128
// ---------------------------------------------------------------------------

impl VarIntEncoding for i128 {
    fn required_space(self) -> usize {
        required_space_unsigned(zigzag_encode(self))
    }
    fn decode_var(src: &[u8]) -> Option<(Self, usize)> {
        decode_unsigned(src).map(|(v, n)| (zigzag_decode(v), n))
    }
    fn encode_var(self, dst: &mut [u8]) -> usize {
        encode_unsigned(zigzag_encode(self), dst)
    }
}

// ---------------------------------------------------------------------------
// Impl: smaller unsigned types — cast through u128
// ---------------------------------------------------------------------------

macro_rules! impl_unsigned {
    ($($t:ty),*) => { $(
        impl VarIntEncoding for $t {
            fn required_space(self) -> usize {
                required_space_unsigned(self as u128)
            }
            fn decode_var(src: &[u8]) -> Option<(Self, usize)> {
                let (v, n) = u128::decode_var(src)?;
                let narrowed = <$t>::try_from(v).ok()?;
                Some((narrowed, n))
            }
            fn encode_var(self, dst: &mut [u8]) -> usize {
                (self as u128).encode_var(dst)
            }
        }
    )* };
}

impl_unsigned!(u8, u16, u32, u64);

// ---------------------------------------------------------------------------
// Impl: smaller signed types — cast through i128
// ---------------------------------------------------------------------------

macro_rules! impl_signed {
    ($($t:ty),*) => { $(
        impl VarIntEncoding for $t {
            fn required_space(self) -> usize {
                (self as i128).required_space()
            }
            fn decode_var(src: &[u8]) -> Option<(Self, usize)> {
                let (v, n) = i128::decode_var(src)?;
                let narrowed = <$t>::try_from(v).ok()?;
                Some((narrowed, n))
            }
            fn encode_var(self, dst: &mut [u8]) -> usize {
                (self as i128).encode_var(dst)
            }
        }
    )* };
}

impl_signed!(i8, i16, i32, i64);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsigned_roundtrip() {
        let values: &[u128] = &[0, 1, 127, 128, 255, 256, u64::MAX as u128, u128::MAX];
        for &v in values {
            let mut buf = [0u8; 19];
            let n = v.encode_var(&mut buf);
            let (decoded, consumed) = u128::decode_var(&buf[..n]).unwrap();
            assert_eq!(decoded, v);
            assert_eq!(consumed, n);
            assert_eq!(v.required_space(), n);
        }
    }

    #[test]
    fn signed_roundtrip() {
        let values: &[i128] = &[
            0,
            1,
            -1,
            127,
            -128,
            i64::MAX as i128,
            i64::MIN as i128,
            i128::MAX,
            i128::MIN,
        ];
        for &v in values {
            let mut buf = [0u8; 19];
            let n = v.encode_var(&mut buf);
            let (decoded, consumed) = i128::decode_var(&buf[..n]).unwrap();
            assert_eq!(decoded, v);
            assert_eq!(consumed, n);
            assert_eq!(v.required_space(), n);
        }
    }

    #[test]
    fn small_type_roundtrip() {
        // u8
        let v = 200u8;
        let enc = v.encode_var_vec();
        let (dec, _) = u8::decode_var(&enc).unwrap();
        assert_eq!(dec, v);

        // i8
        let v = -100i8;
        let enc = v.encode_var_vec();
        let (dec, _) = i8::decode_var(&enc).unwrap();
        assert_eq!(dec, v);

        // u32
        let v = 1_293_012u32;
        let enc = v.encode_var_vec();
        let (dec, _) = u32::decode_var(&enc).unwrap();
        assert_eq!(dec, v);

        // i64
        let v = -999_999i64;
        let enc = v.encode_var_vec();
        let (dec, _) = i64::decode_var(&enc).unwrap();
        assert_eq!(dec, v);
    }

    /// Verify wire-compatibility with the `integer-encoding` crate for u64/i64.
    /// The encoding of small values through u128 must produce identical bytes
    /// to encoding directly as u64/i64, since LEB128 only emits significant bytes.
    #[test]
    fn wire_compatible_with_u64_leb128() {
        // u64 values
        for v in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
            let ours = VarIntEncoding::encode_var_vec(v);
            let theirs = integer_encoding::VarInt::encode_var_vec(v);
            assert_eq!(ours, theirs, "u64 mismatch for {v}");
        }
        // i64 values
        for v in [0i64, 1, -1, 127, -128, i32::MAX as i64, i64::MIN] {
            let ours = VarIntEncoding::encode_var_vec(v);
            let theirs = integer_encoding::VarInt::encode_var_vec(v);
            assert_eq!(ours, theirs, "i64 mismatch for {v}");
        }
    }

    #[test]
    fn non_minimal_encoding_rejected() {
        // Value 1 encoded non-minimally as [0x81, 0x00] — must be rejected
        // because the canonical encoding is [0x01] (1 byte).
        assert!(u128::decode_var(&[0x81, 0x00]).is_none());

        // Value 0 encoded non-minimally as [0x80, 0x00] — must be rejected
        // because the canonical encoding is [0x00] (1 byte).
        assert!(u128::decode_var(&[0x80, 0x00]).is_none());

        // Value 127 encoded non-minimally as [0xFF, 0x00] — must be rejected
        // because the canonical encoding is [0x7F] (1 byte).
        assert!(u128::decode_var(&[0xFF, 0x00]).is_none());

        // Value 128 encoded minimally as [0x80, 0x01] — must succeed.
        let (val, size) = u128::decode_var(&[0x80, 0x01]).unwrap();
        assert_eq!(val, 128);
        assert_eq!(size, 2);

        // Non-minimal signed: zigzag(0) = 0, encoded as [0x80, 0x00] — rejected.
        assert!(i128::decode_var(&[0x80, 0x00]).is_none());
    }

    #[test]
    fn max_encoding_length() {
        let mut buf = [0u8; 19];
        let n = u128::MAX.encode_var(&mut buf);
        assert!(n <= 19);
    }

    #[test]
    fn decode_empty_returns_none() {
        assert!(u128::decode_var(&[]).is_none());
        assert!(i128::decode_var(&[]).is_none());
    }

    #[test]
    fn decode_unterminated_returns_none() {
        assert!(u128::decode_var(&[0x80, 0x80, 0x80]).is_none());
    }

    #[test]
    fn decode_overflow_u8_rejected() {
        // 384 encodes as [0x80, 0x03] in LEB128. Truncating to u8 gives 128,
        // which has the same required_space (2). Without the overflow check,
        // this would be silently accepted as 128u8.
        assert!(u8::decode_var(&[0x80, 0x03]).is_none());
        // But 128 itself (encoded as [0x80, 0x01]) must still work.
        let (v, n) = u8::decode_var(&[0x80, 0x01]).unwrap();
        assert_eq!(v, 128u8);
        assert_eq!(n, 2);
    }

    #[test]
    fn decode_overflow_u16_rejected() {
        // 65536 + 128 = 65664, LEB128 = [0x80, 0x80, 0x04].
        // Truncates to u16 = 128, required_space = 2 != 3, so minimality
        // would catch this one. But a tighter example: 65664 as u16 = 128.
        // Regardless, the overflow check should reject it outright.
        assert!(u16::decode_var(&[0x80, 0x80, 0x04]).is_none());
    }

    #[test]
    fn decode_overflow_i8_rejected() {
        // Zigzag encode of 128i128 = 256u128, LEB128 = [0x80, 0x02].
        // Truncating i128 128 to i8 gives -128, which is a different value.
        assert!(i8::decode_var(&[0x80, 0x02]).is_none());
        // But -1i8 (zigzag=1, LEB128=[0x01]) must still work.
        let (v, _) = i8::decode_var(&[0x01]).unwrap();
        assert_eq!(v, -1i8);
    }

    #[test]
    fn decode_overflow_u32_rejected() {
        // u32::MAX + 1 = 4294967296, must not decode as u32.
        let mut buf = [0u8; 19];
        let n = (u32::MAX as u128 + 1).encode_var(&mut buf);
        assert!(u32::decode_var(&buf[..n]).is_none());
    }

    #[test]
    fn decode_overflow_i32_rejected() {
        // i32::MAX + 1 as i128, must not decode as i32.
        let mut buf = [0u8; 19];
        let n = (i32::MAX as i128 + 1).encode_var(&mut buf);
        assert!(i32::decode_var(&buf[..n]).is_none());
        // i32::MIN - 1 as i128, must not decode as i32.
        let n = (i32::MIN as i128 - 1).encode_var(&mut buf);
        assert!(i32::decode_var(&buf[..n]).is_none());
    }
}
