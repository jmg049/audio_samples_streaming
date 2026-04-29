/// Stream a WAV file through a processing pipeline and write the result to a new WAV file.
///
/// Usage: cargo run --example file_to_file -- input.wav output.wav
///
/// The key property: only one chunk (1024 frames) is ever in memory at once, regardless
/// of how large the input file is.
use std::fs::File;
use std::io::BufWriter;
use std::num::{NonZeroU32, NonZeroUsize};

use audio_samples::AudioSamples;
use audio_samples_streaming::pipeline;
use audio_samples_streaming::{AudioSink, WavFileSink, WavFileStream};

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

    println!("Input:  {input}");
    println!(
        "  {sample_rate} Hz  {channels} ch  {} frames",
        source.total_frames()
    );

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

    let frames = pipeline::run(&mut source, &mut sink, &mut buffer)?;
    sink.finalize()?;

    println!("Output: {output}  ({frames} frames written)");
    Ok(())
}
