use std::pin::Pin;
use std::task::{Context, Poll};

use audio_samples::AudioSamples;
use futures_core::Stream;

use crate::traits::AudioStream;

/// Adapts any [`AudioStream`] into a [`futures_core::Stream`].
///
/// Each `poll_next` call invokes [`fill_chunk`](AudioStream::fill_chunk) synchronously.
/// This is suitable for file-based streaming where blocking is short. For sources that
/// may block for an extended period (e.g. device capture), wrap in `spawn_blocking`.
///
/// Yields `Ok(n)` where `n` is the number of frames written into the buffer, or
/// `Err(e)` on error. Returns `None` when the source is exhausted.
pub struct AsyncAudioStream<S: AudioStream + Send> {
    stream: S,
    buffer: AudioSamples<'static, S::Sample>,
}

impl<S: AudioStream + Send> AsyncAudioStream<S> {
    /// Wrap `stream` as an async `Stream`, using `buffer` as the per-chunk staging area.
    ///
    /// `buffer` must be pre-allocated with the correct channel count, sample rate, and
    /// chunk size. Its dimensions are fixed for the lifetime of the `AsyncAudioStream`.
    pub fn new(stream: S, buffer: AudioSamples<'static, S::Sample>) -> Self {
        Self { stream, buffer }
    }

    /// Access the underlying buffer after each successful poll.
    pub fn buffer(&self) -> &AudioSamples<'static, S::Sample> {
        &self.buffer
    }
}

impl<S: AudioStream + Send + Unpin> Stream for AsyncAudioStream<S> {
    type Item = Result<usize, S::Error>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.stream.fill_chunk(&mut this.buffer) {
            Ok(Some(n)) => Poll::Ready(Some(Ok(n))),
            Ok(None) => Poll::Ready(None),
            Err(e) => Poll::Ready(Some(Err(e))),
        }
    }
}
