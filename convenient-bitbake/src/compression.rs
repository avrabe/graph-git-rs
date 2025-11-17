//! Compression support for cache artifacts

use std::io::{self, Read, Write};

/// Compression algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionAlgorithm {
    /// No compression
    None,
    /// Zstandard compression (best compression ratio)
    Zstd,
    /// LZ4 compression (fastest)
    Lz4,
}

impl CompressionAlgorithm {
    /// Get recommended compression level
    pub fn default_level(&self) -> i32 {
        match self {
            Self::None => 0,
            Self::Zstd => 3,  // Balance speed/compression
            Self::Lz4 => 0,   // LZ4 has single mode
        }
    }

    /// Detect compression from magic bytes
    pub fn detect(data: &[u8]) -> Self {
        if data.len() < 4 {
            return Self::None;
        }

        // Zstd magic: 0x28 0xB5 0x2F 0xFD
        if data[0..4] == [0x28, 0xB5, 0x2F, 0xFD] {
            return Self::Zstd;
        }

        // LZ4 frame magic: 0x04 0x22 0x4D 0x18
        if data[0..4] == [0x04, 0x22, 0x4D, 0x18] {
            return Self::Lz4;
        }

        Self::None
    }
}

/// Compression statistics
#[derive(Debug, Clone, Default)]
pub struct CompressionStats {
    pub original_size: u64,
    pub compressed_size: u64,
    pub compression_time_ms: u64,
    pub decompression_time_ms: u64,
}

impl CompressionStats {
    /// Calculate compression ratio
    pub fn ratio(&self) -> f64 {
        if self.original_size == 0 {
            0.0
        } else {
            (self.compressed_size as f64 / self.original_size as f64) * 100.0
        }
    }

    /// Calculate space saved
    pub fn space_saved(&self) -> u64 {
        self.original_size.saturating_sub(self.compressed_size)
    }

    /// Calculate compression throughput (MB/s)
    pub fn compression_throughput(&self) -> f64 {
        if self.compression_time_ms == 0 {
            0.0
        } else {
            (self.original_size as f64 / 1_000_000.0) / (self.compression_time_ms as f64 / 1000.0)
        }
    }

    /// Calculate decompression throughput (MB/s)
    pub fn decompression_throughput(&self) -> f64 {
        if self.decompression_time_ms == 0 {
            0.0
        } else {
            (self.original_size as f64 / 1_000_000.0) / (self.decompression_time_ms as f64 / 1000.0)
        }
    }
}

/// Compress data
pub fn compress(
    data: &[u8],
    algorithm: CompressionAlgorithm,
    level: i32,
) -> io::Result<Vec<u8>> {
    match algorithm {
        CompressionAlgorithm::None => Ok(data.to_vec()),

        CompressionAlgorithm::Zstd => {
            zstd::encode_all(data, level)
        }

        CompressionAlgorithm::Lz4 => {
            let mut encoder = lz4::EncoderBuilder::new()
                .build(Vec::new())?;
            encoder.write_all(data)?;
            let (compressed, result) = encoder.finish();
            result?;
            Ok(compressed)
        }
    }
}

/// Decompress data
pub fn decompress(data: &[u8]) -> io::Result<Vec<u8>> {
    let algorithm = CompressionAlgorithm::detect(data);

    match algorithm {
        CompressionAlgorithm::None => Ok(data.to_vec()),

        CompressionAlgorithm::Zstd => {
            zstd::decode_all(data)
        }

        CompressionAlgorithm::Lz4 => {
            let mut decoder = lz4::Decoder::new(data)?;
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed)?;
            Ok(decompressed)
        }
    }
}

/// Compress data with statistics
pub fn compress_with_stats(
    data: &[u8],
    algorithm: CompressionAlgorithm,
    level: i32,
) -> io::Result<(Vec<u8>, CompressionStats)> {
    let start = std::time::Instant::now();
    let compressed = compress(data, algorithm, level)?;
    let elapsed = start.elapsed();

    let stats = CompressionStats {
        original_size: data.len() as u64,
        compressed_size: compressed.len() as u64,
        compression_time_ms: elapsed.as_millis() as u64,
        decompression_time_ms: 0,
    };

    Ok((compressed, stats))
}

/// Decompress data with statistics
pub fn decompress_with_stats(data: &[u8]) -> io::Result<(Vec<u8>, CompressionStats)> {
    let start = std::time::Instant::now();
    let decompressed = decompress(data)?;
    let elapsed = start.elapsed();

    let stats = CompressionStats {
        original_size: decompressed.len() as u64,
        compressed_size: data.len() as u64,
        compression_time_ms: 0,
        decompression_time_ms: elapsed.as_millis() as u64,
    };

    Ok((decompressed, stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_compression() {
        let data = b"Hello, World!";
        let compressed = compress(data, CompressionAlgorithm::None, 0).unwrap();
        assert_eq!(compressed, data);

        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_zstd_compression() {
        let data = b"Hello, World! ".repeat(100);
        let compressed = compress(&data, CompressionAlgorithm::Zstd, 3).unwrap();
        assert!(compressed.len() < data.len());

        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_lz4_compression() {
        let data = b"Hello, World! ".repeat(100);
        let compressed = compress(&data, CompressionAlgorithm::Lz4, 0).unwrap();
        assert!(compressed.len() < data.len());

        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_detect_zstd() {
        let data = b"Hello, World! ".repeat(100);
        let compressed = compress(&data, CompressionAlgorithm::Zstd, 3).unwrap();
        assert_eq!(CompressionAlgorithm::detect(&compressed), CompressionAlgorithm::Zstd);
    }

    #[test]
    fn test_detect_lz4() {
        let data = b"Hello, World! ".repeat(100);
        let compressed = compress(&data, CompressionAlgorithm::Lz4, 0).unwrap();
        assert_eq!(CompressionAlgorithm::detect(&compressed), CompressionAlgorithm::Lz4);
    }

    #[test]
    fn test_compression_stats() {
        let data = b"Hello, World! ".repeat(100);
        let (compressed, stats) = compress_with_stats(&data, CompressionAlgorithm::Zstd, 3).unwrap();

        assert_eq!(stats.original_size, data.len() as u64);
        assert_eq!(stats.compressed_size, compressed.len() as u64);
        assert!(stats.ratio() < 100.0);  // Should be compressed
        assert!(stats.space_saved() > 0);
    }

    #[test]
    fn test_round_trip() {
        let original = b"The quick brown fox jumps over the lazy dog ".repeat(50);

        // Test Zstd
        let compressed = compress(&original, CompressionAlgorithm::Zstd, 3).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);

        // Test LZ4
        let compressed = compress(&original, CompressionAlgorithm::Lz4, 0).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }
}
