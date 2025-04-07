#[cfg(feature = "audio")]
use std::fs::File;
use std::io;
#[cfg(feature = "audio")]
use std::io::BufWriter;
#[cfg(feature = "audio")]
use std::io::Write;
#[cfg(feature = "audio")]
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use alsa::{
    pcm::{Access, Format, HwParams, PCM},
    Direction, Error, ValueOr,
};
use circular_buffer::CircularBuffer;
#[cfg(feature = "audio")]
use crossbeam_channel::Receiver;
use crossbeam_channel::RecvError;
#[cfg(feature = "audio")]
use crossbeam_channel::Sender;
#[cfg(feature = "audio")]
use hound::{WavSpec, WavWriter};
use log::info;
#[cfg(feature = "audio")]
use log::warn;
use parking_lot::Mutex;

#[cfg(feature = "audio")]
pub const AUDIO_FILE_DURATION: Duration = Duration::from_secs(60);
#[cfg(feature = "audio")]
pub const SEND_BUF_SIZE: usize = 4800;

#[cfg(feature = "audio")]
struct AudioSender {
    buffer: ([i32; SEND_BUF_SIZE], i64),
    index: usize,
    sender: Sender<([i32; SEND_BUF_SIZE], i64)>,
}

#[cfg(feature = "audio")]
impl AudioSender {
    fn new(sender: Sender<([i32; SEND_BUF_SIZE], i64)>) -> Self {
        Self {
            sender,
            index: 0,
            buffer: ([0i32; SEND_BUF_SIZE], 0),
        }
    }

    fn set_timestamp(&mut self, ts: i64) {
        self.buffer.1 = ts;
    }

    fn send_sample(&mut self, sample: i32) {
        self.buffer.0[self.index] = sample;
        self.index += 1;
        if self.index == SEND_BUF_SIZE {
            match self.sender.send(self.buffer) {
                Ok(()) => {}
                Err(_err) => {
                    warn!("Failed to send audio data. Buffer overrun");
                }
            };
            self.index = 0;
            //false
        }
        //else {
        //    self.index == SEND_BUF_SIZE - 1
        //}
    }
}

#[cfg(feature = "audio")]
pub struct AudioWriter {
    pub wav_writer: WavWriter<BufWriter<File>>,
    pub clock_writer: BufWriter<File>,
    pub wav_file: String,
    pub output_dir: PathBuf,
    inc: u32,
    pub sample: u64,
    pub file_sample: u32,
    pub file_start: Instant,
    pub clock: Instant,
    pub wav_spec: WavSpec,
    pub receiver: Receiver<([i32; SEND_BUF_SIZE], i64)>,
    pub buffer: CircularBuffer<8192, i32>,
    // pub buffer_index: usize,
}

#[cfg(feature = "audio")]
impl AudioWriter {
    pub fn new(
        output_dir: PathBuf,
        clock_dir: PathBuf,
        wav_spec: hound::WavSpec,
        receiver: Receiver<([i32; SEND_BUF_SIZE], i64)>,
    ) -> Result<Self, CaptureDeviceError> {
        let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
        let path = output_dir.join(format!("{nanos}.wav"));
        let writer = WavWriter::create(path.clone(), wav_spec)?;

        let clock_path = clock_dir.join(format!("{nanos}.csv"));
        let mut clock_writer = BufWriter::new(File::create(clock_path)?);
        writeln!(clock_writer, "time,sample,file_sample,file")?;

        Ok(Self {
            wav_file: path.file_name().unwrap().to_str().unwrap().to_string(),
            wav_writer: writer,
            clock_writer,
            output_dir,
            inc: SEND_BUF_SIZE as u32 / wav_spec.channels as u32,
            sample: 0,
            file_sample: 0,
            file_start: Instant::now(),
            clock: Instant::now(),
            wav_spec,
            receiver,
            buffer: CircularBuffer::new(),
            // buffer_index: 0,
        })
    }

    pub fn receive(&mut self) -> Result<bool, CaptureDeviceError> {
        let (buf, ts) = self.receiver.recv()?;
        if self.clock.elapsed() >= Duration::from_secs(1) {
            self.write_clock(ts)?;
        }
        for s in buf {
            self.write_sample(s)?;
        }
        for s in buf.iter().step_by(2) {
            // TODO figure out what to do with 8192 and 2400
            self.buffer.push_back(*s);
            // self.buffer[self.buffer_index] = *s;
            // self.buffer_index += 1;
        }
        self.inc_sample();
        Ok(self.buffer.is_full())
        // if self.buffer.is_full() {
        //
        // }
        // if self.buffer_index == 8192 {
        //     self.buffer_index = 0;
        //     Ok(true)
        // } else {
        //     Ok(false)
        // }
    }

    pub fn time_to_write(&self) -> bool {
        self.file_start.elapsed() >= AUDIO_FILE_DURATION
    }

    pub fn write_clock(&mut self, ts: i64) -> Result<(), CaptureDeviceError> {
        self.clock = self.clock.checked_add(Duration::from_secs(1)).unwrap();
        //let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
        writeln!(self.clock_writer, "{ts},{},{},{}", self.sample, self.file_sample, self.wav_file)?;
        self.clock_writer.flush()?;
        Ok(())
    }

    pub fn write_wav(mut self) -> Result<Self, CaptureDeviceError> {
        self.file_start = self.file_start.checked_add(AUDIO_FILE_DURATION).unwrap();
        self.wav_writer.finalize()?;
        let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
        let path = self.output_dir.join(format!("{nanos}.wav"));
        self.wav_writer = WavWriter::create(path.clone(), self.wav_spec)?;
        self.wav_file = path.file_name().unwrap().to_str().unwrap().to_string();
        self.file_sample = 0;
        Ok(self)
    }

    pub fn write_sample(&mut self, sample: i32) -> Result<(), CaptureDeviceError> {
        self.wav_writer.write_sample(sample)?;
        Ok(())
    }

    pub fn inc_sample(&mut self) {
        self.file_sample += self.inc;
        self.sample += self.inc as u64;
    }
}

#[derive(thiserror::Error, Debug)]
pub enum CaptureDeviceError {
    #[error("Format unimplemented: {0}")]
    FormatUnimplemented(Format),
    #[error("Alsa error: {0}")]
    Alsa(#[from] alsa::Error),
    #[error("Hound error: {0}")]
    Hound(#[from] hound::Error),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Receive error: {0}")]
    Receive(#[from] RecvError),
}

pub struct CaptureDevice<'a> {
    device_name: String,
    channels: u32,
    samplerate: u32,
    format: Format,
    running: &'a AtomicBool,
    status: &'a AtomicU8,
    max_read: Arc<Mutex<i32>>,
}

impl<'a> CaptureDevice<'a> {
    pub fn new(
        device_name: &str,
        channels: u32,
        samplerate: u32,
        format: Format,
        running: &'a AtomicBool,
        status: &'a AtomicU8,
        max_read: Arc<Mutex<i32>>,
    ) -> Self {
        Self {
            device_name: device_name.to_owned(),
            channels,
            samplerate,
            format,
            running,
            status,
            max_read,
        }
    }

    fn init_device(&self) -> Result<PCM, Error> {
        let pcm = PCM::new(&self.device_name, Direction::Capture, true)?;
        {
            let hwp = HwParams::any(&pcm)?;
            hwp.set_channels(self.channels)?;
            hwp.set_rate(self.samplerate, ValueOr::Nearest)?;
            hwp.set_format(self.format)?;
            hwp.set_access(Access::RWInterleaved)?;
            let buf_size = hwp.get_buffer_size_max()?;
            hwp.set_buffer_size(buf_size)?;
            pcm.hw_params(&hwp)?;
        }
        pcm.prepare()?;
        pcm.start()?;
        Ok(pcm)
    }

    pub fn read(
        &self,
        #[cfg(feature = "audio")] sender: Sender<([i32; SEND_BUF_SIZE], i64)>,
    ) -> Result<(), CaptureDeviceError> {
        let pcm = self.init_device()?;
        let io = match &self.format {
            Format::S32LE | Format::S32BE => pcm.io_i32()?,
            default => return Err(CaptureDeviceError::FormatUnimplemented(*default)),
        };

        let mut buf = [0i32; 1024 * 128];

        #[cfg(feature = "audio")]
        let mut sender = AudioSender::new(sender);

        let mut last_read = Instant::now();
        info!("start audio read");
        while self.running.load(Ordering::Relaxed) {
            let start = Instant::now();

            #[cfg(feature = "audio")]
            let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
            match io.readi(&mut buf) {
                Ok(s) => {
                    #[cfg(feature = "audio")]
                    sender.set_timestamp(nanos);
                    let n = s * self.channels as usize;
                    let mut max_sample = i32::MIN;
                    let mut zeros = 0;
                    for sample in &buf[0..n] {
                        if sample.abs() > max_sample {
                            max_sample = *sample;
                        }
                        if sample.trailing_zeros() >= 28 || sample.leading_zeros() >= 28 {
                            zeros += 1;
                        }
                        #[cfg(feature = "audio")]
                        sender.send_sample(*sample);
                        //if sender.send_sample(*sample) {
                        //    sender.set_timestamp(
                        //        nanos
                        //            - ((i as u32 + 1 - self.channels) as f64
                        //                / self.channels as f64
                        //                / self.samplerate as f64
                        //                * 1e9)
                        //                .round() as i64,
                        //    );
                        //}
                    }
                    if zeros < n {
                        last_read = Instant::now();
                    }
                    let mut saved_max = self.max_read.lock();
                    *saved_max = saved_max.max(max_sample);
                }
                Err(err) => {
                    if err.errno() != 11 {
                        info!("ALSA try recover from: {err}");
                        pcm.try_recover(err, false)?;
                    }
                }
            }

            if last_read.elapsed().as_secs() >= 2 {
                self.status.store(1, Ordering::Relaxed);
            }

            thread::sleep(Duration::from_millis(1).saturating_sub(start.elapsed()));
        }

        Ok(())
    }
}
