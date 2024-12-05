use core::f32;
//#![allow(unused)]
use std::f32::consts::PI;
use std::fmt::Debug;
use std::time::Instant;

use log::info;
use mpu9250::{Mpu9250, MpuConfig};
use serde::{Deserialize, Serialize};

use super::Device;

trait Buffer {
    type Container;
}

impl<T> Buffer for Vec<T> {
    type Container = Vec<T>;
}

struct CircularBuffer<B: Buffer> {
    size: usize,
    buf: B::Container,
    index: usize,
}

impl<B: Buffer> CircularBuffer<B> {
    fn increment_index(&mut self) {
        self.index += 1;
        self.index %= self.size;
    }
}

impl<T: Clone + Copy> CircularBuffer<Vec<T>> {
    fn new(size: usize, fill: T) -> Self {
        Self {
            size,
            buf: vec![fill; size],
            index: 0,
        }
    }

    fn push(&mut self, value: T) {
        self.buf[self.index] = value;
        self.increment_index();
    }

    fn newest(&self) -> T {
        if self.index == 0 {
            return self.buf[self.size - 1];
        }
        self.buf[self.index - 1]
    }

    fn oldest(&self) -> T {
        self.buf[self.index]
    }

    //fn iter(&self) -> impl Iterator<Item = &T> {
    //    //self.buf.iter().skip(self.index).chain(self.buf.iter().take(self.index))
    //    self.buf.iter().cycle().skip(self.index).take(self.size)
    //}
}

//type Circular2DArray<T> = CircularBuffer<Array2<T>>;
type CircularVector<T> = CircularBuffer<Vec<T>>;

pub struct Imu {
    device: Mpu9250<mpu9250::I2cDevice<rppal::i2c::I2c>, mpu9250::Marg>,
    acc_data: CircularVector<[f32; 3]>,
    gyro_data: CircularVector<[f32; 3]>,
    mag_data: CircularVector<[f32; 3]>,
    time_data: CircularVector<Instant>,
    mag_sens_adj: [f32; 3],
    mag_bias: [f32; 3],
    mag_scale: [f32; 3],
    filtered_mag: [f32; 3],
    filtered_acc: [f32; 3],
    //acc_biases: [f32; 3],
    //b: Array2<f32>,
    //a_1: Array2<f32>,
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
    //const COEFFS_FILE: &'static str = "mag_coeffs";
    //const SAMPLES: usize = 200;
    const ACCEL_SCALE: f32 = 2.0 / 32768.0;
    const DEG_TO_RAD: f32 = PI / 180.0;
    const GYRO_SCALE: f32 = 250.0 / 32768.0;
    //const MAG_SCALE: f32 = 4800.0 / 8192.0;
    const MAG_SCALE: f32 = 0.15;

    pub fn new(bus: u8, samples: usize) -> Result<Self, Error> {
        let i2c = rppal::i2c::I2c::with_bus(bus)?;
        let mut delay = rppal::hal::Delay::new();
        let mut config = MpuConfig::marg();
        config.mag_scale(mpu9250::MagScale::_16BITS);
        let mpu = Mpu9250::marg(i2c, &mut delay, &mut config)?;
        let s = Self {
            device: mpu,
            acc_data: CircularVector::new(samples, [0.0; 3]),
            gyro_data: CircularVector::new(samples, [0.0; 3]),
            mag_data: CircularVector::new(samples, [0.0; 3]),
            time_data: CircularVector::new(samples, Instant::now()),
            mag_sens_adj: [0.0; 3],
            mag_bias: [0.0; 3],
            mag_scale: [1.0; 3],
            filtered_mag: [0.0; 3],
            filtered_acc: [0.0; 3],
        };
        //if s.load_mag_coeffs_from_file(Self::COEFFS_FILE) {
        //    info!("Magnetometer coefficients loaded from file: {:?}", s.mag_coeffs);
        //    info!("Magnetometer north vector loaded from file: {:?}", s.north_vector);
        //}
        Ok(s)
    }

    //fn load_mag_coeffs_from_file(&mut self, file: &str) -> bool {
    //    if Path::new(file).exists() {
    //        let mut file = match File::open(Self::COEFFS_FILE) {
    //            Ok(f) => f,
    //            Err(_e) => {
    //                info!("Magnetometer coefficients file doesn't exists");
    //                return false;
    //            }
    //        };
    //        let mut buf = [0u8; 24];
    //        match file.read_exact(&mut buf) {
    //            Ok(()) => {}
    //            Err(_e) => {
    //                warn!("Failed to read magnetometer coefficients from file");
    //                return false;
    //            }
    //        };
    //        let (coeffs, north): ([f32; 3], [f32; 3]) = match bytemuck::try_cast_slice(&buf) {
    //            Ok(c) => {
    //                if c.len() != 6 {
    //                    warn!("Wrong data size when reading magnetometer coefficients from file");
    //                    return false;
    //                }
    //                let coeffs: [f32; 3] = match c[0..3].try_into() {
    //                    Ok(cs) => cs,
    //                    Err(_e) => {
    //                        warn!(
    //                            "Wrong data size when reading magnetometer coefficients from file"
    //                        );
    //                        return false;
    //                    }
    //                };
    //                let north: [f32; 3] = match c[3..6].try_into() {
    //                    Ok(cs) => cs,
    //                    Err(_e) => {
    //                        warn!(
    //                            "Wrong data size when reading magnetometer coefficients from file"
    //                        );
    //                        return false;
    //                    }
    //                };
    //
    //                (coeffs, north)
    //            }
    //            Err(_e) => {
    //                warn!("Failed to convert magnetometer coefficients file data to floats");
    //                return false;
    //            }
    //        };
    //        self.mag_coeffs = coeffs;
    //        self.north_vector = north;
    //        return true;
    //    }
    //    false
    //}

    //fn detect_rotation(&mut self, threshold: f32, time_limit: Duration, n: usize) -> bool {
    //    //if n != self.mag_data[0].len()
    //    //    || n != self.mag_data[1].len()
    //    //    || n != self.mag_data[2].len()
    //    //    || n != self.time_data.len()
    //    //{
    //    //    self.gyro_data = vec![];
    //    //    //self.mag_data = Default::default();
    //    //    self.time_data = vec![];
    //    //    return false;
    //    //}
    //    let mut total_angle = 0f32;
    //    let start = self.time_data[0];
    //    for i in 1..n {
    //        let angle_diff = self.gyro_data[i]
    //            * (self.time_data[i]
    //                .duration_since(self.time_data[i - 1])
    //                .as_secs_f32());
    //        total_angle += angle_diff;
    //        if self.time_data[i].duration_since(start) >= time_limit {
    //            return false;
    //        }
    //        if total_angle.abs() >= threshold {
    //            return true;
    //        }
    //    }
    //    false
    //}

    fn update_mag_calibartion(&mut self) {
        let [mut max_x, mut max_y, mut max_z] = self.mag_data.buf[0];
        let [mut min_x, mut min_y, mut min_z] = self.mag_data.buf[0];
        for &[x, y, z] in self.mag_data.buf.iter().skip(1) {
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            max_z = max_z.max(z);
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            min_z = min_z.min(z);
        }

        self.mag_bias = [
            (max_x + min_x) / 2.0,
            (max_y + min_y) / 2.0,
            (max_z + min_z) / 2.0,
        ];

        let [avg_delta_x, avg_delta_y, avg_delta_z] = [
            (max_x - min_x) / 2.0,
            (max_y - min_y) / 2.0,
            (max_z - min_z) / 2.0,
        ];

        let avg_delta = (avg_delta_x + avg_delta_y + avg_delta_z) / 3.0;

        self.mag_scale = [
            avg_delta / avg_delta_x,
            avg_delta / avg_delta_y,
            avg_delta / avg_delta_z,
        ];

        info!("MAGNETOMETER CALIBRATION");
    }

    fn dot(v: &[f32], w: &[f32]) -> f32 {
        v.iter().zip(w.iter()).map(|(x, y)| x * y).sum()
    }

    /// Orthogonal projection of v on onto the plane orthogonal to w
    fn oproj(v: &[f32; 3], w: &[f32; 3]) -> [f32; 3] {
        let a = Self::dot(v, w) / Self::dot(w, w);
        [v[0] - a * w[0], v[1] - a * w[1], v[2] - a * w[2]]
    }

    fn calculate_angle(mag: &[f32; 3], acc: &[f32; 3]) -> f32 {
        // Project mag onto a plane perpendicular to Earth's gravity vector
        let vec_north = Self::oproj(mag, acc);

        // Assuming x is forward y is left
        vec_north[0].atan2(vec_north[1]) * 180.0 / PI
    }

    pub fn calibrate(&mut self) -> Result<(), Error> {
        const G: f32 = 9.807;
        let mut accel_biases: [f32; 3] =
            match self.device.calibrate_at_rest(&mut rppal::hal::Delay::new()) {
                Ok(b) => b,
                Err(e) => return Err(Error::Mpu(e)),
            };
        self.mag_sens_adj = self.device.mag_sensitivity_adjustments();

        //eprintln!("{accel_biases:?}");
        if accel_biases[2] > 0.0 {
            accel_biases[2] -= G;
        } else {
            accel_biases[2] += G;
        }
        self.device
            //.set_accel_bias(true, accel_biases.map(|a| a / 9.807))?;
            .set_accel_bias(true, accel_biases)?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, Copy)]
pub struct Data {
    pub acc: [f32; 3],
    gyro: [f32; 3],
    pub mag: [f32; 3],
    pub angle_rel_to_north: f32,
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

fn low_pass_filter(a: &[f32; 3], b: &[f32; 3]) -> [f32; 3] {
    const OLD: f32 = 0.0;
    const NEW: f32 = 1.0 - OLD;
    [
        OLD * a[0] + NEW * b[0],
        OLD * a[1] + NEW * b[1],
        OLD * a[2] + NEW * b[2],
    ]
}

impl Device for Imu {
    type Data = Data;
    type Error = Error;

    fn get_data(&mut self) -> Result<Self::Data, Self::Error> {
        match self.device.unscaled_all::<[i16; 3]>() {
            Ok(data) => {
                let now = Instant::now();
                let mag = [
                    f32::from(data.mag[0]) * Self::MAG_SCALE * self.mag_sens_adj[0],
                    f32::from(data.mag[1]) * Self::MAG_SCALE * self.mag_sens_adj[1],
                    f32::from(data.mag[2]) * Self::MAG_SCALE * self.mag_sens_adj[2],
                ];
                let acc = [
                    f32::from(data.accel[0]) * Self::ACCEL_SCALE,
                    f32::from(data.accel[1]) * Self::ACCEL_SCALE,
                    f32::from(data.accel[2]) * Self::ACCEL_SCALE,
                ];
                let gyro = [
                    f32::from(data.gyro[0]) * Self::GYRO_SCALE * Self::DEG_TO_RAD,
                    f32::from(data.gyro[1]) * Self::GYRO_SCALE * Self::DEG_TO_RAD,
                    f32::from(data.gyro[2]) * Self::GYRO_SCALE * Self::DEG_TO_RAD,
                ];

                self.time_data.push(now);
                self.mag_data.push(mag);
                self.acc_data.push(acc);
                self.gyro_data.push(gyro);

                let mag = [
                    (mag[0] - self.mag_bias[0]) * self.mag_scale[0],
                    (mag[1] - self.mag_bias[1]) * self.mag_scale[1],
                    (mag[2] - self.mag_bias[2]) * self.mag_scale[2],
                ];

                self.filtered_acc = low_pass_filter(&self.filtered_acc, &acc);
                self.filtered_mag = low_pass_filter(&self.filtered_mag, &mag);

                let angle = Self::calculate_angle(&self.filtered_mag, &self.filtered_acc);
                //eprintln!("raw_acc: {:?}", data.accel);
                eprintln!("angle: {angle}  |  acc: {:?}  |  mag: {:?}", self.filtered_acc, self.filtered_mag);

                let n = self.gyro_data.index;
                //eprintln!("{n}");
                if n == 0 {
                    self.update_mag_calibartion();
                };

                Ok(Self::Data {
                    acc,
                    gyro,
                    mag,
                    angle_rel_to_north: angle,
                })
            }
            Err(e) => Err(Error::Bus(e)),
        }
    }
}
