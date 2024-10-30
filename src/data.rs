//#![allow(unused)]
use std::fs::File;
use std::io::BufWriter;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use ::serde::{Deserialize, Serialize};
use log::{error, info, warn};

use self::aht::Aht;
use self::bmp::Bmp;
use self::device_manager::{DeviceManager, Status, Statuses};
use self::gps::Gps;
use self::imu::Imu;
use self::wind::Wind;

pub mod aht;
pub mod bmp;
pub mod device_manager;
pub mod gps;
pub mod imu;
pub mod wind;

pub trait Device {
    type Data;
    type Error;

    fn get_data(&mut self) -> Result<Self::Data, Self::Error>;
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Data {
    gps: Option<gps::Data>,
    aht: Option<aht::Data>,
    wind: Option<wind::Data>,
    imu: Option<imu::Data>,
    bmp: Option<bmp::Data>,
}

pub struct Reader {
    pub device_manager: DeviceManager,
    pub path: &'static str,
    pub read_period: Duration,
}

impl Reader {
    pub fn new() -> Self {
        Self {
            device_manager: DeviceManager::new(),
            path: "../data/data",
            read_period: Duration::from_secs(5),
        }
    }

    fn handle_gps_data_error(&mut self, err: &gps::Error) {
        match err {
            //gps::Error::Uart(_) => todo!(),
            //gps::Error::Nmea(_) => todo!(),
            //gps::Error::DataOverflow => todo!(),
            //gps::Error::Io(_) => todo!(),
            gps::Error::NoData => self.device_manager.statuses.gps = Status::NoData,
            //gps::Error::InvalidNmeaString => todo!(),
            _ => {}
        }
        error!("GPS data error: {err}");
    }

    fn handle_gps_init_error(&mut self, err: &gps::Error) {
        match err {
            gps::Error::Uart(uart_err) => {
                warn!("GPS init failed: {uart_err}");
                self.device_manager.statuses.gps = Status::Disconnected;
            }
            _ => unreachable!(),
        }
    }

    fn handle_aht_data_error(&mut self, err: &aht::Error) {
        //match err {
        //    aht::Error::I2c(_) => todo!(),
        //    aht::Error::Aht(_) => todo!(),
        //}
        error!("AHT10 data error: {err}");
    }

    fn handle_aht_init_error(&mut self, err: &aht::Error) {
        match err {
            aht::Error::I2c(i2c_err) => {
                warn!("AHT10 init failed: {i2c_err}");
                self.device_manager.statuses.aht = Status::Disconnected;
            }
            aht::Error::Aht(_) => unreachable!(),
        }
    }

    fn handle_wind_data_error(&mut self, err: &wind::Error) {
        //match err {
        //    wind::Error::NoData => todo!(),
        //    wind::Error::Uart(_) => todo!(),
        //}
        error!("Wind data error: {err}");
    }

    fn handle_wind_init_error(&mut self, err: &wind::Error) {
        match err {
            wind::Error::Uart(uart_err) => {
                warn!("Wind init failed: {uart_err}");
                self.device_manager.statuses.wind = Status::Disconnected;
            }
            wind::Error::NoData => unreachable!(),
        }
    }

    fn handle_imu_data_error(&mut self, err: &imu::Error) {
        //match err {
        //    imu::Error::Mpu(_) => todo!(),
        //    imu::Error::Bus(_) => todo!(),
        //    imu::Error::I2c(_) => todo!(),
        //}
        error!("IMU data error: {err}");
    }

    fn handle_imu_init_error(&mut self, err: &imu::Error) {
        match err {
            imu::Error::Mpu(err) => {
                warn!("IMU init failed: {err:?}");
            }
            imu::Error::Bus(err) => {
                warn!("IMU init failed: {err:?}");
            }
            imu::Error::I2c(err) => {
                warn!("IMU init failed: {err}");
            }
        }
        self.device_manager.statuses.imu = Status::Disconnected;
    }

    fn handle_bmp_data_error(&mut self, err: &bmp::Error) {
        //match err {
        //    bmp::Error::Bmp(_) => todo!(),
        //}
        error!("BMP280 data error: {err}");
    }

    fn handle_bmp_init_error(&mut self, err: &bmp::Error) {
        match err {
            bmp::Error::Bmp(err) => {
                warn!("BMP init failed: {err}");
                self.device_manager.statuses.bmp = Status::Disconnected;
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    pub fn read(&mut self, running: &Arc<AtomicBool>) {
        while running.load(std::sync::atomic::Ordering::Relaxed) {
            let start = Instant::now();

            let mut data = Data::default();

            if let Some(wind) = self.device_manager.wind.as_mut() {
                match wind.get_data() {
                    Ok(d) => data.wind = Some(d),
                    Err(e) => {
                        self.handle_wind_data_error(&e);
                    }
                }
            } else {
                match self.device_manager.try_set_wind() {
                    Ok(()) => {
                        info!("Wind device initiated");
                    }
                    Err(e) => {
                        self.handle_wind_init_error(&e);
                    }
                }
            }

            if let Some(gps) = self.device_manager.gps.as_mut() {
                match gps.get_data() {
                    Ok(d) => data.gps = Some(d),
                    Err(e) => {
                        self.handle_gps_data_error(&e);
                    }
                }
            } else {
                match self.device_manager.try_set_gps() {
                    Ok(()) => {
                        info!("GPS device initiated");
                    }
                    Err(e) => {
                        self.handle_gps_init_error(&e);
                    }
                }
            }

            if let Some(aht) = self.device_manager.aht.as_mut() {
                match aht.get_data() {
                    Ok(d) => data.aht = Some(d),
                    Err(e) => {
                        self.handle_aht_data_error(&e);
                    }
                }
            } else {
                match self.device_manager.try_set_aht() {
                    Ok(()) => {
                        info!("AHT10 device initiated");
                    }
                    Err(e) => {
                        self.handle_aht_init_error(&e);
                    }
                }
            }

            if let Some(imu) = self.device_manager.imu.as_mut() {
                match imu.get_data() {
                    Ok(d) => data.imu = Some(d),
                    Err(e) => {
                        self.handle_imu_data_error(&e);
                    }
                }
            } else {
                match self.device_manager.try_set_imu() {
                    Ok(()) => {
                        info!("IMU device initiated");
                    }
                    Err(e) => {
                        self.handle_imu_init_error(&e);
                    }
                }
            }

            if let Some(bmp) = self.device_manager.bmp.as_mut() {
                match bmp.get_data() {
                    Ok(d) => data.bmp = Some(d),
                    Err(e) => {
                        self.handle_bmp_data_error(&e);
                    }
                }
            } else {
                match self.device_manager.try_set_bmp() {
                    Ok(()) => {
                        info!("BMP280 device initiated");
                    }
                    Err(e) => {
                        self.handle_bmp_init_error(&e);
                    }
                }
            }

            let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
            let path = format!("{}/{nanos}.json", self.path);
            match File::create(&path) {
                Ok(file) => {
                    #[derive(Serialize, Deserialize)]
                    struct JsonData {
                        statuses: Statuses,
                        data: Data,
                    }
                    let writer = BufWriter::new(file);
                    match serde_json::to_writer(
                        writer,
                        &JsonData {
                            statuses: self.device_manager.statuses,
                            data,
                        },
                    ) {
                        Ok(()) => {}
                        Err(e) => {
                            warn!("Failed to serialize data to json: {e}");
                        }
                    };
                }
                Err(e) => {
                    warn!("Failed to create data file: {e}");
                }
            };

            thread::sleep(self.read_period.saturating_sub(start.elapsed()));
        }
    }
}
