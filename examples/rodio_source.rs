/// Stream a WAV file through rodio for playback on the default output device.
///
/// `RodioSource` adapts any `AudioStream<Sample = f32>` into a `rodio::Source`, so
/// the full rodio pipeline (mixing, effects, sinks) is available. The WAV data is
/// decoded one chunk at a time — the file is never fully loaded into memory.
///
/// Usage: cargo run --example rodio_source --features rodio -- input.wav
#[cfg(feature = "rodio")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::num::{NonZeroU32, NonZeroUsize};

    use audio_samples::AudioSamples;
    use audio_samples_streaming::{RodioSource, WavFileStream};
    use rodio::{DeviceSinkBuilder, Player};

    let input = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "input.wav".into());

    const CHUNK_FRAMES: usize = 2048;

    let source = WavFileStream::<_, f32>::open(&input)?;

    let sample_rate = source.sample_rate();
    let channels = source.num_channels();

    println!(
        "Playing: {input}  ({sample_rate} Hz  {channels} ch  {} frames)",
        source.total_frames()
    );

    let buffer = AudioSamples::<f32>::zeros_multi_channel(
        NonZeroU32::new(channels as u32).unwrap(),
        NonZeroUsize::new(CHUNK_FRAMES).unwrap(),
        NonZeroU32::new(sample_rate).unwrap(),
    );

    let rodio_source = RodioSource::new(source, buffer);

    let device_sink = DeviceSinkBuilder::open_default_sink()?;
    let player = Player::connect_new(device_sink.mixer());

    player.append(rodio_source);
    player.sleep_until_end();

    println!("Playback complete.");
    Ok(())
}

#[cfg(not(feature = "rodio"))]
fn main() {
    eprintln!("Run with --features rodio");
}
