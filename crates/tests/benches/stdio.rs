use criterion::{Bencher, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use pojoc_tests::pojoc_stdio_bench::{self as codec, StdioBench, runtime::PojocVec};
use std::hint::black_box;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const BATCH: usize = 64;
const SIZES: &[usize] = &[256, 4 * 1024, 64 * 1024];

struct Sidecar(Child);

impl Sidecar {
    fn start(binary: &Path) -> (Self, ChildStdin, ChildStdout) {
        let mut child = Command::new(binary)
            .args(["serve", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("could not launch the generated sidecar");
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        (Self(child), input, output)
    }

    fn wait(&mut self) {
        assert!(self.0.wait().expect("could not wait for sidecar").success());
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn frame_pair(bytes: usize) -> (Vec<u8>, Vec<u8>) {
    let value = StdioBench {
        id: 42,
        payload: PojocVec::from_vec((0..bytes).map(|i| i as u8).collect()),
        flags: 0,
    };
    let mut request = Vec::new();
    let mut reply = Vec::new();
    codec::encode_for_version(&mut request, &value, 1).unwrap();
    codec::encode(&mut reply, &value);
    (request, reply)
}

fn exchange(input: &mut ChildStdin, output: &mut ChildStdout, request: &[u8], reply: &mut [u8]) {
    input.write_all(request).expect("could not send request");
    output.read_exact(reply).expect("could not read reply");
}

fn one_process(binary: &Path, request: &[u8], reply: &mut [u8]) {
    let (mut child, mut input, mut output) = Sidecar::start(binary);
    input.write_all(request).expect("could not send request");
    drop(input);
    output.read_exact(reply).expect("could not read reply");
    assert_eq!(
        output.read(&mut [0]).unwrap(),
        0,
        "extra output after reply"
    );
    child.wait();
}

fn pipelined(b: &mut Bencher<'_>, binary: &Path, request: &[u8], reply: &[u8]) {
    let (mut child, mut input, mut output) = Sidecar::start(binary);
    let mut responses = vec![0; reply.len() * BATCH];
    let requests = request.repeat(BATCH);
    let expected = reply.repeat(BATCH);

    // Drain stdout concurrently so a batch can exceed the capacity of both pipes.
    thread::scope(|scope| {
        let (send, receive) = mpsc::channel::<usize>();
        let writer = scope.spawn(move || {
            while let Ok(n) = receive.recv() {
                input.write_all(&requests[..n * request.len()]).unwrap();
            }
        });
        let mut exchange_batch = |n| {
            send.send(n).unwrap();
            let bytes = n * reply.len();
            output.read_exact(&mut responses[..bytes]).unwrap();
            black_box(&responses[..bytes]);
        };
        exchange_batch(BATCH);
        assert_eq!(responses, expected);

        b.iter_custom(|messages| {
            let start = Instant::now();
            let mut remaining = messages;
            while remaining != 0 {
                let n = remaining.min(BATCH as u64);
                send.send(n as usize).unwrap();
                let bytes = n as usize * reply.len();
                output.read_exact(&mut responses[..bytes]).unwrap();
                black_box(&responses[..bytes]);
                remaining -= n;
            }
            start.elapsed()
        });
        drop(send);
        writer.join().unwrap();
    });
    child.wait();
}

fn stdio(c: &mut Criterion) {
    let override_path = std::env::var_os("POJOC_STDIO_BIN");
    let binary = override_path.as_ref().map_or_else(
        || {
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../../target/stdio-bench/stdio_bench{}",
                std::env::consts::EXE_SUFFIX
            ))
        },
        PathBuf::from,
    );
    if override_path.is_none() && !binary.is_file() {
        eprintln!("Skipping stdio: build schemas/stdio_bench.pojoc with --bin (see README).");
        return;
    }
    let binary = binary.canonicalize().expect("invalid POJOC_STDIO_BIN path");

    let mut group = c.benchmark_group("stdio");
    group
        .throughput(Throughput::Elements(1))
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .sample_size(25);

    for &bytes in SIZES {
        let (request, expected) = frame_pair(bytes);
        let mut reply = vec![0; expected.len()];
        // Compare bytes before timing. The v1 request must come back as a v2 envelope.
        one_process(&binary, &request, &mut reply);
        assert_eq!(reply, expected, "sidecar returned the wrong envelope");
        let bytes_on_wire = request.len();

        group.bench_function(BenchmarkId::new("codec_only", bytes_on_wire), |b| {
            let mut encoded = Vec::with_capacity(expected.len());
            b.iter(|| {
                let value = codec::decode(black_box(&request)).unwrap();
                encoded.clear();
                codec::encode_for_version(&mut encoded, &value, 2).unwrap();
                black_box(&encoded);
            });
        });
        if bytes == SIZES[0] {
            group.bench_function(BenchmarkId::new("spawn_per_message", bytes_on_wire), |b| {
                b.iter(|| one_process(&binary, &request, &mut reply));
            });
        }
        group.bench_function(BenchmarkId::new("round_trip", bytes_on_wire), |b| {
            let (mut child, mut input, mut output) = Sidecar::start(&binary);
            exchange(&mut input, &mut output, &request, &mut reply);
            b.iter(|| {
                exchange(&mut input, &mut output, &request, &mut reply);
                black_box(&reply);
            });
            drop(input);
            child.wait();
        });
        group.bench_function(BenchmarkId::new("pipelined_64", bytes_on_wire), |b| {
            pipelined(b, &binary, &request, &expected);
        });
    }
    group.finish();
}

criterion_group!(benches, stdio);
criterion_main!(benches);
