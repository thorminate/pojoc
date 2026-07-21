//! Throughput cost of `intern`'s lookup/dedup machinery: encoding an
//! interned field does a `HashMap` lookup-or-insert per element
//! (`InternBuilder::intern`) instead of writing the string's bytes
//! directly; decoding does a bounds-checked table index instead of reading
//! length-prefixed bytes. This isolates that cost by comparing `tags`
//! (interned) against `tags_plain` (identical data, no interning) on the
//! same schema — same element count, same string pool, only the intern
//! bookkeeping differs.

use criterion::{Criterion, criterion_group, criterion_main};
use pojoc_tests::pojoc_intern_bench::{self, InternBench, runtime::*};
use std::hint::black_box;
use std::time::Duration;

const POOL: &[&str] = &[
    "starter_zone",
    "pvp_enabled",
    "vip",
    "quest_giver",
    "faction_red",
    "faction_blue",
    "trade_hub",
    "safe_zone",
    "pve_only",
    "event_active",
    "guild_hall",
    "raid_boss",
    "dungeon_entrance",
    "world_boss",
    "crafting_station",
    "auction_house",
];
const ELEMENT_COUNT: usize = 5000;

fn pool_cycle() -> Vec<&'static str> {
    (0..ELEMENT_COUNT).map(|i| POOL[i % POOL.len()]).collect()
}

fn make_interned() -> InternBench<'static> {
    InternBench {
        tags: PojocVec::from_vec(pool_cycle()),
        tags_plain: PojocVec::new(),
    }
}

fn make_plain() -> InternBench<'static> {
    InternBench {
        tags: PojocVec::new(),
        tags_plain: PojocVec::from_vec(pool_cycle()),
    }
}

fn encode_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("intern_encode");
    group.sample_size(1000).warm_up_time(Duration::from_secs(3));
    group.throughput(criterion::Throughput::Elements(ELEMENT_COUNT as u64));

    group.bench_function("interned", |b| {
        b.iter(|| {
            let value = make_interned();
            let mut buf = Vec::new();
            pojoc_intern_bench::encode(&mut buf, black_box(&value));
            black_box(buf);
        })
    });

    group.bench_function("plain", |b| {
        b.iter(|| {
            let value = make_plain();
            let mut buf = Vec::new();
            pojoc_intern_bench::encode(&mut buf, black_box(&value));
            black_box(buf);
        })
    });

    group.finish();
}

fn decode_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("intern_decode");
    group.sample_size(1000).warm_up_time(Duration::from_secs(3));
    group.throughput(criterion::Throughput::Elements(ELEMENT_COUNT as u64));

    let interned_buf = {
        let mut buf = Vec::new();
        pojoc_intern_bench::encode(&mut buf, &make_interned());
        buf
    };
    group.bench_function("interned", |b| {
        b.iter(|| black_box(pojoc_intern_bench::decode(black_box(&interned_buf)).unwrap()))
    });

    let plain_buf = {
        let mut buf = Vec::new();
        pojoc_intern_bench::encode(&mut buf, &make_plain());
        buf
    };
    group.bench_function("plain", |b| {
        b.iter(|| black_box(pojoc_intern_bench::decode(black_box(&plain_buf)).unwrap()))
    });

    group.finish();
}

criterion_group!(benches, encode_benchmarks, decode_benchmarks);
criterion_main!(benches);
