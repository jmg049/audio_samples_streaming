use std::hint::black_box;
use std::num::{NonZeroU32, NonZeroUsize};
use std::time::Duration;

use audio_samples::AudioSamples;
use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use non_empty_slice::NonEmptyVec;

fn fast_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_millis(200))
        .measurement_time(Duration::from_millis(800))
        .sample_size(30)
}

const SAMPLE_RATE: u32 = 48_000;

// Benchmark fill_chunk patterns across common chunk sizes and channel counts.
fn bench_fill_chunk(c: &mut Criterion) {
    let mut group = c.benchmark_group("fill_chunk");

    for &(frames, channels) in &[(512usize, 1usize), (512, 2), (1024, 2), (512, 6)] {
        let samples = frames * channels;
        let channels_nz = NonZeroU32::new(channels as u32).unwrap();
        let sample_rate = NonZeroU32::new(SAMPLE_RATE).unwrap();
        let frames_nz = NonZeroUsize::new(frames).unwrap();

        // Synthetic interleaved input, same as what the cpal callback writes.
        let input: Vec<f32> = (0..samples).map(|i| i as f32 / samples as f32).collect();

        let label = format!("{frames}f_{channels}ch");

        // Old: alloc Vec per call + full AudioSamples rebuild.
        group.bench_with_input(BenchmarkId::new("alloc_rebuild", &label), &(), |b, _| {
            b.iter(|| {
                let mut interleaved: Vec<f32> = Vec::with_capacity(samples);
                interleaved.extend_from_slice(&input);
                let nev = NonEmptyVec::try_from(interleaved).unwrap();
                let audio =
                    AudioSamples::<f32>::from_interleaved_vec(nev, channels_nz, sample_rate)
                        .unwrap();
                black_box(audio)
            })
        });

        // New: deinterleave in-place into pre-allocated buffer.
        group.bench_with_input(
            BenchmarkId::new("inplace_deinterleave", &label),
            &(),
            |b, _| {
                let mut buffer =
                    AudioSamples::<f32>::zeros_multi_channel(channels_nz, frames_nz, sample_rate);
                b.iter(|| {
                    let multi = buffer.as_multi_channel_mut().unwrap();
                    let flat = multi.as_slice_mut().unwrap();
                    for (idx, &sample) in input.iter().enumerate() {
                        let ch = idx % channels;
                        let frame = idx / channels;
                        flat[ch * frames + frame] = sample;
                    }
                    black_box(flat.as_mut_ptr());
                })
            },
        );
    }

    group.finish();
}

// Benchmark write_chunk patterns: per-sample push vs. bulk write_chunk_uninit.
fn bench_write_chunk(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_chunk");

    for &(frames, channels) in &[(512usize, 1usize), (512, 2), (1024, 2), (512, 6)] {
        let samples = frames * channels;
        let channels_nz = NonZeroU32::new(channels as u32).unwrap();
        let sample_rate = NonZeroU32::new(SAMPLE_RATE).unwrap();
        let frames_nz = NonZeroUsize::new(frames).unwrap();

        let buffer = AudioSamples::<f32>::zeros_multi_channel(channels_nz, frames_nz, sample_rate);
        let label = format!("{frames}f_{channels}ch");

        // Old: alloc Vec per call, push sample-by-sample.
        group.bench_with_input(BenchmarkId::new("alloc_push_one", &label), &(), |b, _| {
            b.iter_batched(
                || rtrb::RingBuffer::<f32>::new(samples * 2),
                |(mut prod, cons)| {
                    let arr = buffer.as_multi_channel().unwrap();
                    let mut interleaved: Vec<f32> = vec![0f32; samples];
                    for ch in 0..channels {
                        for frame in 0..frames {
                            interleaved[frame * channels + ch] = arr[[ch, frame]];
                        }
                    }
                    for &s in &interleaved {
                        prod.push(s).ok();
                    }
                    black_box(cons)
                },
                BatchSize::SmallInput,
            )
        });

        // New: reuse Vec field, single write_chunk_uninit call.
        group.bench_with_input(BenchmarkId::new("reuse_bulk_write", &label), &(), |b, _| {
            let mut interleaved = Vec::<f32>::with_capacity(samples);
            b.iter_batched(
                || rtrb::RingBuffer::<f32>::new(samples * 2),
                |(mut prod, cons)| {
                    let arr = buffer.as_multi_channel().unwrap();
                    interleaved.clear();
                    interleaved.resize(samples, 0.0);
                    for ch in 0..channels {
                        for frame in 0..frames {
                            interleaved[frame * channels + ch] = arr[[ch, frame]];
                        }
                    }
                    prod.write_chunk_uninit(samples)
                        .expect("ring buffer sized to hold full chunk")
                        .fill_from_iter(interleaved.iter().copied());
                    black_box(cons)
                },
                BatchSize::SmallInput,
            )
        });

        // Also benchmark the cpal-side: per-sample push vs. push_partial_slice.
        group.bench_with_input(
            BenchmarkId::new("cpal_push_one", &label),
            &input_slice(samples),
            |b, input| {
                b.iter_batched(
                    || rtrb::RingBuffer::<f32>::new(samples * 2),
                    |(mut prod, cons)| {
                        for &s in input.iter() {
                            prod.push(s).ok();
                        }
                        black_box(cons)
                    },
                    BatchSize::SmallInput,
                )
            },
        );

        group.bench_with_input(
            BenchmarkId::new("cpal_push_slice", &label),
            &input_slice(samples),
            |b, input| {
                b.iter_batched(
                    || rtrb::RingBuffer::<f32>::new(samples * 2),
                    |(mut prod, cons)| {
                        let _ = prod.push_partial_slice(input);
                        black_box(cons)
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

// Benchmark Consumer::read_chunk bulk vs. individual pop calls.
fn bench_read_chunk(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_chunk");

    for &(frames, channels) in &[(512usize, 2usize), (1024, 2), (512, 6)] {
        let samples = frames * channels;
        let label = format!("{frames}f_{channels}ch");

        // Old: individual pop calls.
        group.bench_with_input(BenchmarkId::new("pop_one", &label), &(), |b, _| {
            b.iter_batched(
                || {
                    let (mut prod, cons) = rtrb::RingBuffer::<f32>::new(samples * 2);
                    let _ = prod.push_partial_slice(&input_slice(samples));
                    (prod, cons)
                },
                |(_prod, mut cons)| {
                    let mut out = Vec::with_capacity(samples);
                    for _ in 0..samples {
                        out.push(cons.pop().unwrap_or(0.0));
                    }
                    black_box(out)
                },
                BatchSize::SmallInput,
            )
        });

        // New: read_chunk + copy_from_slice.
        group.bench_with_input(BenchmarkId::new("read_chunk_bulk", &label), &(), |b, _| {
            b.iter_batched(
                || {
                    let (mut prod, cons) = rtrb::RingBuffer::<f32>::new(samples * 2);
                    let _ = prod.push_partial_slice(&input_slice(samples));
                    (prod, cons)
                },
                |(_prod, mut cons)| {
                    let mut out = vec![0f32; samples];
                    let chunk = cons.read_chunk(samples).unwrap();
                    let (s1, s2) = chunk.as_slices();
                    let mid = s1.len();
                    out[..mid].copy_from_slice(s1);
                    out[mid..].copy_from_slice(s2);
                    chunk.commit_all();
                    black_box(out)
                },
                BatchSize::SmallInput,
            )
        });
    }

    group.finish();
}

fn input_slice(n: usize) -> Vec<f32> {
    (0..n).map(|i| i as f32 / n as f32).collect()
}

criterion_group! {
    name    = benches;
    config  = fast_criterion();
    targets = bench_fill_chunk, bench_write_chunk, bench_read_chunk
}
criterion_main!(benches);
