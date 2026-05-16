//! Shared low-level encoding/decoding primitives used by both the serde path
//! (`ser.rs`/`de.rs`) and the dynamic path (`dynamic.rs`).
//!
//! Every wire-format operation lives here so that the two paths cannot diverge.

use crate::result::{CordError, CordResult};
use crate::schema::Width;
use crate::varint::VarIntEncoding;
#[cfg(feature = "unicode")]
use unicode_normalization::UnicodeNormalization;

// ---------------------------------------------------------------------------
// Length prefix
// ---------------------------------------------------------------------------

/// Write a length value as a big-endian integer of the given `width`.
pub(crate) fn write_length<W: ?Sized + std::io::Write>(
    output: &mut W,
    len: usize,
    width: Width,
) -> CordResult<()> {
    match width {
        Width::W8 => {
            let v: u8 = len.try_into().map_err(|_| CordError::Overflow)?;
            output.write_all(&v.to_be_bytes())?;
        }
        Width::W16 => {
            let v: u16 = len.try_into().map_err(|_| CordError::Overflow)?;
            output.write_all(&v.to_be_bytes())?;
        }
        Width::W32 => {
            let v: u32 = len.try_into().map_err(|_| CordError::Overflow)?;
            output.write_all(&v.to_be_bytes())?;
        }
        Width::W64 => {
            let v: u64 = len.try_into().map_err(|_| CordError::Overflow)?;
            output.write_all(&v.to_be_bytes())?;
        }
    }
    Ok(())
}

/// Read a length prefix of the given `width` from `input`.
/// Returns an error if the decoded length exceeds `max_length`.
pub(crate) fn read_length(input: &mut &[u8], width: Width, max_length: usize) -> CordResult<usize> {
    let len = match width {
        Width::W8 => {
            let b = read_bytes(input, 1)?;
            b[0] as usize
        }
        Width::W16 => {
            let b = read_bytes(input, 2)?;
            u16::from_be_bytes(b.try_into().unwrap()) as usize
        }
        Width::W32 => {
            let b = read_bytes(input, 4)?;
            u32::from_be_bytes(b.try_into().unwrap()) as usize
        }
        Width::W64 => {
            let b = read_bytes(input, 8)?;
            let v = u64::from_be_bytes(b.try_into().unwrap());
            v.try_into().map_err(|_| CordError::Overflow)?
        }
    };
    if len > max_length {
        return Err(CordError::LengthLimitExceeded(len, max_length));
    }
    Ok(len)
}

// ---------------------------------------------------------------------------
// Variant index
// ---------------------------------------------------------------------------

/// Write a variant index as a big-endian integer of the given `width`.
pub(crate) fn write_variant_index<W: ?Sized + std::io::Write>(
    output: &mut W,
    idx: u32,
    width: Width,
) -> CordResult<()> {
    match width {
        Width::W8 => {
            let v: u8 = idx.try_into().map_err(|_| CordError::Overflow)?;
            output.write_all(&v.to_be_bytes())?;
        }
        Width::W16 => {
            let v: u16 = idx.try_into().map_err(|_| CordError::Overflow)?;
            output.write_all(&v.to_be_bytes())?;
        }
        Width::W32 => {
            output.write_all(&idx.to_be_bytes())?;
        }
        Width::W64 => {
            output.write_all(&(idx as u64).to_be_bytes())?;
        }
    }
    Ok(())
}

/// Read a variant index of the given `width` from `input`.
pub(crate) fn read_variant_index(input: &mut &[u8], width: Width) -> CordResult<u32> {
    match width {
        Width::W8 => {
            let b = read_bytes(input, 1)?;
            Ok(b[0] as u32)
        }
        Width::W16 => {
            let b = read_bytes(input, 2)?;
            Ok(u16::from_be_bytes(b.try_into().unwrap()) as u32)
        }
        Width::W32 => {
            let b = read_bytes(input, 4)?;
            Ok(u32::from_be_bytes(b.try_into().unwrap()))
        }
        Width::W64 => {
            let b = read_bytes(input, 8)?;
            let v = u64::from_be_bytes(b.try_into().unwrap());
            v.try_into().map_err(|_| CordError::Overflow)
        }
    }
}

// ---------------------------------------------------------------------------
// Raw byte reading
// ---------------------------------------------------------------------------

/// Read exactly `n` bytes from `input`, advancing the slice.
pub(crate) fn read_bytes<'a>(input: &mut &'a [u8], n: usize) -> CordResult<&'a [u8]> {
    if input.len() < n {
        return Err(CordError::UnexpectedEof);
    }
    let (head, tail) = input.split_at(n);
    *input = tail;
    Ok(head)
}

// ---------------------------------------------------------------------------
// Varint
// ---------------------------------------------------------------------------

/// Encode a varint value and append to `buf`.
pub(crate) fn write_varint<T: VarIntEncoding>(buf: &mut Vec<u8>, v: T) {
    // write_varint_to would return CordResult, but writing to Vec<u8> cannot fail.
    // Keep this as a direct impl to avoid unwrapping an infallible result.
    let mut tmp = [0u8; 19];
    let size = v.encode_var(&mut tmp);
    buf.extend_from_slice(&tmp[..size]);
}

/// Encode a varint value and write to a generic `Write` destination.
pub(crate) fn write_varint_to<T: VarIntEncoding, W: ?Sized + std::io::Write>(
    output: &mut W,
    v: T,
) -> CordResult<()> {
    let mut buf = [0u8; 19];
    let size = v.encode_var(&mut buf);
    output.write_all(&buf[..size])?;
    Ok(())
}

/// Decode a varint from `input`, advancing the slice.
pub(crate) fn read_varint<T: VarIntEncoding>(input: &mut &[u8]) -> CordResult<T> {
    let (value, size) = T::decode_var(input).ok_or(CordError::InvalidVarInt)?;
    if value.required_space() != size {
        return Err(CordError::InvalidVarInt);
    }
    *input = &input[size..];
    Ok(value)
}

// ---------------------------------------------------------------------------
// String encoding/decoding
// ---------------------------------------------------------------------------

/// NFC-normalize a string. Returns the original string (as owned) if it is
/// already ASCII or NFC, otherwise returns the normalized form.
///
/// When the `unicode` feature is disabled, returns the string as-is.
pub(crate) fn normalize_nfc(s: &str) -> std::borrow::Cow<'_, str> {
    #[cfg(feature = "unicode")]
    {
        if s.is_ascii() || unicode_normalization::is_nfc(s) {
            std::borrow::Cow::Borrowed(s)
        } else {
            std::borrow::Cow::Owned(s.nfc().collect())
        }
    }
    #[cfg(not(feature = "unicode"))]
    {
        std::borrow::Cow::Borrowed(s)
    }
}

/// Write a length-prefixed UTF-8 string to `buf`.
/// When the `unicode` feature is enabled, the string is NFC-normalized first.
pub(crate) fn write_str(buf: &mut Vec<u8>, s: &str, width: Width) -> CordResult<()> {
    let normalized = normalize_nfc(s);
    write_length(buf, normalized.len(), width)?;
    buf.extend_from_slice(normalized.as_bytes());
    Ok(())
}

/// Read a length-prefixed UTF-8 string from `input`.
/// When the `unicode` feature is enabled, validates NFC normalization.
pub(crate) fn read_str<'a>(
    input: &mut &'a [u8],
    width: Width,
    max_length: usize,
) -> CordResult<&'a str> {
    let len = read_length(input, width, max_length)?;
    let b = read_bytes(input, len)?;
    let s = std::str::from_utf8(b).map_err(|_| CordError::InvalidUtf8)?;
    #[cfg(feature = "unicode")]
    if !s.is_ascii() && !unicode_normalization::is_nfc(s) {
        return Err(CordError::NotNfcNormalized);
    }
    Ok(s)
}

// ---------------------------------------------------------------------------
// Bytes encoding/decoding
// ---------------------------------------------------------------------------

/// Write length-prefixed raw bytes to `buf`.
pub(crate) fn write_bytes(buf: &mut Vec<u8>, data: &[u8], width: Width) -> CordResult<()> {
    write_length(buf, data.len(), width)?;
    buf.extend_from_slice(data);
    Ok(())
}

/// Read length-prefixed raw bytes from `input`.
pub(crate) fn read_bytes_prefixed<'a>(
    input: &mut &'a [u8],
    width: Width,
    max_length: usize,
) -> CordResult<&'a [u8]> {
    let len = read_length(input, width, max_length)?;
    read_bytes(input, len)
}

// ---------------------------------------------------------------------------
// Boolean
// ---------------------------------------------------------------------------

/// Write a boolean as a single byte (0 or 1).
pub(crate) fn write_bool(buf: &mut Vec<u8>, v: bool) {
    buf.push(if v { 1 } else { 0 });
}

/// Read a boolean from `input` (single byte, must be 0 or 1).
pub(crate) fn read_bool(input: &mut &[u8]) -> CordResult<bool> {
    let b = read_bytes(input, 1)?[0];
    match b {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(CordError::InvalidBooleanValue),
    }
}

// ---------------------------------------------------------------------------
// Canonical sort + dedup for maps and sets
// ---------------------------------------------------------------------------

/// Sort map entries by serialized key bytes and reject duplicate keys.
/// Each entry is `(key_start, key_end, val_end)` — ranges into `buf`.
pub(crate) fn sort_and_dedup_map(
    buf: &[u8],
    entries: &mut [(usize, usize, usize)],
) -> CordResult<()> {
    entries.sort_by(|(s1, e1, _), (s2, e2, _)| buf[*s1..*e1].cmp(&buf[*s2..*e2]));
    for w in entries.windows(2) {
        if buf[w[0].0..w[0].1] == buf[w[1].0..w[1].1] {
            return Err(CordError::DuplicateMapKey);
        }
    }
    Ok(())
}

/// Sort set elements by serialized bytes and reject duplicates.
/// Each entry is `(start, end)` — a range into `buf`.
pub(crate) fn sort_and_dedup_set(buf: &[u8], ranges: &mut [(usize, usize)]) -> CordResult<()> {
    ranges.sort_by(|(s1, e1), (s2, e2)| buf[*s1..*e1].cmp(&buf[*s2..*e2]));
    for w in ranges.windows(2) {
        if buf[w[0].0..w[0].1] == buf[w[1].0..w[1].1] {
            return Err(CordError::DuplicateSetElement);
        }
    }
    Ok(())
}
