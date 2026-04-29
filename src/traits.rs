use audio_samples::{AudioSamples, traits::StandardSample};

use crate::error::StreamingError;

/// Pull-based audio source.
///
/// Implementors produce audio frame-by-frame into a caller-owned buffer. The buffer must be
/// pre-allocated with the correct channel count and sample rate before the first call —
/// both are fixed by the source at construction time.
pub trait AudioStream {
    /// The PCM sample type produced by this source (e.g. `f32`, `i16`).
    type Sample: StandardSample;

    /// The error type, convertible to [`StreamingError`].
    type Error: Into<StreamingError>;

    /// Fill `buffer` with the next chunk of audio frames.
    ///
    /// Returns `Ok(Some(n))` where `n` is the number of frames written, or `Ok(None)` when
    /// the stream is exhausted. The buffer dimensions (channel count, sample rate, frame
    /// capacity) must match what the source was configured with.
    fn fill_chunk(
        &mut self,
        buffer: &mut AudioSamples<'static, Self::Sample>,
    ) -> Result<Option<usize>, Self::Error>;
}

/// Push-based audio sink.
///
/// Implementors consume audio chunks and write them to an underlying destination (file,
/// device, network, etc.). [`finalize`](Self::finalize) must be called exactly once when
/// all audio has been written — some implementations (e.g. WAV) write critical header
/// fields only at finalization time.
pub trait AudioSink {
    /// The PCM sample type accepted by this sink (e.g. `f32`, `i16`).
    type Sample: StandardSample;

    /// The error type, convertible to [`StreamingError`].
    type Error: Into<StreamingError>;

    /// Write `chunk` to the sink.
    fn write_chunk(
        &mut self,
        chunk: &AudioSamples<'static, Self::Sample>,
    ) -> Result<(), Self::Error>;

    /// Flush any internally buffered data to the underlying destination.
    ///
    /// For device sinks this blocks until the hardware has consumed all queued audio.
    fn flush(&mut self) -> Result<(), Self::Error>;

    /// Finalize the sink, flushing and releasing all resources.
    ///
    /// Must be called once when writing is complete. For [`WavFileSink`](crate::WavFileSink)
    /// this writes the correct byte-count fields into the WAV header; omitting it produces
    /// a malformed file.
    fn finalize(self) -> Result<(), Self::Error>;
}
