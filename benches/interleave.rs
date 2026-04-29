use std::hint::black_box;
use std::num::{NonZeroU32, NonZeroUsize};
use std::time::Duration;

use audio_samples::AudioSamples;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

const SAMPLE_RATE: u32 = 48_000;

fn fast_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_millis(200))
        .measurement_time(Duration::from_millis(800))
        .sample_size(30)
}

fn bench_planar_to_interleaved(c: &mut Criterion) {
    let mut group = c.benchmark_group("planar_to_interleaved");

    for &(frames, channels) in &[(512usize, 2usize), (1024, 2), (512, 6), (1024, 6)] {
        let samples = frames * channels;
        let channels_nz = NonZeroU32::new(channels as u32).unwrap();
        let sample_rate = NonZeroU32::new(SAMPLE_RATE).unwrap();
        let frames_nz = NonZeroUsize::new(frames).unwrap();
        let buffer = AudioSamples::<f32>::zeros_multi_channel(channels_nz, frames_nz, sample_rate);
        let label = format!("{frames}f_{channels}ch");

        // Current: outer=channel, inner=frame — sequential planar reads, strided output writes.
        group.bench_with_input(BenchmarkId::new("ch_major", &label), &(), |b, _| {
            let mut out = vec![0f32; samples];
            b.iter(|| {
                let arr = buffer.as_multi_channel().unwrap();
                for ch in 0..channels {
                    for frame in 0..frames {
                        out[frame * channels + ch] = arr[[ch, frame]];
                    }
                }
                black_box(out.as_mut_ptr());
            })
        });

        // Alternative: outer=frame, inner=channel — strided planar reads, sequential output writes.
        group.bench_with_input(BenchmarkId::new("frame_major", &label), &(), |b, _| {
            let mut out = vec![0f32; samples];
            b.iter(|| {
                let arr = buffer.as_multi_channel().unwrap();
                for frame in 0..frames {
                    for ch in 0..channels {
                        out[frame * channels + ch] = arr[[ch, frame]];
                    }
                }
                black_box(out.as_mut_ptr());
            })
        });
    }

    group.finish();
}

fn bench_interleaved_to_planar(c: &mut Criterion) {
    let mut group = c.benchmark_group("interleaved_to_planar");

    for &(frames, channels) in &[(512usize, 2usize), (1024, 2), (512, 6), (1024, 6)] {
        let samples = frames * channels;
        let channels_nz = NonZeroU32::new(channels as u32).unwrap();
        let sample_rate = NonZeroU32::new(SAMPLE_RATE).unwrap();
        let frames_nz = NonZeroUsize::new(frames).unwrap();
        let input: Vec<f32> = (0..samples).map(|i| i as f32 / samples as f32).collect();
        let label = format!("{frames}f_{channels}ch");

        // Old: alloc new Vec + AudioSamples every call.
        group.bench_with_input(BenchmarkId::new("alloc_from_vec", &label), &(), |b, _| {
            b.iter(|| {
                let nev = non_empty_slice::NonEmptyVec::try_from(input.clone()).unwrap();
                let audio =
                    AudioSamples::<f32>::from_interleaved_vec(nev, channels_nz, sample_rate)
                        .unwrap();
                black_box(audio)
            })
        });

        // New: deinterleave with idx%/div into pre-allocated flat planar slice.
        group.bench_with_input(BenchmarkId::new("inplace_indexed", &label), &(), |b, _| {
            let mut buf =
                AudioSamples::<f32>::zeros_multi_channel(channels_nz, frames_nz, sample_rate);
            b.iter(|| {
                let multi = buf.as_multi_channel_mut().unwrap();
                let flat = multi.as_slice_mut().unwrap();
                for (idx, &s) in input.iter().enumerate() {
                    let ch = idx % channels;
                    let frame = idx / channels;
                    flat[ch * frames + frame] = s;
                }
                black_box(flat.as_mut_ptr());
            })
        });

        // New: frame-major loop avoids modulo/divide — sequential input reads, strided planar writes.
        group.bench_with_input(
            BenchmarkId::new("inplace_frame_major", &label),
            &(),
            |b, _| {
                let mut buf =
                    AudioSamples::<f32>::zeros_multi_channel(channels_nz, frames_nz, sample_rate);
                b.iter(|| {
                    let multi = buf.as_multi_channel_mut().unwrap();
                    let flat = multi.as_slice_mut().unwrap();
                    for frame in 0..frames {
                        for ch in 0..channels {
                            flat[ch * frames + frame] = input[frame * channels + ch];
                        }
                    }
                    black_box(flat.as_mut_ptr());
                })
            },
        );
    }

    group.finish();
}

criterion_group! {
    name    = benches;
    config  = fast_criterion();
    targets = bench_planar_to_interleaved, bench_interleaved_to_planar
}
criterion_main!(benches);
