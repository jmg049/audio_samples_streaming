use thiserror::Error;

/// All errors that can be produced by streaming operations.
#[derive(Debug, Error)]
pub enum StreamingError {
    /// An underlying I/O error from the OS or file system.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// An error from the `audio_samples` crate (e.g. shape mismatch, SIMD conversion failure).
    #[error(transparent)]
    AudioSamples(#[from] audio_samples::AudioSampleError),

    /// An error from the `audio_samples_io` crate (e.g. malformed WAV header, unsupported codec).
    #[error(transparent)]
    AudioIO(#[from] audio_samples_io::AudioIOError),

    /// The stream has no more data to produce.
    ///
    /// Returned by sources that signal exhaustion through an error rather than `Ok(None)`.
    #[error("stream exhausted")]
    Exhausted,

    /// The source and sink disagree on sample rate, channel count, or bit depth.
    #[error("format mismatch: {0}")]
    FormatMismatch(String),

    /// A requested feature or sample format is not supported by this implementation.
    ///
    /// Common cause: a cpal device reports a native format (e.g. `F64`) that has no
    /// conversion path in this crate.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// A construction or configuration parameter was invalid.
    ///
    /// Common causes: no default input/output device, device open failure, ring buffer
    /// size of zero.
    #[error("parameter error: {0}")]
    Parameter(String),
}

pub type StreamingResult<T> = Result<T, StreamingError>;
