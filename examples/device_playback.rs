/// Stream a WAV file to the default output device (speakers / headphones).
///
/// The file is never fully loaded into memory — chunks are decoded and handed to
/// the device in real time.
///
/// Usage: cargo run --example device_playback --features device -- input.wav
#[cfg(feature = "device")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::num::{NonZeroU32, NonZeroUsize};

    use audio_samples::AudioSamples;
    use audio_samples_streaming::{AudioSink, AudioStream, DevicePlayback, WavFileStream};

    let input = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "input.wav".into());

    const CHUNK_FRAMES: usize = 1024;

    let mut source = WavFileStream::<_, f32>::open(&input)?;

    let sample_rate = source.sample_rate();
    let channels = source.num_channels();

    println!(
        "Playing: {input}  ({sample_rate} Hz  {channels} ch  {} frames)",
        source.total_frames()
    );

    let mut sink = DevicePlayback::from_default_output(Some(std::time::Duration::from_millis(10)))?;

    let mut buffer = AudioSamples::<f32>::zeros_multi_channel(
        NonZeroU32::new(channels as u32).unwrap(),
        NonZeroUsize::new(CHUNK_FRAMES).unwrap(),
        NonZeroU32::new(sample_rate).unwrap(),
    );

    while let Some(_n) = source.fill_chunk(&mut buffer)? {
        // Insert any per-chunk processing here before sending to the device.
        sink.write_chunk(&buffer)?;
    }

    println!("Xruns: {}", sink.xruns());
    sink.finalize()?;
    println!("Playback complete.");
    Ok(())
}

#[cfg(not(feature = "device"))]
fn main() {
    eprintln!("Run with --features device");
}
