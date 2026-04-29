use audio_samples::AudioSamples;

use crate::error::StreamingResult;
use crate::traits::{AudioSink, AudioStream};

/// Drive `source` into `sink` chunk by chunk using a pre-allocated `buffer`.
///
/// The buffer must be allocated with the correct sample rate, channel count, and chunk size
/// before calling — typically via [`AudioSamples::zeros_multi_channel`]. Returns the total
/// number of frames processed.
///
/// Calls [`flush`](crate::traits::AudioSink::flush) on the sink after the source is exhausted,
/// but does **not** call [`finalize`](crate::traits::AudioSink::finalize) — the caller must do
/// that after `run` returns.
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
                sink.write_chunk(buffer).map_err(Into::into)?;
            }
        }
    }

    sink.flush().map_err(Into::into)?;

    Ok(total_frames)
}
