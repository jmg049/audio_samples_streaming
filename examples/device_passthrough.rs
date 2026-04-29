/// Live passthrough — capture from the default microphone and play back through the default
/// output device in real time.
///
/// Each chunk passes through the loop body before going to the speakers. Drop in any
/// `AudioSamples` operation there for real-time effects.
///
/// Usage: cargo run --example device_passthrough --features device
#[cfg(feature = "device")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::num::{NonZeroU32, NonZeroUsize};

    use audio_samples::AudioSamples;
    use audio_samples_streaming::{AudioSink, AudioStream, DeviceCapture, DevicePlayback};

    const CHUNK_FRAMES: usize = 1024;

    println!("Opening devices...");
    let mut source = DeviceCapture::from_default_input(Some(std::time::Duration::from_millis(5)))?;
    let mut sink = DevicePlayback::from_default_output(Some(std::time::Duration::from_millis(5)))?;

    let channels = source.channels();
    let sample_rate = source.sample_rate();

    println!("Input:  {sample_rate} Hz  {channels} ch");
    println!("Output: {} Hz  {} ch", sink.sample_rate(), sink.channels());

    let mut buffer = AudioSamples::<f32>::zeros_multi_channel(
        NonZeroU32::new(channels as u32).unwrap(),
        NonZeroUsize::new(CHUNK_FRAMES).unwrap(),
        NonZeroU32::new(sample_rate).unwrap(),
    );

    println!("Passthrough active — Ctrl-C to stop.");

    // Run until the user interrupts. DeviceCapture::fill_chunk returns None only on
    // device error; the loop exits via Ctrl-C (SIGINT).
    while let Some(_n) = source.fill_chunk(&mut buffer)? {
        // Insert per-chunk processing here before sending to the output device, e.g.:
        //
        //   buffer.scale(gain);                              // requires: processing
        //   buffer.apply_limiter(&LimiterConfig::default()); // requires: dynamic-range
        //   buffer.fade_in(Duration::from_millis(50));       // requires: editing

        sink.write_chunk(&buffer)?;
    }

    println!(
        "Xruns — input: {}  output: {}",
        source.xruns(),
        sink.xruns()
    );
    sink.finalize()?;
    Ok(())
}

#[cfg(not(feature = "device"))]
fn main() {
    eprintln!("Run with --features device");
}
