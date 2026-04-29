/// Capture audio from the default microphone and save it to a WAV file.
///
/// Records for a fixed duration, then finalises the output file.
///
/// Usage: cargo run --example device_capture --features device -- output.wav [seconds]
#[cfg(feature = "device")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs::File;
    use std::io::BufWriter;
    use std::num::{NonZeroU32, NonZeroUsize};
    use std::time::{Duration, Instant};

    use audio_samples::AudioSamples;
    use audio_samples_streaming::{AudioSink, AudioStream, DeviceCapture, WavFileSink};

    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "capture.wav".into());
    let seconds: f64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5.0);

    const CHUNK_FRAMES: usize = 1024;

    println!("Opening default input device...");
    let mut source = DeviceCapture::from_default_input(Some(std::time::Duration::from_millis(10)))?;

    let channels = source.channels();
    let sample_rate = source.sample_rate();

    println!("Device: {sample_rate} Hz  {channels} ch");

    let mut sink = WavFileSink::<_, f32>::new_f32(
        BufWriter::new(File::create(&output)?),
        channels,
        sample_rate,
    )?;

    let mut buffer = AudioSamples::<f32>::zeros_multi_channel(
        NonZeroU32::new(channels as u32).unwrap(),
        NonZeroUsize::new(CHUNK_FRAMES).unwrap(),
        NonZeroU32::new(sample_rate).unwrap(),
    );

    println!("Recording {seconds:.1}s → {output}  (Ctrl-C to stop early)");

    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    let mut total_frames = 0;

    while Instant::now() < deadline {
        if let Some(n) = source.fill_chunk(&mut buffer)? {
            // Apply processing to each chunk here if needed.
            sink.write_chunk(&buffer)?;
            total_frames += n;
        }
    }

    sink.finalize()?;
    println!(
        "Saved {total_frames} frames to {output}  ({} xruns)",
        source.xruns()
    );
    Ok(())
}

#[cfg(not(feature = "device"))]
fn main() {
    eprintln!("Run with --features device");
}
