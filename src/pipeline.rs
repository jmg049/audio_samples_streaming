use std::num::NonZeroUsize;

use audio_samples::AudioSamples;
use audio_samples::traits::StandardSample;

use crate::error::{StreamingError, StreamingResult};
use crate::traits::{AudioSink, AudioStream};

/// Drive `source` into `sink` chunk by chunk using a pre-allocated `buffer`.
///
/// The buffer must be allocated with the correct sample rate, channel count, and chunk size
/// before calling — typically via [`AudioSamples::zeros_multi_channel`]. Returns the total
/// number of frames processed.
///
/// When the source fills only part of the buffer (the final chunk of a file, or a device
/// capture period), only those frames are written to the sink — a partial chunk is copied
/// into a right-sized buffer first, so the sink never receives stale padding frames.
///
/// Calls [`flush`](crate::traits::AudioSink::flush) on the sink after the source is exhausted,
/// but does **not** call [`finalize`](crate::traits::AudioSink::finalize) — use
/// [`run_finalized`] when the sink's lifetime ends with the pipeline, or finalize manually
/// after `run` returns.
///
/// [`AudioSamples::zeros_multi_channel`]: audio_samples::AudioSamples::zeros_multi_channel
pub fn run<S, K>(
    source: &mut S,
    sink: &mut K,
    buffer: &mut AudioSamples<'static, S::Sample>,
) -> StreamingResult<usize>
where
    S: AudioStream,
    K: AudioSink<Sample = S::Sample>,
    S::Error: Into<crate::error::StreamingError>,
    K::Error: Into<crate::error::StreamingError>,
{
    let mut total_frames = 0;

    loop {
        match source.fill_chunk(buffer).map_err(Into::into)? {
            None => break,
            Some(n) => {
                total_frames += n;
                // File sources shrink the buffer itself on a partial (final) read, so
                // the dimensions are re-checked after every fill. When a source reports
                // fewer frames than the buffer holds (e.g. device capture), the frames
                // beyond `n` are stale data and must not reach the sink.
                if n == buffer.samples_per_channel().get() {
                    sink.write_chunk(buffer).map_err(Into::into)?;
                } else {
                    let partial = truncate_chunk(buffer, n)?;
                    sink.write_chunk(&partial).map_err(Into::into)?;
                }
            }
        }
    }

    sink.flush().map_err(Into::into)?;

    Ok(total_frames)
}

/// Drive `source` into `sink` like [`run`], then consume and finalize the sink.
///
/// This is the complete-pipeline variant: sinks whose on-disk representation is only
/// valid after finalization (WAV header byte counts, FLAC STREAMINFO) are guaranteed to
/// be finalized before this returns. Returns the total number of frames processed.
pub fn run_finalized<S, K>(
    source: &mut S,
    mut sink: K,
    buffer: &mut AudioSamples<'static, S::Sample>,
) -> StreamingResult<usize>
where
    S: AudioStream,
    K: AudioSink<Sample = S::Sample>,
    S::Error: Into<crate::error::StreamingError>,
    K::Error: Into<crate::error::StreamingError>,
{
    let total_frames = run(source, &mut sink, buffer)?;
    sink.finalize().map_err(Into::into)?;
    Ok(total_frames)
}

/// Copy the first `n` frames of `buffer` into a right-sized `AudioSamples`.
///
/// Allocates — but only runs for partial chunks, which occur at most once per file
/// (and per capture period for device sources).
fn truncate_chunk<T: StandardSample + 'static>(
    buffer: &AudioSamples<'static, T>,
    n: usize,
) -> StreamingResult<AudioSamples<'static, T>> {
    let n_nz = NonZeroUsize::new(n)
        .ok_or_else(|| StreamingError::Parameter("cannot truncate to zero frames".into()))?;
    let channels = buffer.num_channels();
    let buf_frames = buffer.samples_per_channel().get();
    let sample_rate = buffer.sample_rate();

    if channels.get() == 1 {
        let src = buffer
            .as_mono()
            .ok_or_else(|| StreamingError::Parameter("expected mono buffer".into()))?;
        let src_slice = src
            .as_slice()
            .ok_or_else(|| StreamingError::Parameter("buffer not contiguous".into()))?;
        let mut out = AudioSamples::<T>::zeros_mono(n_nz, sample_rate);
        out.as_mono_mut()
            .ok_or_else(|| StreamingError::Parameter("expected mono buffer".into()))?
            .as_slice_mut()
            .copy_from_slice(&src_slice[..n]);
        Ok(out)
    } else {
        let src = buffer
            .as_multi_channel()
            .ok_or_else(|| StreamingError::Parameter("expected multi-channel buffer".into()))?;
        let src_flat = src
            .as_slice()
            .ok_or_else(|| StreamingError::Parameter("buffer not contiguous".into()))?;
        let mut out = AudioSamples::<T>::zeros_multi_channel(channels, n_nz, sample_rate);
        {
            let dst = out
                .as_multi_channel_mut()
                .ok_or_else(|| StreamingError::Parameter("expected multi-channel buffer".into()))?;
            let dst_flat = dst
                .as_slice_mut()
                .ok_or_else(|| StreamingError::Parameter("buffer not contiguous".into()))?;
            for ch in 0..channels.get() as usize {
                dst_flat[ch * n..(ch + 1) * n]
                    .copy_from_slice(&src_flat[ch * buf_frames..ch * buf_frames + n]);
            }
        }
        Ok(out)
    }
}
