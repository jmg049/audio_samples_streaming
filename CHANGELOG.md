# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added
- `pipeline::run_finalized` — drives the pipeline and then consumes and finalizes the sink, so finalization can no longer be forgotten
- Integration test suite covering WAV/FLAC pipelines, partial final chunks, and FLAC seek/reset

### Changed
- `FlacFileStream` decodes incrementally via `StreamedFlacFile` (memory bounded by one FLAC block) instead of fully decoding the file at construction; its type now carries the reader generic, matching `WavFileStream`
- Updated to `audio_samples_io` 0.3 and `audio_samples` 1.0.11

### Fixed
- `pipeline::run` wrote the full buffer even when the source produced a partial chunk, leaking stale frames into the sink for sources that do not shrink the buffer (e.g. device capture)

## v0.1.0

### Added

- Initial release: chunk-based audio streaming over a unified `AudioStream`/`AudioSink` trait pair, with WAV and FLAC file I/O, real-time device capture/playback via cpal, a rodio 0.22 `Source` adapter, and an async `Stream` adapter


### Todo

- More testing