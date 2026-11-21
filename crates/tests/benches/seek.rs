// What a partial read saves over a full decode, per seek path.
//
// Each `seek_*` case is paired with the `decode().field` that returns the same
// value, so the delta is exactly the payload the seek skipped. Message size is
// swept because that is the whole point: a seek's cost is bounded by where its
// target sits in the wire order, a decode's grows with everything after it.
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use pojoc_tests::pojoc_seek_bench::{self as sb, SeekBench, runtime::*};
use std::hint::black_box;
use std::time::Duration;

const LABELS: &[&str] = &[
    "starter_zone",
    "pvp_enabled",
    "trade_hub",
    "safe_zone",
    "raid_boss",
    "world_boss",
];

/// `n` scales the bulk fields only — the four seekable targets stay identical,
/// so any change in seek time across sizes is real work, not a bigger target.
fn make(n: usize) -> SeekBench<'static> {
    SeekBench {
        id: 0xDEAD_BEEF_1234_5678,
        kind: 42,
        name: "entity-name-of-moderate-length",
        label: LABELS[0],

        timestamp: 1_700_000_000,
        flags: 0b1011,
        score: 1234.5678,
        description: "a description long enough that skipping it is not free",
        tags: PojocVec::from_vec((0..n).map(|i| LABELS[i % LABELS.len()]).collect()),
        samples: PojocVec::from_vec((0..n).map(|i| i as f64 * 1.5).collect()),
        history: PojocVec::from_vec((0..n).map(|i| i as u64 * 7).collect()),
        points: PojocVec::from_vec(
            (0..n)
                .map(|i| (i as f32, i as f32 * 2.0, i as f32 * 3.0))
                .collect(),
        ),
        notes: PojocVec::from_vec((0..n).map(|i| LABELS[(i + 3) % LABELS.len()]).collect()),
    }
}

fn encoded(n: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    sb::encode(&mut buf, &make(n));
    buf
}

const SIZES: &[usize] = &[0, 16, 256, 4096];

/// one seekable target vs the full decode that would yield the same value
fn per_field(c: &mut Criterion) {
    // (group, seek fn, the same field off a full decode)
    type Case = (
        &'static str,
        fn(&[u8]) -> PojocResult<u64>,
        fn(&[u8]) -> u64,
    );

    // u64-returning cases share one signature; the string ones are separate
    // below since their return types differ
    let cases: &[Case] = &[(
        "blastable_u64",
        |b| sb::seek_id(b),
        |b| sb::decode(b).expect("decode failed").id,
    )];

    for (name, seek, dec) in cases {
        let mut g = c.benchmark_group(*name);
        g.warm_up_time(Duration::from_secs(1))
            .measurement_time(Duration::from_secs(3));
        for &n in SIZES {
            let buf = encoded(n);
            g.throughput(Throughput::Bytes(buf.len() as u64));
            g.bench_with_input(BenchmarkId::new("seek", buf.len()), &buf, |b, buf| {
                b.iter(|| black_box(seek(black_box(buf)).unwrap()))
            });
            g.bench_with_input(BenchmarkId::new("decode", buf.len()), &buf, |b, buf| {
                b.iter(|| black_box(dec(black_box(buf))))
            });
        }
        g.finish();
    }

    // hoisted variable-width target
    let mut g = c.benchmark_group("hoisted_string");
    g.warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    for &n in SIZES {
        let buf = encoded(n);
        g.throughput(Throughput::Bytes(buf.len() as u64));
        g.bench_with_input(BenchmarkId::new("seek", buf.len()), &buf, |b, buf| {
            b.iter(|| black_box(sb::seek_name(black_box(buf)).unwrap()))
        });
        g.bench_with_input(BenchmarkId::new("decode", buf.len()), &buf, |b, buf| {
            b.iter(|| black_box(sb::decode(black_box(buf)).unwrap().name))
        });
    }
    g.finish();

    // bare `intern string`: resolves one index instead of building the table
    let mut g = c.benchmark_group("interned_string");
    g.warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    for &n in SIZES {
        let buf = encoded(n);
        g.throughput(Throughput::Bytes(buf.len() as u64));
        g.bench_with_input(BenchmarkId::new("seek", buf.len()), &buf, |b, buf| {
            b.iter(|| black_box(sb::seek_label(black_box(buf)).unwrap()))
        });
        g.bench_with_input(BenchmarkId::new("decode", buf.len()), &buf, |b, buf| {
            b.iter(|| black_box(sb::decode(black_box(buf)).unwrap().label))
        });
    }
    g.finish();
}

/// pulling all four targets: one walk via seek_all, four independent walks via
/// the per-field accessors, or one full decode
fn batch(c: &mut Criterion) {
    let mut g = c.benchmark_group("all_four_fields");
    g.warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));

    for &n in SIZES {
        let buf = encoded(n);
        g.throughput(Throughput::Bytes(buf.len() as u64));

        g.bench_with_input(BenchmarkId::new("seek_all", buf.len()), &buf, |b, buf| {
            b.iter(|| {
                let s = sb::seek_all(black_box(buf)).unwrap();
                black_box((s.id, s.kind, s.name, s.label))
            })
        });
        g.bench_with_input(BenchmarkId::new("four_seeks", buf.len()), &buf, |b, buf| {
            b.iter(|| {
                let buf = black_box(buf);
                black_box((
                    sb::seek_id(buf).unwrap(),
                    sb::seek_kind(buf).unwrap(),
                    sb::seek_name(buf).unwrap(),
                    sb::seek_label(buf).unwrap(),
                ))
            })
        });
        g.bench_with_input(BenchmarkId::new("decode", buf.len()), &buf, |b, buf| {
            b.iter(|| {
                let v = sb::decode(black_box(buf)).unwrap();
                black_box((v.id, v.kind, v.name, v.label))
            })
        });
    }
    g.finish();
}

criterion_group!(benches, per_field, batch, many_targets);
criterion_main!(benches);

/// With many seekable fields, `seek_all` must walk to the *last* one before it
/// can return — so pulling a few early fields can cost more through the batch
/// reader than through individual accessors. This measures where that crossover
/// actually sits.
fn many_targets(c: &mut Criterion) {
    use pojoc_tests::pluck_seek_many as pluck;
    use pojoc_tests::pojoc_seek_many::{self as sm, SeekMany};

    let value = SeekMany {
        f0: 1,
        f1: 2,
        f2: 3,
        f3: 4,
        f4: 5,
        f5: 6,
        f6: 7.0,
        f7: 8.0,
        s0: "string-value-zero",
        s1: "string-value-one",
        s2: "string-value-two",
        s3: "string-value-three",
        s4: "string-value-four",
        s5: "string-value-five",
        s6: "string-value-six",
        s7: "string-value-seven",
        s8: "string-value-eight",
        s9: "string-value-nine",
        s10: "string-value-ten",
        s11: "string-value-eleven",
        blob: PojocVec::from_vec(vec![7u8; 4096]),
        samples: PojocVec::from_vec((0..4096).map(|i| i as f64).collect()),
        notes: PojocVec::from_vec(vec!["note"; 4096]),
    };
    let mut buf = Vec::new();
    sm::encode(&mut buf, &value);

    let mut g = c.benchmark_group("many_targets");
    g.warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));

    // a single early target: stops at s0, eleven strings still ahead of it
    g.bench_function("seek_one_early", |b| {
        b.iter(|| black_box(sm::seek_s0(black_box(&buf)).unwrap()))
    });
    // a single late target: walks all eleven preceding strings
    g.bench_function("seek_one_late", |b| {
        b.iter(|| black_box(sm::seek_s11(black_box(&buf)).unwrap()))
    });
    // three early targets, one prologue each
    g.bench_function("seek_three_early_individually", |b| {
        b.iter(|| {
            let buf = black_box(&buf);
            black_box((
                sm::seek_s0(buf).unwrap(),
                sm::seek_s1(buf).unwrap(),
                sm::seek_s2(buf).unwrap(),
            ))
        })
    });
    // the same three via the batch reader, which walks all twenty
    g.bench_function("seek_all_for_three_early", |b| {
        b.iter(|| {
            let s = sm::seek_all(black_box(&buf)).unwrap();
            black_box((s.s0, s.s1, s.s2))
        })
    });
    g.bench_function("seek_all", |b| {
        b.iter(|| black_box(sm::seek_all(black_box(&buf)).unwrap()))
    });
    // the ergonomic layer, to confirm it costs nothing over the raw mask call
    g.bench_function("pluck_three_early", |b| {
        b.iter(|| black_box(pluck!(sm, black_box(&buf), s0, s1, s2).unwrap()))
    });
    g.bench_function("pluck_one_early", |b| {
        b.iter(|| black_box(pluck!(sm, black_box(&buf), s0).unwrap()))
    });
    // fixed-width only: must never enter the tail
    g.bench_function("pluck_two_fixed", |b| {
        b.iter(|| black_box(pluck!(sm, black_box(&buf), f0, f7).unwrap()))
    });
    g.bench_function("pluck_all_twenty", |b| {
        b.iter(|| {
            black_box(
                pluck!(
                    sm,
                    black_box(&buf),
                    f0,
                    f1,
                    f2,
                    f3,
                    f4,
                    f5,
                    f6,
                    f7,
                    s0,
                    s1,
                    s2,
                    s3,
                    s4,
                    s5,
                    s6,
                    s7,
                    s8,
                    s9,
                    s10,
                    s11
                )
                .unwrap(),
            )
        })
    });
    g.bench_function("decode", |b| {
        b.iter(|| black_box(sm::decode(black_box(&buf)).unwrap().s0))
    });
    g.finish();
}
