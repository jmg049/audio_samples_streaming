/// Manual chunk loop — shows exactly where per-chunk AudioSamples operations go.
///
/// This is the expanded form of what `pipeline::run` does internally. Use this
/// pattern when you need control over each chunk: conditional processing, early
/// exit, per-chunk metrics, or operations that pipeline::run can't express.
///
/// Usage: cargo run --example manual_loop -- input.wav output.wav
use std::fs::File;
use std::io::BufWriter;
use std::num::{NonZeroU32, NonZeroUsize};

use audio_samples::AudioSamples;
use audio_samples_streaming::{AudioSink, AudioStream, WavFileSink, WavFileStream};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "input.wav".into());
    let output = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "output.wav".into());

    let mut source = WavFileStream::<_, f32>::open(&input)?;

    let sample_rate = source.sample_rate();
    let channels = source.num_channels();

    let mut sink = WavFileSink::<_, f32>::new_f32(
        BufWriter::new(File::create(&output)?),
        channels,
        sample_rate,
    )?;

    let mut buffer = AudioSamples::<f32>::zeros_multi_channel(
        NonZeroU32::new(channels as u32).unwrap(),
        NonZeroUsize::new(1024).unwrap(),
        NonZeroU32::new(sample_rate).unwrap(),
    );

    let mut total_frames = 0;
    let mut chunks = 0;

    while let Some(n) = source.fill_chunk(&mut buffer)? {
        // `buffer` is an AudioSamples<f32> — the full audio_samples API applies here.
        // Enable the relevant audio_samples features and drop in operations, e.g.:
        //
        //   buffer.scale(gain);                              // requires: processing
        //   buffer.normalize(NormalizationConfig::peak());   // requires: processing
        //   buffer.apply_limiter(&LimiterConfig::default()); // requires: dynamic-range
        //   buffer.fade_in(Duration::from_millis(50));       // requires: editing

        sink.write_chunk(&buffer)?;

        total_frames += n;
        chunks += 1;
    }

    sink.finalize()?;

    println!("Processed {total_frames} frames in {chunks} chunks");
    Ok(())
}
