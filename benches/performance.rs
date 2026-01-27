use cord::{deserialize, serialize, Map, Set};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct SmallStruct {
    id: u32,
    name: String,
    active: bool,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
enum AccessLevel {
    Public,
    Restricted(Vec<String>),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct LargeStruct {
    id: u32,
    tags: Set<String>,
    access: AccessLevel,
    metadata: Map<String, String>,
}

fn bench_primitives(c: &mut Criterion) {
    let mut group = c.benchmark_group("Primitives");
    let val_u64: u64 = 1234567890;
    let val_str = "This is a moderately long string for benchmarking purposes.";

    group.bench_function("cord_ser_u64", |b| {
        b.iter(|| serialize(black_box(&val_u64)))
    });
    group.bench_function("bincode_ser_u64", |b| {
        b.iter(|| bincode::serialize(black_box(&val_u64)))
    });

    group.bench_function("cord_ser_str", |b| {
        b.iter(|| serialize(black_box(&val_str)))
    });
    group.bench_function("bincode_ser_str", |b| {
        b.iter(|| bincode::serialize(black_box(&val_str)))
    });

    let cord_bytes = serialize(&val_str).unwrap();
    group.bench_function("cord_de_str", |b| {
        b.iter(|| deserialize::<String>(black_box(&cord_bytes)))
    });

    group.finish();
}

fn bench_collections(c: &mut Criterion) {
    let mut group = c.benchmark_group("Collections");

    let vec_data: Vec<u64> = (0..100).collect();
    group.bench_function("cord_ser_vec_100", |b| {
        b.iter(|| serialize(black_box(&vec_data)))
    });
    group.bench_function("bincode_ser_vec_100", |b| {
        b.iter(|| bincode::serialize(black_box(&vec_data)))
    });

    let mut set_inner = HashSet::new();
    for i in 0..50 {
        set_inner.insert(format!("tag_{}", i));
    }
    let set_data = Set::from(set_inner);
    group.bench_function("cord_ser_set_50", |b| {
        b.iter(|| serialize(black_box(&set_data)))
    });

    let mut map_inner = HashMap::new();
    for i in 0..50 {
        map_inner.insert(format!("key_{}", i), format!("value_{}", i));
    }
    let map_data = Map::from(map_inner);
    group.bench_function("cord_ser_map_50", |b| {
        b.iter(|| serialize(black_box(&map_data)))
    });

    group.finish();
}

fn bench_complex(c: &mut Criterion) {
    let mut group = c.benchmark_group("Complex");

    let small = SmallStruct {
        id: 42,
        name: "Alice".to_string(),
        active: true,
    };
    group.bench_function("cord_ser_small_struct", |b| {
        b.iter(|| serialize(black_box(&small)))
    });
    group.bench_function("bincode_ser_small_struct", |b| {
        b.iter(|| bincode::serialize(black_box(&small)))
    });

    let mut tags = HashSet::new();
    for i in 0..10 {
        tags.insert(format!("tag_{}", i));
    }

    let mut metadata = HashMap::new();
    for i in 0..10 {
        metadata.insert(format!("key_{}", i), format!("val_{}", i));
    }

    let large = LargeStruct {
        id: 1337,
        tags: Set::from(tags),
        access: AccessLevel::Restricted(vec!["admin".to_string(), "editor".to_string()]),
        metadata: Map::from(metadata),
    };

    group.bench_function("cord_ser_large_struct", |b| {
        b.iter(|| serialize(black_box(&large)))
    });

    let cord_bytes = serialize(&large).unwrap();
    group.bench_function("cord_de_large_struct", |b| {
        b.iter(|| deserialize::<LargeStruct>(black_box(&cord_bytes)))
    });

    group.finish();
}

criterion_group!(benches, bench_primitives, bench_collections, bench_complex);
criterion_main!(benches);
