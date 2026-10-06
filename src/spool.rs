//! Disk-backed PCM handoff. Only sample counts cross the synchronization boundary.
//! A slow engine never holds a lock or stops the decoder from draining ffmpeg.
use anyhow::{Context, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
};

const BLOCK_SAMPLES: usize = 16_384;

#[derive(Default)]
struct Progress {
    samples: usize,
    closed: bool,
    cancelled: bool,
}
#[derive(Default)]
struct Shared {
    progress: Mutex<Progress>,
    changed: Condvar,
}

pub struct Spool(PathBuf);
pub struct Writer {
    file: File,
    shared: Arc<Shared>,
}
pub struct Reader {
    file: File,
    shared: Arc<Shared>,
    consumed: usize,
}

impl Spool {
    /// Use the disk-backed media workspace, not a potentially RAM-backed /tmp.
    pub fn create(directory: &Path) -> Result<(Self, Writer, Reader)> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = directory.join(format!(".audio-{}-{nonce}.f32le", std::process::id()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let spool = Self(path);
        let reader = File::open(&spool.0)?;
        let shared = Arc::new(Shared::default());
        Ok((
            spool,
            Writer {
                file,
                shared: shared.clone(),
            },
            Reader {
                file: reader,
                shared,
                consumed: 0,
            },
        ))
    }

    pub fn keep(&self, destination: &Path) -> Result<()> {
        fs::copy(&self.0, destination).context("cannot retain decoded audio")?;
        Ok(())
    }
}
impl Drop for Spool {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.0) {
            eprintln!(
                "warning: cannot remove audio spool {}: {error}",
                self.0.display()
            );
        }
    }
}
impl Writer {
    pub fn write(&mut self, block: &[f32]) -> Result<bool> {
        if self.shared.progress.lock().unwrap().cancelled {
            return Ok(false);
        }
        let bytes: Vec<_> = block
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        self.file
            .write_all(&bytes)
            .context("cannot spool decoded audio")?;
        // Publish only complete writes. The reader never sees a partial f32 sample.
        self.shared.progress.lock().unwrap().samples += block.len();
        self.shared.changed.notify_one();
        Ok(true)
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.shared.progress.lock().unwrap().closed = true;
        self.shared.changed.notify_one();
    }
}
impl Reader {
    pub fn next(&mut self) -> Result<Option<Vec<f32>>> {
        let mut progress = self.shared.progress.lock().unwrap();
        while progress.samples == self.consumed && !progress.closed {
            progress = self.shared.changed.wait(progress).unwrap();
        }
        let samples = (progress.samples - self.consumed).min(BLOCK_SAMPLES);
        drop(progress);
        if samples == 0 {
            return Ok(None);
        }
        let mut bytes = vec![0; samples * 4];
        self.file
            .read_exact(&mut bytes)
            .context("cannot read decoded audio spool")?;
        self.consumed += samples;
        Ok(Some(
            bytes
                .chunks_exact(4)
                .map(|bytes| f32::from_le_bytes(bytes.try_into().expect("four-byte sample")))
                .collect(),
        ))
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        self.shared.progress.lock().unwrap().cancelled = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_finishes_while_consumer_is_stalled_and_keep_is_exact() {
        let directory =
            std::env::temp_dir().join(format!("scribe-spool-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let (spool, mut writer, mut reader) = Spool::create(&directory).unwrap();
        let (done, finished) = std::sync::mpsc::channel();
        let producer = std::thread::spawn(move || {
            // One hour of PCM, much larger than the bounded read blocks.
            let block = vec![0.25; BLOCK_SAMPLES];
            for _ in 0..3_516 {
                assert!(writer.write(&block).unwrap());
            }
            drop(writer);
            done.send(()).unwrap();
        });
        // No consumer reads until the producer completes. A bounded PCM queue would deadlock.
        finished
            .recv_timeout(std::time::Duration::from_secs(25))
            .unwrap();
        producer.join().unwrap();
        let mut samples = 0;
        while let Some(block) = reader.next().unwrap() {
            assert!(block.len() <= BLOCK_SAMPLES);
            assert!(block.iter().all(|sample| *sample == 0.25));
            samples += block.len();
        }
        assert_eq!(samples, 57_606_144);
        let kept = directory.join("audio.f32le");
        spool.keep(&kept).unwrap();
        assert_eq!(fs::metadata(&kept).unwrap().len(), 230_424_576);
        let mut first = [0; 4];
        File::open(&kept).unwrap().read_exact(&mut first).unwrap();
        assert_eq!(first, [0, 0, 128, 62]);
        drop(reader);
        drop(spool);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn ffmpeg_drains_before_timeout_without_any_engine_reads() {
        let directory =
            std::env::temp_dir().join(format!("scribe-spool-ffmpeg-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source.wav");
        let generated = std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440",
                "-t",
                "60",
                "-ar",
                "16000",
                "-ac",
                "1",
                "-y",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(generated.success());
        let (spool, mut writer, mut reader) = Spool::create(&directory).unwrap();
        let (done, finished) = std::sync::mpsc::channel();
        let producer = std::thread::spawn(move || {
            let result =
                crate::audio::decode_blocks(source.as_os_str(), false, 0, Some(60.0), |block| {
                    writer.write(&block).unwrap()
                });
            drop(writer);
            done.send(result).unwrap();
        });
        // The consumer does not run at all. ffmpeg must finish in less than its 30 s
        // network timeout, even if the engine stays paused for a minute or longer.
        assert_eq!(
            finished
                .recv_timeout(std::time::Duration::from_secs(25))
                .unwrap()
                .unwrap(),
            960_000
        );
        producer.join().unwrap();
        let mut samples = 0;
        while let Some(block) = reader.next().unwrap() {
            samples += block.len();
        }
        assert_eq!(samples, 960_000);
        drop(reader);
        drop(spool);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn dropped_consumer_cancels_decoder_and_failed_writer_releases_reader() {
        let directory =
            std::env::temp_dir().join(format!("scribe-spool-cancel-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let (spool, mut writer, reader) = Spool::create(&directory).unwrap();
        drop(reader);
        assert!(!writer.write(&[1.0]).unwrap());
        drop(writer);
        drop(spool);
        let (spool, writer, mut reader) = Spool::create(&directory).unwrap();
        drop(writer);
        assert!(reader.next().unwrap().is_none());
        drop(reader);
        drop(spool);
        fs::remove_dir_all(directory).unwrap();
    }
}
