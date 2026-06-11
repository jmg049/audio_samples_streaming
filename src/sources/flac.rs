use std::fs::File;
use std::io::BufReader;
use std::marker::PhantomData;
use std::path::Path;

use audio_samples::AudioSamples;
use audio_samples::traits::StandardSample;
use audio_samples_io::ReadSeek;
use audio_samples_io::error::AudioIOError;
use audio_samples_io::flac::StreamedFlacFile;
use audio_samples_io::traits::{AudioStreamRead, AudioStreamReader};

use crate::traits::AudioStream;

/// An [`AudioStream`] that decodes frames from a FLAC file on demand.
///
/// Wraps [`StreamedFlacFile`] from `audio_samples_io`: the stream info is parsed at
/// construction and FLAC frames are decoded incrementally as
/// [`fill_chunk`](AudioStream::fill_chunk) consumes them, so memory stays bounded by
/// one FLAC block regardless of file length.
pub struct FlacFileStream<R: ReadSeek, T: StandardSample> {
    inner: StreamedFlacFile<R>,
    _marker: PhantomData<T>,
}

impl<R: ReadSeek, T: StandardSample> FlacFileStream<R, T> {
    /// Create a stream from any `Read + Seek` source.
    pub fn new(reader: R) -> Result<Self, AudioIOError> {
        Ok(Self {
            inner: StreamedFlacFile::new(reader)?,
            _marker: PhantomData,
        })
    }

    /// Sample rate of the file in Hz.
    pub fn sample_rate(&self) -> u32 {
        AudioStreamReader::sample_rate(&self.inner)
    }

    /// Number of channels in the file.
    pub fn channels(&self) -> u16 {
        AudioStreamReader::num_channels(&self.inner)
    }

    /// Total frames in the file.
    pub fn total_frames(&self) -> usize {
        AudioStreamReader::total_frames(&self.inner)
    }

    /// Frames remaining from the current position.
    pub fn remaining_frames(&self) -> usize {
        AudioStreamReader::remaining_frames(&self.inner)
    }

    /// Seek to a specific frame position (0-indexed).
    pub fn seek_to_frame(&mut self, frame: usize) -> Result<(), AudioIOError> {
        AudioStreamReader::seek_to_frame(&mut self.inner, frame)
    }

    /// Reset to the beginning of the audio data.
    pub fn reset(&mut self) -> Result<(), AudioIOError> {
        AudioStreamReader::reset(&mut self.inner)
    }
}

impl<T: StandardSample + 'static> FlacFileStream<BufReader<File>, T> {
    /// Open a FLAC file by path for incremental decoding.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, AudioIOError> {
        let reader = BufReader::new(File::open(path)?);
        Self::new(reader)
    }
}

impl<R: ReadSeek, T: StandardSample + 'static> AudioStream for FlacFileStream<R, T> {
    type Sample = T;
    type Error = AudioIOError;

    fn fill_chunk(
        &mut self,
        buffer: &mut AudioSamples<'static, T>,
    ) -> Result<Option<usize>, Self::Error> {
        if AudioStreamReader::remaining_frames(&self.inner) == 0 {
            return Ok(None);
        }

        let frame_count = buffer.samples_per_channel();
        let n = self.inner.read_frames_into(buffer, frame_count)?;

        if n == 0 { Ok(None) } else { Ok(Some(n)) }
    }
}
