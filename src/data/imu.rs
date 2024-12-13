//#![allow(unused)]
use std::f32::consts::PI;
use std::fmt::Debug;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

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

    fn reset(&mut self, fill: T) {
        self.index = 0;
        self.buf = vec![fill; self.size];
    }

    //fn iter(&self) -> impl Iterator<Item = &T> {
    //    //self.buf.iter().skip(self.index).chain(self.buf.iter().take(self.index))
    //    self.buf.iter().cycle().skip(self.index).take(self.size)
    //}
}

//type Circular2DArray<T> = CircularBuffer<Array2<T>>;
type CircularVector<T> = CircularBuffer<Vec<T>>;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
struct MagCalib {
    bias: [f32; 3],
    scale: [f32; 3],
}

pub struct Imu {
    device: Mpu9250<mpu9250::I2cDevice<rppal::i2c::I2c>, mpu9250::Marg>,
    //acc_data: CircularVector<[f32; 3]>,
    gyro_data: CircularVector<[f32; 3]>,
    mag_data: CircularVector<[f32; 3]>,
    //time_data: CircularVector<Instant>,
    mag_sens_adj: [f32; 3],
    mag_bias: [f32; 3],
    mag_scale: [f32; 3],
    filtered_mag: [f32; 3],
    filtered_acc: [f32; 3],
    filtered_gyro: [f32; 3],
    rotation: [f32; 3],
    calib_path: PathBuf,
    mag_calib_path: PathBuf,
    //calibrated: bool,
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
    //const SAMPLES: usize = 200;
    const ACCEL_SCALE: f32 = 2.0 / 32768.0;
    const DEG_TO_RAD: f32 = PI / 180.0;
    const GYRO_SCALE: f32 = 250.0 / 32768.0;
    const MAG_SCALE: f32 = 0.15;
    const DEV_CALIB_FILE: &'static str = "calibration";
    const MAG_CALIB_FILE: &'static str = "mag_calibration";

    pub fn new(bus: u8, samples: usize, path: &Path) -> Result<Self, Error> {
        let i2c = rppal::i2c::I2c::with_bus(bus)?;
        let mut delay = rppal::hal::Delay::new();
        let mut config = MpuConfig::marg();
        config.mag_scale(mpu9250::MagScale::_16BITS);
        config.gyro_scale(mpu9250::GyroScale::_250DPS);
        let mpu = Mpu9250::marg(i2c, &mut delay, &mut config)?;
        let calib_path = path.join(Self::DEV_CALIB_FILE);
        let mag_calib_path = path.join(Self::MAG_CALIB_FILE);
        let mut s = Self {
            device: mpu,
            //acc_data: CircularVector::new(samples, [0.0; 3]),
            gyro_data: CircularVector::new(samples, [0.0; 3]),
            mag_data: CircularVector::new(samples, [0.0; 3]),
            //time_data: CircularVector::new(samples, Instant::now()),
            mag_sens_adj: [0.0; 3],
            mag_bias: [0.0; 3],
            mag_scale: [1.0; 3],
            filtered_mag: [0.0; 3],
            filtered_acc: [0.0; 3],
            filtered_gyro: [0.0; 3],
            rotation: [0.0; 3],
            calib_path,
            mag_calib_path,
            //calibrated: false,
        };

        let calib_file_path = Path::new(Self::MAG_CALIB_FILE);

        if calib_file_path.exists() {
            info!("MAGNETOMETER CALIBRATION FILE FOUND");
            let file = File::open(calib_file_path)?;
            let reader = BufReader::new(file);
            let calib: MagCalib = serde_json::from_reader(reader)?;
            info!("MAGNETOMETER CALIBRATION READ FROM FILE");
            s.mag_bias = calib.bias;
            s.mag_scale = calib.scale;
            info!("MAGNETOMETER CALIBRATION COMPLETED");
        } else {
            info!("MAGNETOMETER CALIBRATION FILE NOT FOUND");
        }

        Ok(s)
    }

    fn update_mag_calibartion(&mut self) -> Result<(), Error> {
        info!("MAGNETOMETER CALIBRATION START");

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

        info!("WRITING TO MAGNETOMETER CALIBRATION FILE");

        let file = File::create(self.mag_calib_path.clone())?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(
            &mut writer,
            &MagCalib {
                bias: self.mag_bias,
                scale: self.mag_scale,
            },
        )?;

        info!("MAGNETOMETER CALIBRATION SAVED TO FILE");
        info!("MAGNETOMETER CALIBRATION COMPLETED");

        Ok(())
    }

    fn dot(v: &[f32; 3], w: &[f32; 3]) -> f32 {
        v[0] * w[0] + v[1] * w[1] + v[2] * w[2]
        //v.iter().zip(w.iter()).map(|(x, y)| x * y).sum()
    }

    /// Orthogonal projection of v on onto the plane orthogonal to w
    fn oproj(v: &[f32; 3], w: &[f32; 3]) -> [f32; 3] {
        let a = Self::dot(v, w) / Self::dot(w, w);
        [v[0] - a * w[0], v[1] - a * w[1], v[2] - a * w[2]]
    }

    fn calculate_angle(mag: &[f32; 3], acc: &[f32; 3]) -> f32 {
        // Project mag onto a plane perpendicular to Earth's gravity vector
        let vec_north = Self::oproj(mag, acc);
        //let vec_north = mag;

        // Assuming x is forward y is left (or the other way around, directions are hard)
        vec_north[1].atan2(vec_north[0]) * 180.0 / PI
    }

    pub fn calibrate(&mut self, try_from_file: bool) -> Result<(), Error> {
        const G: f32 = 9.807;

        #[derive(Serialize, Deserialize, Default, Clone, Copy, Debug)]
        struct Calib {
            acc_bias: [f32; 3],
            gyro_bias: [f32; 3],
            mag_sens_adj: [f32; 3],
        }

        info!("DEVICE CALIBRATION START");

        let calib_file_path = self.calib_path.clone();

        if try_from_file {
            if calib_file_path.exists() {
                info!("DEVICE CALIBRATION FILE FOUND");
                let file = File::open(calib_file_path)?;
                let reader = BufReader::new(file);
                let calib: Calib = serde_json::from_reader(reader)?;
                info!("DEVICE CALIBRATION READ FROM FILE");
                self.mag_sens_adj = calib.mag_sens_adj;
                self.device.set_gyro_bias(false, calib.gyro_bias)?;
                self.device.set_accel_bias(true, calib.acc_bias)?;
                info!("DEVICE CALIBRATION COMPLETED");
                return Ok(());
            }
            info!("DEVICE CALIBRATION FILE NOT FOUND");
        }

        let mut acc_bias: [f32; 3] =
            match self.device.calibrate_at_rest(&mut rppal::hal::Delay::new()) {
                Ok(b) => b,
                Err(e) => return Err(Error::Mpu(e)),
            };
        let gyro_bias: [f32; 3] = self.device.get_gyro_bias()?;
        info!("gyro_bias: {gyro_bias:?}");
        self.mag_sens_adj = self.device.mag_sensitivity_adjustments();

        if acc_bias[2] > 0.0 {
            acc_bias[2] -= G;
        } else {
            acc_bias[2] += G;
        }
        let acc_bias = [-acc_bias[0], -acc_bias[1], -acc_bias[2]];

        info!("WRITING TO DEVICE CALIBRATION FILE");

        let file = File::create(calib_file_path)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(
            &mut writer,
            &Calib {
                acc_bias,
                gyro_bias,
                mag_sens_adj: self.mag_sens_adj,
            },
        )?;

        info!("DEVICE CALIBRATION SAVED TO FILE");

        self.device.set_accel_bias(true, acc_bias)?;
        info!("DEVICE CALIBRATION COMPLETED");
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, Copy)]
pub struct Data {
    pub acc: [f32; 3],
    gyro: [f32; 3],
    pub mag: [f32; 3],
    pub angle: f32,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("MPU9250 error: {0:?}")]
    Mpu(mpu9250::Error<mpu9250::I2CError<rppal::i2c::Error>>),
    #[error("MPU9250 bus error")]
    Bus(mpu9250::I2CError<rppal::i2c::Error>),
    #[error("I2c error")]
    I2c(#[from] rppal::i2c::Error),
    #[error("IO error")]
    Io(#[from] std::io::Error),
    #[error("Serde JSON error")]
    Serde(#[from] serde_json::Error),
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
                //let now = Instant::now();
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

                //self.time_data.push(now);
                self.mag_data.push(mag);
                //self.acc_data.push(acc);

                eprintln!("gyro: {gyro:?}");
                self.filtered_gyro = low_pass_filter(&self.filtered_gyro, &gyro);
                self.gyro_data.push(self.filtered_gyro);

                let mag = [
                    (mag[0] - self.mag_bias[0]) * self.mag_scale[0],
                    (mag[1] - self.mag_bias[1]) * self.mag_scale[1],
                    (mag[2] - self.mag_bias[2]) * self.mag_scale[2],
                ];

                self.filtered_acc = low_pass_filter(&self.filtered_acc, &acc);
                self.filtered_mag = low_pass_filter(&self.filtered_mag, &mag);

                let mut angle = Self::calculate_angle(&self.filtered_mag, &self.filtered_acc);
                if angle < 0.0 {
                    angle += 360.0;
                }

                //eprintln!(
                //    "angle: {angle}  |  acc: {:?}  |  mag: {:?}",
                //    self.filtered_acc, self.filtered_mag
                //);

                //let n = self.gyro_data.index;
                //if !self.calibrated && n == 0 {
                //    self.update_mag_calibartion()?;
                //    self.calibrated = true;
                //};

                let newest = self.gyro_data.newest();
                let oldest = self.gyro_data.oldest();

                self.rotation[0] += newest[0] - oldest[0];
                self.rotation[1] += newest[1] - oldest[1];
                self.rotation[2] += newest[2] - oldest[2];

                eprintln!("rotation: {:?}", self.rotation);

                if self.rotation.iter().any(|r| r.abs() >= 2.0 * PI) {
                    self.update_mag_calibartion()?;
                    self.rotation = [0.0; 3];
                    self.gyro_data.reset([0.0; 3]);
                }

                Ok(Self::Data { acc, gyro, mag, angle })
            }
            Err(e) => Err(Error::Bus(e)),
        }
    }
}
