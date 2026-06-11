//! Chunk-based audio streaming for Rust.
//!
//! Two traits drive the whole crate:
//!
//! - [`AudioStream`] — pull source. [`fill_chunk`](AudioStream::fill_chunk) fills a
//!   pre-allocated [`AudioSamples`](audio_samples::AudioSamples) buffer and returns the
//!   frame count, or `None` when exhausted.
//! - [`AudioSink`] — push destination. [`write_chunk`](AudioSink::write_chunk) accepts the
//!   same buffer; [`finalize`](AudioSink::finalize) must be called when writing is complete.
//!
//! # Sources and sinks
//!
//! | Type | Direction | Feature |
//! |------|-----------|---------|
//! | [`WavFileStream`] | source | `wav` |
//! | [`WavFileSink`] | sink | `wav` |
//! | [`FlacFileStream`] | source | `flac` |
//! | [`DeviceCapture`] | source | `device` |
//! | [`DevicePlayback`] | sink | `device` |
//! | [`RodioSource`] | source adapter | `rodio` |
//! | [`AsyncAudioStream`] | stream adapter | `async` |
//!
//! No features are enabled by default.
//!
//! # Quick start
//!
//! Read `input.wav` chunk by chunk and write it to `output.wav`, using
//! [`pipeline::run_finalized`] to drive the loop and finalize the sink. Insert
//! per-chunk processing between source and sink by writing a manual loop
//! instead — see the `manual_loop` example.
//!
//! ```no_run
//! # #[cfg(feature = "wav")] {
//! use std::num::{NonZeroU32, NonZeroUsize};
//! use audio_samples::AudioSamples;
//! use audio_samples_streaming::{WavFileStream, WavFileSink, pipeline};
//!
//! let mut source = WavFileStream::<_, f32>::open("input.wav").unwrap();
//! let sink = WavFileSink::<_, f32>::create("output.wav",
//!     source.num_channels(), source.sample_rate()).unwrap();
//!
//! let mut buffer = AudioSamples::<f32>::zeros_multi_channel(
//!     NonZeroU32::new(source.num_channels() as u32).unwrap(),
//!     NonZeroUsize::new(1024).unwrap(),
//!     NonZeroU32::new(source.sample_rate()).unwrap(),
//! );
//!
//! pipeline::run_finalized(&mut source, sink, &mut buffer).unwrap();
//! # }
//! ```
//!
//! # Device I/O
//!
//! Enable the `device` feature for real-time capture and playback. Both types use a
//! lock-free ring buffer and condvar notification — no spin-sleep, no hot-path allocation.
//! Pass a [`Duration`](std::time::Duration) to request a specific callback period; `None`
//! uses the driver default.
//!
//! ```no_run
//! # #[cfg(feature = "device")] {
//! use std::time::Duration;
//! use audio_samples_streaming::{DeviceCapture, DevicePlayback};
//!
//! let capture  = DeviceCapture::from_default_input(Some(Duration::from_millis(5))).unwrap();
//! let playback = DevicePlayback::from_default_output(Some(Duration::from_millis(5))).unwrap();
//!
//! println!("Capture:  {} Hz  {} ch", capture.sample_rate(), capture.channels());
//! println!("Playback: {} Hz  {} ch", playback.sample_rate(), playback.channels());
//! # }
//! ```

pub mod error;
pub mod interop;
pub mod pipeline;
pub mod sinks;
pub mod sources;
pub mod traits;

#[cfg(feature = "device")]
pub(crate) mod notify;

pub use error::{StreamingError, StreamingResult};
pub use traits::{AudioSink, AudioStream};

#[cfg(feature = "wav")]
pub use sinks::WavFileSink;
#[cfg(feature = "flac")]
pub use sources::FlacFileStream;
#[cfg(feature = "wav")]
pub use sources::WavFileStream;

#[cfg(feature = "device")]
pub use sinks::DevicePlayback;
#[cfg(feature = "device")]
pub use sources::DeviceCapture;

#[cfg(feature = "async")]
pub use interop::AsyncAudioStream;
#[cfg(feature = "rodio")]
pub use interop::RodioSource;
