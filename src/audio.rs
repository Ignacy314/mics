use alsa::pcm::IO;
#[cfg(feature = "audio")]
use hound::{SampleFormat, WavSpec, WavWriter};
use parking_lot::Mutex;
#[cfg(feature = "audio")]
use std::fs::File;
use std::io;
#[cfg(feature = "audio")]
use std::io::BufWriter;
#[cfg(feature = "audio")]
use std::io::Write;
#[cfg(feature = "audio")]
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
#[cfg(feature = "audio")]
use std::time::Duration;
use std::time::Instant;

use alsa::{
    pcm::{Access, Format, HwParams, PCM},
    Direction, Error, ValueOr,
};

#[cfg(feature = "audio")]
use crate::AUDIO_FILE_DURATION;

#[derive(thiserror::Error, Debug)]
pub enum CaptureDeviceError {
    //#[error("Format unimplemented: {0}")]
    //FormatUnimplemented(Format),
    #[error("Alsa error: {0}")]
    Alsa(#[from] alsa::Error),
    #[error("Hound error: {0}")]
    Hound(#[from] hound::Error),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

pub const BUF_SIZE: usize = 128 * 1024;

pub struct Timers {
    #[cfg(feature = "audio")]
    pub clock: Instant,
    #[cfg(feature = "audio")]
    pub file_start: Instant,
    pub last_read: Instant,
}

#[cfg(feature = "audio")]
pub struct Writers {
    pub wav: WavWriter<BufWriter<File>>,
    pub wav_file: String,
    pub clock: BufWriter<File>,
}

//struct Dev {
//    pcm: PCM,
//    buf: [i32; BUF_SIZE],
//}
//
//impl Dev {
//    fn new(pcm: PCM) -> Self {
//        //let io = match &format {
//        //    Format::S32LE | Format::S32BE => pcm.io_i32()?,
//        //    default => return Err(CaptureDeviceError::FormatUnimplemented(*default)),
//        //};
//
//        Self {
//            pcm,
//            buf: [0i32; BUF_SIZE],
//        }
//    }
//}

pub struct CaptureDevice<'a> {
    pub device_name: String,
    channels: u32,
    samplerate: u32,
    format: Format,
    #[cfg(feature = "audio")]
    pub output_dir: PathBuf,
    pub status: &'a AtomicU8,
    #[cfg(feature = "audio")]
    pub wav_spec: WavSpec,
    #[cfg(feature = "audio")]
    pub writers: Writers,
    pub timers: Timers,
    pub sample: usize,
}

#[allow(clippy::too_many_arguments)]
impl<'a> CaptureDevice<'a> {
    pub fn new(
        device_name: &str,
        channels: u32,
        samplerate: u32,
        format: Format,
        #[cfg(feature = "audio")] output_dir: PathBuf,
        #[cfg(feature = "audio")] clock_dir: PathBuf,
        status: &'a AtomicU8,
    ) -> Result<Self, CaptureDeviceError> {
        #[cfg(feature = "audio")]
        let wav_spec = hound::WavSpec {
            channels: channels as u16,
            sample_rate: samplerate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Int,
        };

        #[cfg(feature = "audio")]
        let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
        #[cfg(feature = "audio")]
        let wav_path = output_dir.join(format!("{nanos}.wav"));
        #[cfg(feature = "audio")]
        let wav_writer = WavWriter::create(wav_path.clone(), wav_spec)?;
        #[cfg(feature = "audio")]
        let clock_path = clock_dir.join(format!("{nanos}.csv"));
        #[cfg(feature = "audio")]
        let mut clock_writer = BufWriter::new(File::create(clock_path)?);
        #[cfg(feature = "audio")]
        writeln!(clock_writer, "time,file,sample")?;

        #[cfg(feature = "audio")]
        let file_start = Instant::now();
        #[cfg(feature = "audio")]
        let clock = Instant::now();
        let last_read = Instant::now();
        Ok(Self {
            device_name: device_name.to_owned(),
            #[cfg(feature = "audio")]
            output_dir,
            channels,
            format,
            samplerate,
            status,
            #[cfg(feature = "audio")]
            wav_spec,
            #[cfg(feature = "audio")]
            writers: Writers {
                clock: clock_writer,
                wav_file: wav_path.file_name().unwrap().to_str().unwrap().to_owned(),
                wav: wav_writer,
            },
            timers: Timers {
                #[cfg(feature = "audio")]
                clock,
                #[cfg(feature = "audio")]
                file_start,
                last_read,
            },
            sample: 0,
        })
    }

    pub fn init_device(&self) -> Result<PCM, Error> {
        let pcm = PCM::new(&self.device_name, Direction::Capture, true)?;
        {
            let hwp = HwParams::any(&pcm)?;
            hwp.set_channels(self.channels)?;
            hwp.set_rate(self.samplerate, ValueOr::Nearest)?;
            hwp.set_format(self.format)?;
            hwp.set_access(Access::RWInterleaved)?;
            hwp.set_buffer_size_near(131072)?;
            //info!("{hwp:?}");
            pcm.hw_params(&hwp)?;
        }
        pcm.prepare()?;
        pcm.start()?;
        Ok(pcm)
    }

    pub fn read(
        &mut self,
        io: &IO<'_, i32>,
        buf: &mut [i32; BUF_SIZE],
        max_read: &Arc<Mutex<i32>>,
    ) -> Result<bool, CaptureDeviceError> {
        #[cfg(feature = "audio")]
        if self.timers.clock.elapsed() >= Duration::from_secs(1) {
            let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
            self.timers.clock = self
                .timers
                .clock
                .checked_add(Duration::from_secs(1))
                .unwrap();
            writeln!(self.writers.clock, "{nanos},{},{}", self.sample, self.writers.wav_file)?;
            self.writers.clock.flush()?;
        }

        match io.readi(buf) {
            Ok(s) => {
                let n = s * self.channels as usize;
                let mut max_sample = i32::MIN;
                let mut zeros = 0;
                //let samples = buf.len();
                for sample in &buf[0..n] {
                    if sample.abs() > max_sample {
                        max_sample = *sample;
                    }
                    if sample.trailing_zeros() >= 28 || sample.leading_zeros() >= 28 {
                        zeros += 1;
                    }
                    #[cfg(feature = "audio")]
                    self.writers.wav.write_sample(*sample)?;
                }
                self.sample += s;
                let mut saved_max = max_read.lock();
                *saved_max = saved_max.max(max_sample);
                if zeros < n {
                    self.timers.last_read = Instant::now();
                }
            }
            Err(err) => {
                if err.errno() != 11 {
                    return Err(err.into());
                }
            }
        }
        if self.timers.last_read.elapsed().as_secs() >= 2 {
            self.status.store(1, Ordering::Relaxed);
        }
        #[cfg(feature = "audio")]
        if self.timers.file_start.elapsed() >= AUDIO_FILE_DURATION {
            self.timers.file_start = self
                .timers
                .file_start
                .checked_add(AUDIO_FILE_DURATION)
                .unwrap();
            self.sample = 0;
            return Ok(true);
        }

        Ok(false)
    }
}
