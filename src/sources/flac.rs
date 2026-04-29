use audio_samples::AudioSamples;
use audio_samples::traits::StandardSample;
use audio_samples_io::error::AudioIOError;
use audio_samples_io::flac::FlacFile;
use audio_samples_io::traits::{AudioFile, AudioFileMetadata, AudioFileRead};

use crate::error::StreamingError;
use crate::traits::AudioStream;

/// An [`AudioStream`] that reads frames from a FLAC file on demand.
///
/// The file is fully decoded into memory at construction time (FLAC is not seekable
/// in the same way as WAV, so frame-level random access requires the full decode).
/// After that, [`fill_chunk`](AudioStream::fill_chunk) slices out chunks with no
/// additional allocation.
///
/// For most use cases this is acceptable: a 5-minute stereo 24-bit / 48 kHz file
/// decodes to roughly 170 MB of f32 samples in memory.
pub struct FlacFileStream<T: StandardSample> {
    data: AudioSamples<'static, T>,
    frame_pos: usize,
    total_frames: usize,
    sample_rate: u32,
    channels: u16,
}

impl<T: StandardSample + 'static> FlacFileStream<T> {
    /// Open and fully decode a FLAC file. The decoded PCM is held in memory for
    /// zero-allocation streaming via [`fill_chunk`](AudioStream::fill_chunk).
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Self, AudioIOError> {
        let flac = FlacFile::open(path)?;
        let sample_rate = flac.base_info()?.sample_rate.get();
        let channels = flac.num_channels();
        let data: AudioSamples<'static, T> = flac.read::<T>()?.into_owned();
        let total_frames = data.samples_per_channel().get();
        Ok(Self {
            data,
            frame_pos: 0,
            total_frames,
            sample_rate,
            channels,
        })
    }

    /// Sample rate of the file in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Number of channels in the file.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Total frames in the file.
    pub fn total_frames(&self) -> usize {
        self.total_frames
    }

    /// Frames remaining from the current position.
    pub fn remaining_frames(&self) -> usize {
        self.total_frames - self.frame_pos
    }

    /// Seek to a specific frame position (0-indexed).
    pub fn seek_to_frame(&mut self, frame: usize) {
        self.frame_pos = frame.min(self.total_frames);
    }

    /// Reset to the beginning of the audio data.
    pub fn reset(&mut self) {
        self.frame_pos = 0;
    }
}

impl<T: StandardSample + 'static> AudioStream for FlacFileStream<T> {
    type Sample = T;
    type Error = StreamingError;

    fn fill_chunk(
        &mut self,
        buffer: &mut AudioSamples<'static, T>,
    ) -> Result<Option<usize>, StreamingError> {
        if self.frame_pos >= self.total_frames {
            return Ok(None);
        }

        let buf_frames = buffer.samples_per_channel().get();
        let available = self.total_frames - self.frame_pos;
        let n = buf_frames.min(available);
        let channels = self.channels as usize;

        if channels == 1 {
            let src = self
                .data
                .as_mono()
                .ok_or_else(|| StreamingError::Parameter("expected mono source".into()))?;
            let src_slice = src
                .as_slice()
                .ok_or_else(|| StreamingError::Parameter("source buffer not contiguous".into()))?;
            let dst = buffer
                .as_mono_mut()
                .ok_or_else(|| StreamingError::Parameter("expected mono buffer".into()))?;
            dst.as_slice_mut()[..n].copy_from_slice(&src_slice[self.frame_pos..self.frame_pos + n]);
        } else {
            let total_frames = self.total_frames;
            let src = self
                .data
                .as_multi_channel()
                .ok_or_else(|| StreamingError::Parameter("expected multi-channel source".into()))?;
            let src_flat = src
                .as_slice()
                .ok_or_else(|| StreamingError::Parameter("source buffer not contiguous".into()))?;
            let dst = buffer
                .as_multi_channel_mut()
                .ok_or_else(|| StreamingError::Parameter("expected multi-channel buffer".into()))?;
            let dst_flat = dst
                .as_slice_mut()
                .ok_or_else(|| StreamingError::Parameter("buffer not contiguous".into()))?;

            for ch in 0..channels {
                let src_start = ch * total_frames + self.frame_pos;
                let dst_start = ch * buf_frames;
                dst_flat[dst_start..dst_start + n]
                    .copy_from_slice(&src_flat[src_start..src_start + n]);
            }
        }

        self.frame_pos += n;
        Ok(Some(n))
    }
}
