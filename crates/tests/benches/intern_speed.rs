// tags vs tags_plain isolates the cost of intern's hashmap lookup-or-insert
// on encode and table index on decode, vs writing/reading bytes directly
use criterion::{Criterion, criterion_group, criterion_main};
use pojoc_tests::pojoc_intern_bench::{self, InternBench, runtime::*};
use rkyv::{Archived, rancor::Error as ArchiveError};
use std::hint::black_box;
use std::sync::Arc;
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

fn plain_tags() -> Vec<String> {
    POOL.iter()
        .copied()
        .cycle()
        .take(ELEMENT_COUNT)
        .map(str::to_owned)
        .collect()
}

fn shared_tags(pool: &[Arc<str>]) -> Vec<Arc<str>> {
    pool.iter().cycle().take(ELEMENT_COUNT).cloned().collect()
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

    group.bench_function("rkyv_plain", |b| {
        b.iter(|| black_box(rkyv::to_bytes::<ArchiveError>(black_box(&plain_tags())).unwrap()))
    });
    let pool: Vec<Arc<str>> = POOL.iter().copied().map(Arc::from).collect();
    group.bench_function("rkyv_shared", |b| {
        b.iter(|| {
            black_box(rkyv::to_bytes::<ArchiveError>(black_box(&shared_tags(&pool))).unwrap())
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

    let plain = rkyv::to_bytes::<ArchiveError>(&plain_tags()).unwrap();
    group.bench_function("rkyv_plain_access", |b| {
        b.iter(|| {
            black_box(
                rkyv::access::<Archived<Vec<String>>, ArchiveError>(black_box(&plain)).unwrap(),
            )
        })
    });
    let pool: Vec<Arc<str>> = POOL.iter().copied().map(Arc::from).collect();
    let shared = rkyv::to_bytes::<ArchiveError>(&shared_tags(&pool)).unwrap();
    group.bench_function("rkyv_shared_access", |b| {
        b.iter(|| {
            black_box(
                rkyv::access::<Archived<Vec<Arc<str>>>, ArchiveError>(black_box(&shared)).unwrap(),
            )
        })
    });
    group.finish();
}

criterion_group!(benches, encode_benchmarks, decode_benchmarks);
criterion_main!(benches);
