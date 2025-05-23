//#![allow(unused)]
mod audio;
mod data;
mod models;

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

use alsa::pcm::Format;
use atomic_float::AtomicF64;
use circular_buffer::CircularBuffer;
#[cfg(feature = "audio")]
use crossbeam_channel::unbounded;
use flexi_logger::{with_thread, FileSpec, Logger};
#[cfg(feature = "audio")]
use hound::SampleFormat;
use log::{debug, info, warn};
use ndarray::{Array2, ArrayViewD};
use ort::inputs;
use parking_lot::Mutex;
use signal_hook::consts::SIGINT;
use signal_hook::iterator::Signals;

use audio::CaptureDevice;
use audio::CaptureDeviceError;
use tungstenite::connect;

#[cfg(feature = "audio")]
use self::audio::{AudioWriter, SEND_BUF_SIZE};

fn handle_capture_device_error(err: &CaptureDeviceError, status: &AtomicU8) {
    warn!("{err}");
    status.store(2, Ordering::Relaxed);
    thread::sleep(Duration::from_millis(100));
    //if !err.to_string().contains("(32)") {
    //    thread::sleep(Duration::from_millis(200));
    //}
}

fn main() {
    let home = match std::env::var("HOME") {
        Ok(var) => var,
        Err(err) => {
            warn!("Failed to load $HOME environmental variable: {err}\nChoosing current directory as working directory.");
            ".".to_owned()
        }
    };

    let andros_dir = format!("{home}/andros");
    let andros_dir = Path::new(&andros_dir);
    let andros_dir = match std::fs::create_dir_all(andros_dir) {
        Ok(()) => andros_dir,
        Err(err) => {
            warn!("Failed to create andros directory: {err}\nWriting data and logs to current directory.");
            Path::new(".")
        }
    };

    let ip: Option<(String, String, String)> = {
        let path = andros_dir.join("ip");
        let open = File::open(path);
        let ip = if let Ok(mut file) = open {
            let mut buf = String::new();
            match file.read_to_string(&mut buf) {
                Ok(_) => Some(buf),
                Err(e) => {
                    warn!("Failed to read ip from file: {e}");
                    None
                }
            }
        } else {
            warn!("Failed to open ip file: {}", open.unwrap_err());
            None
        };

        let path = andros_dir.join("mac");
        let open = File::open(path);
        let mac = if let Ok(mut file) = open {
            let mut buf = String::new();
            match file.read_to_string(&mut buf) {
                Ok(_) => Some(buf.to_string()),
                Err(e) => {
                    warn!("Failed to read mac from file: {e}");
                    None
                }
            }
        } else {
            warn!("Failed to open mac file: {}", open.unwrap_err());
            None
        };

        let path = andros_dir.join("post");
        let open = File::open(path);
        let post = if let Ok(mut file) = open {
            let mut buf = String::new();
            match file.read_to_string(&mut buf) {
                Ok(_) => Some(buf.to_string()),
                Err(e) => {
                    warn!("Failed to read post address from file: {e}");
                    None
                }
            }
        } else {
            warn!("Failed to open post address file: {}", open.unwrap_err());
            None
        };

        if let Some(ip) = ip {
            if let Some(mac) = mac {
                post.map(|post| (ip, mac, post))
            } else {
                None
            }
        } else {
            None
        }
    };

    let log_dir = &andros_dir.join("log");
    if !log_dir.exists() {
        std::fs::create_dir(log_dir).unwrap_or_else(|e| {
            warn!("Failed to create {} data directory: {e}", log_dir.display())
        });
    }
    let data_dir = &andros_dir.join("data");
    if !data_dir.exists() {
        std::fs::create_dir(data_dir).unwrap_or_else(|e| {
            warn!("Failed to create {} data directory: {e}", data_dir.display())
        });
    }

    for dir in ["i2s", "umc", "data", "clock_umc", "clock_i2s"] {
        let path = data_dir.join(dir);
        if !path.exists() {
            std::fs::create_dir(path)
                .unwrap_or_else(|e| warn!("Failed to create {dir} data directory: {e}"));
        }
    }

    Logger::try_with_env_or_str("info")
        .unwrap()
        .log_to_file(FileSpec::default().directory(log_dir))
        .duplicate_to_stderr(flexi_logger::Duplicate::All)
        .create_symlink(log_dir.join("current"))
        .format(with_thread)
        .use_utc()
        .start()
        .unwrap();

    let running = &AtomicBool::new(true);
    let i2s_status = &AtomicU8::new(0);
    let umc_status = &AtomicU8::new(0);
    let drone_detected = &AtomicBool::new(false);
    let drone_distance = &AtomicF64::new(0.0);
    let lat = &AtomicF64::new(0.0);
    let lon = &AtomicF64::new(0.0);
    let counter = &AtomicU32::new(0);

    thread::scope(|s| {
        let mut signals = Signals::new([SIGINT]).unwrap();
        s.spawn(move || {
            for sig in signals.forever() {
                if sig == signal_hook::consts::SIGINT {
                    running.store(false, Ordering::Relaxed);
                    println!();
                    break;
                }
            }
        });

        let i2s_max = Arc::new(Mutex::new(0i32));
        let umc_max = Arc::new(Mutex::new(0i32));

        #[cfg(feature = "audio")]
        let (i2s_s, i2s_r) = unbounded::<([i32; SEND_BUF_SIZE], i64)>();

        // Create the Andros I2S microphone capture thread
        thread::Builder::new()
            .stack_size(1024 * 1024 * 32)
            .name("i2s".to_owned())
            .spawn_scoped(s, {
                let i2s_max = i2s_max.clone();
                move || {
                    let i2s = CaptureDevice::new(
                        "hw:CARD=ANDROSi2s,DEV=1",
                        4,
                        192_000,
                        Format::s32(),
                        running,
                        i2s_status,
                        i2s_max,
                    );
                    while running.load(Ordering::Relaxed) {
                        match i2s.read(
                            #[cfg(feature = "audio")]
                            i2s_s.clone(),
                        ) {
                            Ok(()) => {}
                            Err(err) => handle_capture_device_error(&err, i2s_status),
                        };
                    }
                }
            })
            .unwrap();

        #[cfg(feature = "audio")]
        thread::Builder::new()
            .name("i2s_processor".to_owned())
            .spawn_scoped(s, {
                move || {
                    let wav_spec = hound::WavSpec {
                        channels: 4,
                        sample_rate: 192000,
                        bits_per_sample: 32,
                        sample_format: SampleFormat::Int,
                    };
                    let output_dir = data_dir.join("i2s");
                    let clock_dir = data_dir.join("clock_i2s");
                    let mut writer =
                        AudioWriter::new(output_dir, clock_dir, wav_spec, i2s_r).unwrap();
                    while running.load(Ordering::Relaxed) {
                        match writer.receive() {
                            Ok(_b) => {}
                            Err(err) => {
                                handle_capture_device_error(&err, umc_status);
                            }
                        };
                        if writer.time_to_write() {
                            writer = writer.write_wav().unwrap();
                        }
                    }
                }
            })
            .unwrap();

        #[cfg(feature = "audio")]
        let (umc_s, umc_r) = unbounded::<([i32; SEND_BUF_SIZE], i64)>();

        // Create the UMC microphone capture thread
        thread::Builder::new()
            .stack_size(1024 * 1024 * 32)
            .name("umc".to_owned())
            .spawn_scoped(s, {
                let umc_max = umc_max.clone();
                move || {
                    let umc = CaptureDevice::new(
                        "hw:CARD=U192k,DEV=0",
                        2,
                        48_000,
                        Format::s32(),
                        running,
                        umc_status,
                        umc_max,
                    );
                    while running.load(Ordering::Relaxed) {
                        match umc.read(
                            #[cfg(feature = "audio")]
                            umc_s.clone(),
                        ) {
                            Ok(()) => {}
                            Err(err) => handle_capture_device_error(&err, umc_status),
                        };
                    }
                }
            })
            .unwrap();

        #[cfg(feature = "audio")]
        thread::Builder::new()
            .name("umc_processor".to_owned())
            .spawn_scoped(s, {
                move || {
                    let wav_spec = hound::WavSpec {
                        channels: 2,
                        sample_rate: 48000,
                        bits_per_sample: 32,
                        sample_format: SampleFormat::Int,
                    };
                    let output_dir = data_dir.join("umc");
                    let clock_dir = data_dir.join("clock_umc");
                    let mut writer =
                        AudioWriter::new(output_dir, clock_dir, wav_spec, umc_r).unwrap();

                    let detection_model = models::load_onnx(andros_dir.join("detection.onnx"));
                    info!("Detection model loaded");

                    let location_model = models::load_onnx(andros_dir.join("location.onnx"));
                    info!("Location model loaded");

                    let mut detections: CircularBuffer<20, u8> = CircularBuffer::from([0; 20]);
                    let mut distances: CircularBuffer<20, f64> = CircularBuffer::new();

                    while running.load(Ordering::Relaxed) {
                        match writer.receive() {
                            Ok(buffer_full) => {
                                if buffer_full {
                                    let (_freqs, values) =
                                        models::process_samples(writer.buffer.iter());
                                    // if let Ok(x) = DenseMatrix::from_2d_vec(&vec![values]) {
                                    if let Ok(x) = Array2::from_shape_vec((1, values.len()), values)
                                    {
                                        if let Ok(outputs) =
                                            detection_model.run(inputs![x.clone()].unwrap())
                                        {
                                            let pred: ArrayViewD<i32> =
                                                outputs["variable"].try_extract_tensor().unwrap();
                                            let pred_len = pred.len();
                                            let pred =
                                                pred.into_shape_with_order(pred_len).unwrap();
                                            // detections.push_back(if pred[0] == 1 { 1 } else { 0 });
                                            detections.push_back(pred[0] as u8);
                                            let drone_predicted = detections.iter().sum::<u8>() > 1;
                                            drone_detected
                                                .store(drone_predicted, Ordering::Relaxed);
                                            debug!("Drone detected: {drone_predicted}");
                                        }
                                        if let Ok(outputs) = location_model.run(inputs![x].unwrap())
                                        {
                                            let pred: ArrayViewD<f64> =
                                                outputs["variable"].try_extract_tensor().unwrap();
                                            let pred_len = pred.len();
                                            let pred =
                                                pred.into_shape_with_order(pred_len).unwrap();
                                            distances.push_back(pred[0]);
                                            let distance = distances.iter().sum::<f64>() / 20.0;
                                            drone_distance.store(distance, Ordering::Relaxed);
                                            debug!("Drone distance: {distance}");
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                handle_capture_device_error(&err, umc_status);
                            }
                        };
                        if writer.time_to_write() {
                            writer = writer.write_wav().unwrap();
                        }
                    }
                }
            })
            .unwrap();

        #[cfg(feature = "audio")]
        thread::Builder::new()
            .name("drone_detection_sender".to_owned())
            .spawn_scoped(s, {
                let (ip, mac) = ip
                    .as_ref()
                    .map(|(ip, mac, _)| (ip.clone(), mac.clone()))
                    .unwrap_or_else(|| ("no_ip".to_owned(), "no_mac".to_owned()));
                move || {
                    let read_period = Duration::from_millis(50);

                    while running.load(Ordering::Relaxed) {
                        let (mut socket, _response) = match connect("ws://10.66.66.1:3012/socket") {
                            Ok(c) => c,
                            Err(e) => {
                                log::error!("Drone WebSocket connection error: {e}");
                                sleep(Duration::from_millis(1000));
                                continue;
                            }
                        };
                        log::info!("Drone WebSocket connected");

                        sleep(Duration::from_secs(1));

                        while running.load(Ordering::Relaxed) {
                            let start = Instant::now();
                            let counter = counter.load(Ordering::Relaxed) as f64;
                            let lat = lat.load(Ordering::Relaxed) / counter;
                            let lon = lon.load(Ordering::Relaxed) / counter;
                            match socket.send(tungstenite::Message::Text(
                                format!(
                                    "{mac}|{ip}|{lat}|{lon}|{}|{}",
                                    drone_detected.load(Ordering::Relaxed),
                                    drone_distance.load(Ordering::Relaxed)
                                )
                                .into(),
                            )) {
                                Ok(_) => {}
                                Err(err) => {
                                    log::error!("Error sending drone WebSocket message: {err}");
                                    break;
                                }
                            }
                            sleep(read_period.saturating_sub(start.elapsed()));
                        }
                    }
                }
            })
            .unwrap();

        let mut reader = data::Reader::new(
            #[cfg(feature = "sensors")]
            data_dir.join("data"),
            data_dir,
            i2s_status,
            umc_status,
            i2s_max,
            umc_max,
            drone_detected,
            drone_distance,
            lat,
            lon,
            counter,
        );
        reader.read(running, s, ip);
    });
    info!("Exited properly");
}
