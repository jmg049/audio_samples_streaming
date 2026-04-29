/// Process a WAV file using async iteration over audio chunks.
///
/// `AsyncAudioStream` wraps any `AudioStream` as a `futures_core::Stream`. Each chunk
/// is yielded with `Ok(n)` where `n` is the number of frames written into the buffer.
/// Access the buffer via `.buffer()` between polls to inspect or process audio data.
///
/// Note: `fill_chunk` is called synchronously inside `poll_next`. For file-based
/// sources this is fine. For sources that block for extended periods (device capture),
/// use `spawn_blocking` instead.
///
/// Usage: cargo run --example async_stream --features async -- input.wav
#[cfg(feature = "async")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    async_main()
}

#[cfg(feature = "async")]
fn async_main() -> Result<(), Box<dyn std::error::Error>> {
    use std::num::{NonZeroU32, NonZeroUsize};

    use audio_samples::AudioSamples;
    use audio_samples_streaming::{AsyncAudioStream, WavFileStream};
    use futures_core::Stream;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    let input = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "input.wav".into());

    const CHUNK_FRAMES: usize = 1024;

    let source = WavFileStream::<_, f32>::open(&input)?;

    let sample_rate = source.sample_rate();
    let channels = source.num_channels();

    println!("Input: {input}  ({sample_rate} Hz  {channels} ch)");

    let buffer = AudioSamples::<f32>::zeros_multi_channel(
        NonZeroU32::new(channels as u32).unwrap(),
        NonZeroUsize::new(CHUNK_FRAMES).unwrap(),
        NonZeroU32::new(sample_rate).unwrap(),
    );

    let mut stream = AsyncAudioStream::new(source, buffer);

    // Drive the stream with a simple poll loop. In a real async context this would
    // be inside an async runtime (tokio, async-std, etc.) using `stream.next()`.
    let waker = futures_task_noop_waker();
    let mut cx = Context::from_waker(&waker);
    let mut pinned = Pin::new(&mut stream);

    let mut total_frames = 0usize;
    let mut chunks = 0usize;

    loop {
        match pinned.as_mut().poll_next(&mut cx) {
            Poll::Ready(Some(Ok(n))) => {
                // `stream.buffer()` contains the freshly filled chunk.
                // Insert per-chunk processing here via the AudioSamples API.
                total_frames += n;
                chunks += 1;
            }
            Poll::Ready(Some(Err(e))) => return Err(e.into()),
            Poll::Ready(None) => break,
            Poll::Pending => {}
        }
    }

    println!("Processed {total_frames} frames in {chunks} chunks");
    Ok(())
}

/// Minimal no-op waker for driving a `Stream` without a real async executor.
#[cfg(feature = "async")]
fn futures_task_noop_waker() -> std::task::Waker {
    use std::task::{RawWaker, RawWakerVTable, Waker};

    static VTABLE: RawWakerVTable =
        RawWakerVTable::new(|p| RawWaker::new(p, &VTABLE), |_| {}, |_| {}, |_| {});

    unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
}

#[cfg(not(feature = "async"))]
fn main() {
    eprintln!("Run with --features async");
}
