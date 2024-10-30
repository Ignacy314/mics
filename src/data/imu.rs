//#![allow(unused)]
use std::f32::consts::PI;
use std::fmt::Debug;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use log::{info, warn};
use mpu9250::Mpu9250;
use serde::{Deserialize, Serialize};

use super::Device;

pub struct Imu {
    device: Mpu9250<mpu9250::I2cDevice<rppal::i2c::I2c>, mpu9250::Marg>,
    mag_coeffs: [f32; 3],
    north_vector: [f32; 3],
    gyro_data: Vec<f32>,
    mag_data: [Vec<f32>; 3],
    time_data: Vec<Instant>,
    last_calibration: Instant,
}

//impl Debug for Imu {
//    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//        #[derive(Debug)]
//        struct Imu<'a> {
//            mag_coeffs: &'a [f32; 3],
//            north_vector: &'a [f32; 3],
//            gyro_data: &'a Vec<f32>,
//            mag_data: &'a [Vec<f32>; 3],
//            time_data: &'a Vec<Instant>,
//            last_calibration: &'a Instant,
//        }
//
//        let Self {
//            device: _,
//            mag_coeffs,
//            north_vector,
//            gyro_data,
//            mag_data,
//            time_data,
//            last_calibration,
//        } = self;
//
//        fmt::Debug::fmt(
//            &Imu {
//                mag_coeffs,
//                north_vector,
//                gyro_data,
//                mag_data,
//                time_data,
//                last_calibration,
//            },
//            f,
//        )
//    }
//}

impl Imu {
    const COEFFS_FILE: &'static str = "mag_coeffs";

    pub fn new(bus: u8) -> Result<Self, Error> {
        let i2c = rppal::i2c::I2c::with_bus(bus)?;
        let mut delay = rppal::hal::Delay::new();
        let mpu = Mpu9250::marg_default(i2c, &mut delay)?;
        let mut s = Self {
            device: mpu,
            mag_coeffs: [0.0, 0.0, 0.0],
            north_vector: [1.0, 0.0, 0.0],
            gyro_data: vec![],
            mag_data: Default::default(),
            time_data: vec![],
            last_calibration: Instant::now(),
        };
        if s.load_mag_coeffs_from_file(Self::COEFFS_FILE) {
            info!("Magnetometer coefficients loaded from file: {:?}", s.mag_coeffs);
            info!("Magnetometer north vector loaded from file: {:?}", s.north_vector);
        }
        Ok(s)
    }

    fn load_mag_coeffs_from_file(&mut self, file: &str) -> bool {
        if Path::new(file).exists() {
            let mut file = match File::open(Self::COEFFS_FILE) {
                Ok(f) => f,
                Err(_e) => {
                    info!("Magnetometer coefficients file doesn't exists");
                    return false;
                }
            };
            let mut buf = [0u8; 24];
            match file.read_exact(&mut buf) {
                Ok(()) => {}
                Err(_e) => {
                    warn!("Failed to read magnetometer coefficients from file");
                    return false;
                }
            };
            let (coeffs, north): ([f32; 3], [f32; 3]) = match bytemuck::try_cast_slice(&buf) {
                Ok(c) => {
                    if c.len() != 6 {
                        warn!("Wrong data size when reading magnetometer coefficients from file");
                        return false;
                    }
                    let coeffs: [f32; 3] = match c[0..3].try_into() {
                        Ok(cs) => cs,
                        Err(_e) => {
                            warn!(
                                "Wrong data size when reading magnetometer coefficients from file"
                            );
                            return false;
                        }
                    };
                    let north: [f32; 3] = match c[3..6].try_into() {
                        Ok(cs) => cs,
                        Err(_e) => {
                            warn!(
                                "Wrong data size when reading magnetometer coefficients from file"
                            );
                            return false;
                        }
                    };

                    (coeffs, north)
                }
                Err(_e) => {
                    warn!("Failed to convert magnetometer coefficients file data to floats");
                    return false;
                }
            };
            self.mag_coeffs = coeffs;
            self.north_vector = north;
            return true;
        }
        false
    }

    fn detect_rotation(&mut self, threshold: f32, time_limit: Duration, n: usize) -> bool {
        if n != self.mag_data[0].len()
            || n != self.mag_data[1].len()
            || n != self.mag_data[2].len()
            || n != self.time_data.len()
        {
            self.gyro_data = vec![];
            self.mag_data = Default::default();
            self.time_data = vec![];
            return false;
        }
        let mut total_angle = 0f32;
        let start = self.time_data[0];
        for i in 1..n {
            let angle_diff = self.gyro_data[i]
                * (self.time_data[i]
                    .duration_since(self.time_data[i - 1])
                    .as_secs_f32());
            total_angle += angle_diff;
            if self.time_data[i].duration_since(start) >= time_limit {
                return false;
            }
            if total_angle.abs() >= threshold {
                return true;
            }
        }
        false
    }

    fn update_calibartion(&mut self) -> bool {
        //eprintln!("=================================================");
        info!("MAGNETOMETER CALIBRATION START");
        //eprintln!("=================================================");
        // Always called right after detect_rotation() and only if it returns true,
        // so data sizes are confirmed to be correct at this point
        let [x, y, z] = &self.mag_data;
        #[allow(clippy::cast_precision_loss)]
        let x_mean = x.iter().sum::<f32>() / x.len() as f32;
        #[allow(clippy::cast_precision_loss)]
        let y_mean = y.iter().sum::<f32>() / y.len() as f32;
        #[allow(clippy::cast_precision_loss)]
        let z_mean = z.iter().sum::<f32>() / z.len() as f32;
        #[allow(clippy::cast_precision_loss)]
        let x_centered = x.iter().map(|a| a - x_mean).collect::<Vec<f32>>();
        #[allow(clippy::cast_precision_loss)]
        let y_centered = y.iter().map(|a| a - y_mean).collect::<Vec<f32>>();

        #[allow(clippy::cast_precision_loss)]
        let x_centered_mean = x_centered.iter().sum::<f32>() / x_centered.len() as f32;
        #[allow(clippy::cast_precision_loss)]
        let y_centered_mean = y_centered.iter().sum::<f32>() / y_centered.len() as f32;

        let mag_max = (x_centered_mean.powi(2) + y_centered_mean.powi(2)).sqrt();
        if mag_max == 0.0 {
            self.north_vector = [1.0, 0.0, 0.0];
        } else {
            self.north_vector = [x_centered_mean / mag_max, y_centered_mean / mag_max, 0.0];
        };
        self.mag_coeffs = [x_mean, y_mean, z_mean];
        self.last_calibration = Instant::now();
        match File::create(Self::COEFFS_FILE) {
            Ok(mut file) => {
                let bytes: &[u8] = match bytemuck::try_cast_slice(&self.mag_coeffs) {
                    Ok(b) => b,
                    Err(_e) => {
                        warn!("Failed to cast magnetometer coefficients to bytes");
                        return false;
                    }
                };
                match file.write_all(bytes) {
                    Ok(()) => {}
                    Err(_err) => {
                        warn!("Failed to write to magnetometer coefficients file");
                        return false;
                    }
                }
                let bytes: &[u8] = match bytemuck::try_cast_slice(&self.north_vector) {
                    Ok(b) => b,
                    Err(_e) => {
                        warn!("Failed to cast magnetometer north vector to bytes");
                        return false;
                    }
                };
                match file.write_all(bytes) {
                    Ok(()) => {}
                    Err(_err) => {
                        warn!("Failed to write to magnetometer north vector file");
                        return false;
                    }
                }
            }
            Err(_err) => {
                warn!("Failed to open magnetometer coefficients file");
                return false;
            }
        }
        //eprintln!("=================================================");
        info!("MAGNETOMETER CALIBRATION END");
        //eprintln!("=================================================");
        true
    }

    fn calculate_angle_and_magnitude(&self, magn: [f32; 3]) -> (f32, f32) {
        let mag = {
            let mut mag = [0f32; 3];
            for ((a, b), c) in mag.iter_mut().zip(&magn).zip(&self.mag_coeffs) {
                *a = b - c;
            }
            mag
        };
        let mag_magnitude = mag.iter().map(|a| a.powi(2)).sum::<f32>().sqrt();
        let angle = mag[1].atan2(mag[2]) - self.north_vector[1].atan2(self.north_vector[0]);
        //let angle = angle - 2.0 * PI * (angle / (2.0 * PI)).floor();
        let angle = angle.sin().atan2(angle.cos()) + PI;
        //let angle = angle % (2.0 * PI);
        let angle = angle * 180.0 / PI;

        (angle, mag_magnitude)
    }

    pub fn calibrate(&mut self) -> Result<(), Error> {
        let accel_biases: [f32; 3] =
            match self.device.calibrate_at_rest(&mut rppal::hal::Delay::new()) {
                Ok(b) => b,
                Err(e) => return Err(Error::Mpu(e)),
            };

        //eprintln!("{accel_biases:?}");
        self.device.set_accel_bias(false, accel_biases)?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Data {
    accel: [f32; 3],
    gyro: [f32; 3],
    mag: [f32; 3],
    pub angle_rel_to_north: f32,
    mag_magnitute: f32,
    //temp: f32,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("MPU9250 error")]
    Mpu(mpu9250::Error<mpu9250::I2CError<rppal::i2c::Error>>),
    #[error("MPU9250 bus error")]
    Bus(mpu9250::I2CError<rppal::i2c::Error>),
    #[error("I2c error")]
    I2c(#[from] rppal::i2c::Error),
}

impl From<mpu9250::Error<mpu9250::I2CError<rppal::i2c::Error>>> for Error {
    fn from(value: mpu9250::Error<mpu9250::I2CError<rppal::i2c::Error>>) -> Self {
        Self::Mpu(value)
    }
}

impl From<mpu9250::I2CError<rppal::i2c::Error>> for Error {
    fn from(value: mpu9250::I2CError<rppal::i2c::Error>) -> Self {
        Self::Bus(value)
    }
}

impl Device for Imu {
    type Data = Data;
    type Error = Error;

    fn get_data(&mut self) -> Result<Self::Data, Self::Error> {
        match self.device.all::<[f32; 3]>() {
            Ok(data) => {
                let now = Instant::now();
                let magn = data.mag;
                let (angle, mag_magnitute) = self.calculate_angle_and_magnitude(magn);
                self.gyro_data.push(data.gyro[2]);
                self.mag_data[0].push(magn[0]);
                self.mag_data[1].push(magn[1]);
                self.mag_data[2].push(magn[2]);
                self.time_data.push(now);
                let n = self.gyro_data.len();
                if n > 1 {
                    if self.detect_rotation(2.0 * PI, Duration::from_secs(10), n) {
                        self.update_calibartion();
                        self.gyro_data = vec![];
                        self.mag_data = Default::default();
                        self.time_data = vec![];
                    } else if now.duration_since(self.time_data[0]) > Duration::from_secs(10) {
                        self.gyro_data = vec![];
                        self.mag_data = Default::default();
                        self.time_data = vec![];
                    }
                };
                Ok(Self::Data {
                    accel: data.accel,
                    gyro: data.gyro,
                    mag: magn,
                    angle_rel_to_north: angle,
                    mag_magnitute,
                })
            }
            Err(e) => Err(Error::Bus(e)),
        }
    }
}
