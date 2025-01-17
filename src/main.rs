#![allow(unused)]
mod audio;
mod data;

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use alsa::pcm::Format;
use flexi_logger::{with_thread, FileSpec, Logger};
use log::{info, warn};
use parking_lot::Mutex;
use rppal::gpio::Gpio;
use signal_hook::consts::SIGINT;
use signal_hook::iterator::Signals;

use self::audio::CaptureDevice;
use self::audio::CaptureDeviceError;

const AUDIO_FILE_DURATION: Duration = Duration::from_secs(10);

fn handle_capture_device_error(err: &CaptureDeviceError, status: &AtomicU8) {
    warn!("{err}");
    status.store(2, Ordering::Relaxed);
    thread::sleep(Duration::from_secs(1));
}

#[allow(clippy::too_many_lines)]
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

    let ip: Option<(String, String)> = {
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
                Ok(_) => Some(buf[(buf.len() - 6)..].to_string()),
                Err(e) => {
                    warn!("Failed to read mac from file: {e}");
                    None
                }
            }
        } else {
            warn!("Failed to open mac file: {}", open.unwrap_err());
            None
        };

        if let Some(ip) = ip {
            mac.map(|mac| (ip, mac))
        } else {
            None
        }
    };

    let log_dir = &andros_dir.join("log");
    let data_dir = &andros_dir.join("data");

    for dir in ["i2s", "umc", "data"] {
        let path = data_dir.join(dir);
        if !path.exists() {
            std::fs::create_dir(path)
                .unwrap_or_else(|e| warn!("Failed to create {dir} data directory: {e}"));
        }
    }

    //std::fs::create_dir(data_dir.clone())
    //    .unwrap_or_else(|e| warn!("Failed to create data directory: {e}"));
    //std::fs::create_dir(data_dir.clone().join("i2s"))
    //    .unwrap_or_else(|e| warn!("Failed to create i2s data directory: {e}"));
    //std::fs::create_dir(data_dir.clone().join("umc"))
    //    .unwrap_or_else(|e| warn!("Failed to create umc data directory: {e}"));
    //std::fs::create_dir(data_dir.clone().join("data"))
    //    .unwrap_or_else(|e| warn!("Failed to create sensor data directory: {e}"));

    Logger::try_with_env_or_str("info")
        .unwrap()
        .log_to_file(FileSpec::default().directory(log_dir))
        .duplicate_to_stderr(flexi_logger::Duplicate::All)
        .print_message()
        .create_symlink(log_dir.join("current"))
        .format(with_thread)
        .start()
        .unwrap();

    let running = &AtomicBool::new(true);
    let i2s_status = &AtomicU8::new(0);
    let umc_status = &AtomicU8::new(0);

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

        let gpio = Gpio::new().unwrap();
        let mut pps_pin = gpio.get(13).unwrap().into_input_pulldown();

        //let (tx, rx) = unbounded();

        let i2s_pps = Arc::new(Mutex::new((false, 0i64)));
        let umc_pps = Arc::new(Mutex::new((false, 0i64)));
        //let i2s_pps_rdy = &AtomicBool::new(false);
        //let i2s_pps_data = &AtomicI64::new(0);

        pps_pin
            .set_async_interrupt(
                rppal::gpio::Trigger::RisingEdge,
                Some(Duration::from_millis(5)),
                {
                    let i2s_pps = i2s_pps.clone();
                    let umc_pps = umc_pps.clone();
                    move |_| {
                        let now = chrono::Utc::now();
                        info!("PPS at UTC {now}");
                        let nanos = now.timestamp_nanos_opt().unwrap();
                        *i2s_pps.lock() = (true, nanos);
                        *umc_pps.lock() = (true, nanos);
                        //tx.send(nanos).unwrap();
                    }
                },
            )
            .unwrap();

        // Create the Andros I2S microphone capture thread
        thread::Builder::new()
            .name("i2s".to_owned())
            .spawn_scoped(s, {
                //let rx = rx.clone();
                //let i2s_pps = i2s_pps.clone();
                move || {
                    let i2s = CaptureDevice::new(
                        "hw:CARD=ANDROSi2s,DEV=1",
                        4,
                        192_000,
                        Format::s32(),
                        data_dir.join("i2s"),
                        running,
                        i2s_status,
                        i2s_pps,
                    );
                    while running.load(Ordering::Relaxed) {
                        match i2s.read(AUDIO_FILE_DURATION) {
                            Ok(()) => {}
                            Err(err) => handle_capture_device_error(&err, i2s_status),
                        };
                    }
                }
            })
            .unwrap();

        // Create the UMC microphone capture thread
        thread::Builder::new()
            .name("umc".to_owned())
            .spawn_scoped(s, {
                //let rx = rx.clone();
                //let umc_pps = umc_pps.clone();
                move || {
                    let umc = CaptureDevice::new(
                        "hw:CARD=U192k,DEV=0",
                        2,
                        48_000,
                        Format::s32(),
                        data_dir.join("umc"),
                        running,
                        umc_status,
                        umc_pps,
                    );
                    while running.load(Ordering::Relaxed) {
                        match umc.read(AUDIO_FILE_DURATION) {
                            Ok(()) => {}
                            Err(err) => handle_capture_device_error(&err, umc_status),
                        };
                    }
                }
            })
            .unwrap();

        let mut reader = data::Reader::new(data_dir.join("data"), data_dir, i2s_status, umc_status);
        reader.read(running, s, ip);
    });
    info!("Exited properly");
}
