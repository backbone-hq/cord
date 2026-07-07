# ![Cord](./media/banner.svg)

![Build Status](https://img.shields.io/github/actions/workflow/status/backbone-hq/cord/ci.yml?branch=master)
![GitHub License](https://img.shields.io/github/license/backbone-hq/cord)
![crates.io](https://img.shields.io/crates/v/cord)
![Made by Backbone](https://img.shields.io/badge/made_by-Backbone-blue)

Cord is a compact deterministic serialization format for Rust with first-class [serde](https://serde.rs) integration.

- **Rich type system** — structs, enums, sets, maps, byte arrays, date-times, decimals, UUIDs, options, and more
- **Forward evolution** — wrap fields in `Evolving<T>` to round-trip unknown data (e.g., new enum variants) without data loss
- **Fine-grained wire control** — tune integer encoding, length prefix widths, and variant index sizes per field
- **Deterministic output** — every unique value produces exactly one byte sequence, making it safe to sign, hash, cache, and deduplicate serialized data

## Installation

```bash
cargo add cord
```

## Quick Start

Any type that derives `Cord` just works:

```rust
use cord::{serialize, deserialize, Cord};

#[derive(Cord, Debug, PartialEq)]
struct User {
    id: u32,
    name: String,
    active: bool,
}

let user = User {
    id: 42,
    name: "Alice".to_string(),
    active: true,
};

let bytes = serialize(&user).unwrap();
let deserialized: User = deserialize(&bytes).unwrap();
assert_eq!(user, deserialized);
```

`#[derive(Cord)]` generates both `Serialize` and `Deserialize` implementations. Types that already derive `serde::Serialize` and `serde::Deserialize` also work — `#[derive(Cord)]` is only needed when using Cord-specific field attributes.

Cord supports booleans, integers (i8–i128, u8–u128), floats (f32, f64), strings, byte arrays, options, sequences, structs, tuple structs, and enums out of the box.

## Beyond the Basics

Beyond primitive types and structs, Cord provides `DateTime`, `Map`, `Set`, `Decimal`, and `Uuid` — use them directly as field types, no annotations needed. Enums, options, and `Vec<u8>` all work out of the box.

```rust
use cord::{serialize, deserialize, Cord, DateTime, Decimal, Map, Set, Uuid};
use std::collections::{HashMap, HashSet};

#[derive(Cord, Debug, PartialEq)]
enum AccessLevel {
    Public,
    Restricted(Vec<String>),
}

#[derive(Cord, Debug, PartialEq)]
struct Document {
    title: String,
    access: AccessLevel,
    created: DateTime,             // Nanosecond-precision UTC timestamp
    tags: Set<String>,             // Serialized in sorted order
    attributes: Map<String, String>, // Serialized sorted by key
    description: Option<String>,
    id: Uuid,                      // 16-byte canonical UUID
    price: Decimal,                // Arbitrary-precision decimal
}

let mut tags = HashSet::new();
tags.insert("important".to_string());
tags.insert("draft".to_string());

let mut attributes = HashMap::new();
attributes.insert("priority".to_string(), "high".to_string());
attributes.insert("version".to_string(), "2.0".to_string());

let doc = Document {
    title: "Design Doc".to_string(),
    access: AccessLevel::Restricted(vec!["alice".into(), "bob".into()]),
    created: DateTime::now(),
    tags: Set::from(tags),
    attributes: Map::from(attributes),
    description: None,
    id: Uuid::from(uuid::Uuid::nil()),
    price: Decimal::from_i64(1999, 2), // 19.99
};

let bytes = serialize(&doc).unwrap();
let decoded: Document = deserialize(&bytes).unwrap();
assert_eq!(doc, decoded);
```

## Forward Evolution

When different parts of a system run different versions of the same schema, you need a way to handle unknown data without losing it. `Evolving<T>` length-prefixes the serialized payload so that if deserialization of the inner type fails (e.g., an unknown enum variant), the raw bytes are preserved and can be round-tripped without data loss:

```rust
use cord::{serialize, deserialize, Cord, Evolving};

#[derive(Cord, Debug, PartialEq)]
enum Status {
    Active,
    Inactive,
    // Future versions may add more variants
}

#[derive(Cord, Debug, PartialEq)]
struct Message {
    id: u32,
    status: Evolving<Status>,
}

let msg = Message {
    id: 1,
    status: Evolving::new(Status::Active),
};

let bytes = serialize(&msg).unwrap();
let decoded: Message = deserialize(&bytes).unwrap();

// Known values are accessible
assert!(decoded.status.is_known());
assert_eq!(decoded.status.known(), Some(&Status::Active));
```

If a newer version adds `Status::Pending` and serializes it, older code will deserialize it as `Evolving::Unknown(bytes)` — and re-serializing produces identical bytes.

The `#[cord(evolving = N)]` attribute controls the width of the length prefix used for the envelope:

| Attribute | Payload Length Prefix | Max Payload Size |
| ---------------------- | --------------------- | ---------------- |
| `#[cord(evolving = 8)]` | u8 | 255 bytes |
| `#[cord(evolving = 16)]` | u16 | 65,535 bytes |
| `#[cord(evolving = 32)]` | u32 (default) | ~4 GiB |

```rust
#[derive(Cord, Debug, PartialEq)]
struct CompactMessage {
    id: u32,
    #[cord(evolving = 8)]
    status: Evolving<Status>,  // 1-byte length prefix instead of 4
}
```

Without the attribute, `Evolving<T>` defaults to a 32-bit length prefix.

## Hashing

Since Cord guarantees deterministic serialization, you can compute canonical hashes of any serializable value with the built-in SHA3-256 hashing:

```bash
cargo add cord --features hash
```

```rust
use cord::{hash, Cord};

#[derive(Cord)]
struct User {
    name: String,
    age: u32,
}

let user = User { name: "Alice".into(), age: 30 };

// Compute a canonical SHA3-256 hash
let h: [u8; 32] = hash(&user).unwrap();

// Same value always produces the same hash, regardless of when or where
let h2: [u8; 32] = hash(&user).unwrap();
assert_eq!(h, h2);
```

Or bring your own hash — Cord's deterministic encoding means `serialize(value)` always produces the same bytes for the same value:

```rust
use cord::{serialize, Cord};

#[derive(Cord)]
struct User {
    name: String,
    age: u32,
}

let user = User { name: "Alice".into(), age: 30 };
let bytes = serialize(&user).unwrap();
// Hash bytes with any algorithm you prefer
```

## Tuning the Wire Format

By default, Cord uses fixed-width big-endian encoding for integers, 32-bit (u32) length prefixes for sequences/strings/bytes, and 32-bit (u32) variant indices for enums. This makes the format predictable and easy to implement across languages.

For size-sensitive protocols, Cord provides field attributes to control encoding width. These require `#[derive(Cord)]` on the containing type.

### Variable-Length Integers

Use `#[cord(varint)]` for compact variable-length encoding (LEB128 for unsigned, zigzag + LEB128 for signed). Works with all integer types from `u8` to `u128`:

```rust
use cord::Cord;

#[derive(Cord, Debug, PartialEq)]
struct Compact {
    #[cord(varint)]
    small_value: u32,       // 1 byte for values < 128
    large_value: u32,       // Always 4 bytes
    #[cord(varint)]
    big_id: u128,           // Variable-length 128-bit support
}
```

### Width

Control the width of length prefixes (strings, byte arrays, sequences) and variant indices (enums) with `#[cord(width = N)]`. The attribute applies to whichever is relevant for the field type:

| Attribute             | Wire Width | Applies To                                  |
| --------------------- | ---------- | ------------------------------------------- |
| `#[cord(width = 8)]`  | u8 (1B)   | Length prefix or variant index               |
| `#[cord(width = 16)]` | u16 (2B)  | Length prefix or variant index               |
| `#[cord(width = 64)]` | u64 (8B)  | Length prefix or variant index               |

### Custom Variant Indices

Use `#[cord(index = N)]` on enum variants to assign explicit wire indices:

```rust
use cord::Cord;

#[derive(Cord, Debug, PartialEq)]
enum Command {
    #[cord(index = 1)]
    Ping,
    #[cord(index = 5)]
    Pong(u32),
    #[cord(index = 100)]
    Reset,
}
```

If any variant has `#[cord(index)]`, all variants must have it.

### Combining Attributes

```rust
use cord::Cord;

#[derive(Cord, Debug, PartialEq)]
struct Packet {
    #[cord(width = 8)]
    kind: Status,           // 1-byte variant index instead of 4
    #[cord(width = 8)]
    name: String,           // 1-byte length prefix instead of 4
    #[cord(varint)]
    sequence: u64,          // Variable-length encoding
    fixed: u32,             // Standard 4-byte encoding
}
```

## Deterministic Serialization

Cord guarantees that every unique value has exactly one binary representation. This is a property of the format itself — sorted collections, NFC-normalized strings, fixed-width or minimal-length encodings — not something you opt into.

This matters most when serialized bytes are inputs to cryptographic operations. If you sign or hash a data structure and later need to re-serialize it to verify the signature, you need identical bytes. Most formats can't promise that — key order in maps, variable-length integer encodings, and Unicode normalization differences can all silently produce different output for the same logical value.

With Cord, any implementation that follows the spec will produce the same bytes for the same data. You can serialize, deserialize, re-serialize, and the output is always identical. This makes it straightforward to use with signing, hashing, content-addressing, caching, and deduplication.

## Threat Model

Cord is designed to defend against scenarios where attackers exploit ambiguities in data representation to bypass security controls, particularly in cryptographic contexts:

1. **Canonicalization bypass**: Cryptographic systems often verify signatures against a normalized form while operating on raw input. Attackers exploit this gap by crafting inputs with trailing data, comment fields, or flexible encodings that bypass verification but execute differently. Classic examples include XML signature wrapping attacks and JWT header manipulation.
2. **Protocol confusion**: When data is parsed differently across system boundaries, attackers can craft inputs that pass one subsystem's verifications and authorize malicious actions in downstream systems, effectively amounting to a payload substitution attack.
3. **Inconsistency**: When third parties cannot independently reproduce the exact byte sequence of cryptographically authenticated data, verification becomes dependent on trusting the original signer's environment. In distributed verification systems like blockchains or certificate transparency logs, this can lead to consensus failures or validation errors.

Cord does **not** protect against:

- Side-channel attacks during serialization/deserialization
- Memory safety issues outside of Cord's implementation
- Malicious inputs exceeding reasonable size limits
- Implementation flaws in cryptographic primitives used with Cord outputs

## Unicode Normalization

Cord enforces NFC (Canonical Decomposition followed by Canonical Composition) normalization for all strings. Strings are automatically normalized to NFC during serialization, and the deserializer rejects non-NFC strings. This prevents equivalent Unicode sequences (e.g., `e` as a single code point vs. `e` + combining acute accent) from producing different binary representations.

## Depth Limiting

The deserializer enforces a maximum nesting depth of 128 to protect against stack overflows from deeply nested or malicious input, returning `CordError::DepthLimitExceeded` if the limit is exceeded.

```rust
use cord::deserialize;

// Deeply nested options: Some(Some(Some(... None ...)))
// A 200-level nesting will be rejected at depth 128
let mut bytes = vec![0x01; 200]; // 200 layers of Some(...)
bytes.push(0x00);                // innermost None

let result: Result<_, _> = deserialize::<Option<Option<Option<u8>>>>(&bytes);
// Fails with CordError::DepthLimitExceeded
```

## Feature Flags

| Feature       | Default | Description                                      |
| ------------- | ------- | ------------------------------------------------ |
| `hash`        | off     | Adds `cord::hash()` (SHA3-256 hashing)            |

## Supported Types Reference

| Type                              | Support | Notes                                                              |
| --------------------------------- | ------- | ------------------------------------------------------------------ |
| Boolean                           | yes     |                                                                    |
| Integers (i8–i128, u8–u128)       | yes     | Fixed-width big-endian encoding (default)                          |
| Integers (varints)                | yes     | Opt-in variable-length encoding (LEB128/zigzag)                    |
| Floats (f32, f64)                 | yes     | Big-endian IEEE 754; NaN rejected, −0 canonicalized to +0          |
| Char                              | yes     | UTF-8, NFC-normalized, with length prefix                          |
| Strings                           | yes     | UTF-8, NFC-normalized, with length prefix (u32 default)            |
| Byte arrays                       | yes     | With length prefix (u32 default)                                   |
| Sequences                         | yes     | With length prefix (u32 default)                                   |
| Options                           | yes     |                                                                    |
| Struct/Tuple struct               | yes     |                                                                    |
| Enums                             | yes     | Variant index u32 default                                          |
| Evolving                          | yes     | Forward-compatible enum wrapper with length-prefixed payload       |
| Set                               | yes     | Sorted during serialization                                        |
| Map                               | yes     | Sorted by key during serialization                                 |
| DateTime                          | yes     | Nanosecond-precision UTC timestamp (seconds + nanos)               |
| Decimal                           | yes     | Arbitrary-precision decimal (u8 scale + two's complement unscaled) |
| Uuid                              | yes     | 16-byte canonical UUID                                             |

## Limitations and Trade-offs

1. **Not human-readable**: Binary output requires tooling to inspect
2. **Additive schema evolution**: Fields cannot be removed once added without breaking compatibility
3. **Wire format versioning**: The format may change between major versions (v1 and v2 are not wire-compatible)

## Performance

Cord v2 uses fixed-width big-endian encoding by default (16 bytes for 128-bit integers), which is fast to encode and decode. For size-sensitive applications, `#[cord(varint)]` and `#[cord(width = N)]` trade some speed for smaller output. Sets and Maps incur a sort during serialization.

## Migrating from v1

Cord v2 is a **breaking change** — the wire format is not compatible with v1. Data serialized with v1 cannot be deserialized with v2, and vice versa. If you have persisted v1 data, you will need to migrate it (deserialize with v1, re-serialize with v2).

## Current Status

Cord is a mature project that has seen production use in [Backbone](https://backbone.dev). Nevertheless, we urge users to:

- Thoroughly test before using in critical systems
- Be prepared for breaking changes in major versions
- Consider serialization format lock-in for long-term data storage

## Roadmap

Our current priorities are:

- Comprehensive fuzzing
- Language bindings (Python, JavaScript, ...)
- Configurable limits for nested structures
- Formal verification of components

Anything else you'd like to see? [Suggest a feature](https://github.com/backbone-hq/cord/issues)!

---

Built by [Backbone](https://backbone.dev)
