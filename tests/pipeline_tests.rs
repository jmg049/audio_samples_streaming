//! Pipeline and file source/sink integration tests.
//!
//! The frame counts are deliberately *not* multiples of the chunk size: the
//! final partial chunk is where stale buffer contents would leak into the
//! output if the pipeline wrote whole buffers unconditionally.

#![cfg(all(feature = "wav", feature = "flac"))]

use std::num::{NonZeroU32, NonZeroUsize};
use std::path::PathBuf;

use audio_samples::AudioSamples;
use audio_samples_streaming::{AudioStream, FlacFileStream, WavFileSink, WavFileStream, pipeline};

const FRAMES: usize = 1000; // 1000 % 256 != 0 → final chunk is partial
const CHUNK: usize = 256;
const SAMPLE_RATE: u32 = 44_100;

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

/// Deterministic stereo test signal: channel 0 ramps up, channel 1 ramps down.
fn stereo_signal() -> AudioSamples<'static, f32> {
    let mut audio = AudioSamples::<f32>::zeros_multi_channel(
        NonZeroU32::new(2).unwrap(),
        NonZeroUsize::new(FRAMES).unwrap(),
        NonZeroU32::new(SAMPLE_RATE).unwrap(),
    );
    let flat = audio
        .as_multi_channel_mut()
        .expect("multi")
        .as_slice_mut()
        .expect("contiguous");
    for i in 0..FRAMES {
        flat[i] = i as f32 / FRAMES as f32; // ch 0
        flat[FRAMES + i] = -(i as f32) / FRAMES as f32; // ch 1
    }
    audio
}

fn chunk_buffer(channels: u32) -> AudioSamples<'static, f32> {
    AudioSamples::<f32>::zeros_multi_channel(
        NonZeroU32::new(channels).unwrap(),
        NonZeroUsize::new(CHUNK).unwrap(),
        NonZeroU32::new(SAMPLE_RATE).unwrap(),
    )
}

/// Interleaved copy of the samples — layout-agnostic, so it works for readers
/// that return non-contiguous (strided) views.
fn flat_samples(audio: &AudioSamples<'static, f32>) -> Vec<f32> {
    audio.to_interleaved_vec().into_iter().collect()
}

#[test]
fn wav_pipeline_round_trips_exactly_with_partial_final_chunk() {
    let input = tmp("ass_pipe_in.wav");
    let output = tmp("ass_pipe_out.wav");
    let signal = stereo_signal();
    audio_samples_io::write(&input, &signal).expect("write input");

    let mut source = WavFileStream::<_, f32>::open(&input).expect("open source");
    let sink = WavFileSink::<_, f32>::new_f32(
        std::io::BufWriter::new(std::fs::File::create(&output).expect("create")),
        2,
        SAMPLE_RATE,
    )
    .expect("create sink");

    let mut buffer = chunk_buffer(2);
    let frames = pipeline::run_finalized(&mut source, sink, &mut buffer).expect("pipeline");
    assert_eq!(
        frames, FRAMES,
        "pipeline must report exactly the source frame count"
    );

    let back = audio_samples_io::read::<_, f32>(&output).expect("read output");
    assert_eq!(
        back.samples_per_channel().get(),
        FRAMES,
        "output must not contain stale padding frames from the final partial chunk"
    );
    assert_eq!(
        flat_samples(&back),
        flat_samples(&signal),
        "bit-exact f32 round-trip"
    );

    std::fs::remove_file(&input).ok();
    std::fs::remove_file(&output).ok();
}

#[test]
fn flac_source_streams_incrementally_and_matches_full_decode() {
    let input = tmp("ass_flac_src.flac");
    let signal = stereo_signal();
    audio_samples_io::write(&input, &signal).expect("write flac");

    let expected = audio_samples_io::read::<_, f32>(&input).expect("full decode");

    let mut source = FlacFileStream::<_, f32>::open(&input).expect("open flac stream");
    assert_eq!(source.sample_rate(), SAMPLE_RATE);
    assert_eq!(source.channels(), 2);
    assert_eq!(source.total_frames(), FRAMES);

    // Accumulate every chunk the source produces. Note: file sources shrink the
    // buffer itself on the final partial read, so the per-channel stride must be
    // re-read from the buffer after each fill.
    let mut got: Vec<Vec<f32>> = vec![Vec::new(); 2];
    let mut buffer = chunk_buffer(2);
    while let Some(n) = source.fill_chunk(&mut buffer).expect("fill_chunk") {
        let stride = buffer.samples_per_channel().get();
        let flat = buffer
            .as_multi_channel()
            .expect("multi")
            .as_slice()
            .expect("contiguous");
        for (ch, sink) in got.iter_mut().enumerate() {
            sink.extend_from_slice(&flat[ch * stride..ch * stride + n]);
        }
    }

    // Compare per channel against the full decode (interleaved order).
    let expected_flat = flat_samples(&expected);
    let expected_ch =
        |c: usize| -> Vec<f32> { expected_flat.iter().copied().skip(c).step_by(2).collect() };
    assert_eq!(got[0], expected_ch(0), "channel 0 mismatch");
    assert_eq!(got[1], expected_ch(1), "channel 1 mismatch");

    std::fs::remove_file(&input).ok();
}

#[test]
fn flac_source_seek_and_reset() {
    let input = tmp("ass_flac_seek.flac");
    let signal = stereo_signal();
    audio_samples_io::write(&input, &signal).expect("write flac");

    let mut source = FlacFileStream::<_, f32>::open(&input).expect("open");
    assert_eq!(source.remaining_frames(), FRAMES);

    source.seek_to_frame(FRAMES - 10).expect("seek");
    assert_eq!(source.remaining_frames(), 10);
    let mut buffer = chunk_buffer(2);
    let n = source
        .fill_chunk(&mut buffer)
        .expect("fill")
        .expect("frames");
    assert_eq!(n, 10, "only the remaining frames should be produced");
    assert!(
        source.fill_chunk(&mut buffer).expect("fill").is_none(),
        "exhausted"
    );

    source.reset().expect("reset");
    assert_eq!(source.remaining_frames(), FRAMES);
    // The partial read above shrank `buffer` to 10 frames; allocate a fresh one.
    let mut buffer = chunk_buffer(2);
    let n = source
        .fill_chunk(&mut buffer)
        .expect("fill")
        .expect("frames");
    assert_eq!(n, CHUNK);

    std::fs::remove_file(&input).ok();
}

#[test]
fn flac_to_wav_pipeline_round_trips() {
    let input = tmp("ass_flac2wav_in.flac");
    let output = tmp("ass_flac2wav_out.wav");
    let signal = stereo_signal();
    audio_samples_io::write(&input, &signal).expect("write flac");
    // FLAC stores integers; f32 input is quantised on write. The pipeline must
    // reproduce what the FLAC file holds, not the pre-quantisation signal.
    let expected = audio_samples_io::read::<_, f32>(&input).expect("decode input");

    let mut source = FlacFileStream::<_, f32>::open(&input).expect("open flac");
    let sink = WavFileSink::<_, f32>::new_f32(
        std::io::BufWriter::new(std::fs::File::create(&output).expect("create")),
        2,
        SAMPLE_RATE,
    )
    .expect("create sink");

    let mut buffer = chunk_buffer(2);
    let frames = pipeline::run_finalized(&mut source, sink, &mut buffer).expect("pipeline");
    assert_eq!(frames, FRAMES);

    let back = audio_samples_io::read::<_, f32>(&output).expect("read output");
    assert_eq!(back.samples_per_channel().get(), FRAMES);
    assert_eq!(flat_samples(&back), flat_samples(&expected));

    std::fs::remove_file(&input).ok();
    std::fs::remove_file(&output).ok();
}

#[test]
fn mono_pipeline_round_trips() {
    let input = tmp("ass_mono_in.wav");
    let output = tmp("ass_mono_out.wav");

    let mut audio = AudioSamples::<f32>::zeros_mono(
        NonZeroUsize::new(FRAMES).unwrap(),
        NonZeroU32::new(SAMPLE_RATE).unwrap(),
    );
    {
        let s = audio.as_mono_mut().expect("mono").as_slice_mut();
        for (i, v) in s.iter_mut().enumerate() {
            *v = (i as f32 * 0.001).sin();
        }
    }
    audio_samples_io::write(&input, &audio).expect("write input");

    let mut source = WavFileStream::<_, f32>::open(&input).expect("open");
    let sink = WavFileSink::<_, f32>::new_f32(
        std::io::BufWriter::new(std::fs::File::create(&output).expect("create")),
        1,
        SAMPLE_RATE,
    )
    .expect("sink");

    let mut buffer = AudioSamples::<f32>::zeros_mono(
        NonZeroUsize::new(CHUNK).unwrap(),
        NonZeroU32::new(SAMPLE_RATE).unwrap(),
    );
    let frames = pipeline::run_finalized(&mut source, sink, &mut buffer).expect("pipeline");
    assert_eq!(frames, FRAMES);

    let back = audio_samples_io::read::<_, f32>(&output).expect("read");
    assert_eq!(back.samples_per_channel().get(), FRAMES);
    assert_eq!(flat_samples(&back), flat_samples(&audio));

    std::fs::remove_file(&input).ok();
    std::fs::remove_file(&output).ok();
}
