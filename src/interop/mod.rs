#[cfg(feature = "async")]
pub mod async_stream;
#[cfg(feature = "rodio")]
pub mod rodio;

#[cfg(feature = "async")]
pub use async_stream::AsyncAudioStream;
#[cfg(feature = "rodio")]
pub use rodio::RodioSource;
