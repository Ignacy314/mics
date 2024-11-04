//#![allow(unused)]
use ndarray::{array, s, stack, Array, ArrayBase, Axis, Dim, OwnedRepr, ViewRepr};
use ndarray_linalg::solve::Inverse;
use ndarray_linalg::{Eig, Norm};
use num_traits::identities::Zero;
use std::f32::consts::PI;
use std::fmt::Debug;
use std::time::Instant;

use log::{info, warn};
use mpu9250::Mpu9250;
use ndarray::{Array1, Array2};
use serde::{Deserialize, Serialize};

use super::Device;

trait Buffer {
    type Container;
}

impl<T> Buffer for Array2<T> {
    type Container = Array2<T>;
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

impl<T: Clone + Zero> CircularBuffer<Array2<T>> {
    fn new(size: usize, elems: usize) -> Self {
        Self {
            size,
            buf: Array2::<T>::zeros((size, elems)),
            index: 0,
        }
    }

    fn push(&mut self, value: &Array1<T>) {
        self.buf.row_mut(self.index).assign(value);
        self.increment_index();
    }

    fn iter(&self) -> impl Iterator<Item = ArrayBase<ViewRepr<&T>, Dim<[usize; 1]>>> {
        //self.buf.outer_iter().skip(self.index).chain(self.buf.outer_iter().take(self.index))
        self.buf
            .outer_iter()
            .cycle()
            .skip(self.index)
            .take(self.size)
    }
}

impl<T: Clone> CircularBuffer<Vec<T>> {
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

    fn iter(&self) -> impl Iterator<Item = &T> {
        //self.buf.iter().skip(self.index).chain(self.buf.iter().take(self.index))
        self.buf.iter().cycle().skip(self.index).take(self.size)
    }
}

type Circular2DArray<T> = CircularBuffer<Array2<T>>;
type CircularVector<T> = CircularBuffer<Vec<T>>;

pub struct Imu {
    device: Mpu9250<mpu9250::I2cDevice<rppal::i2c::I2c>, mpu9250::Marg>,
    acc_data: Circular2DArray<f32>,
    gyro_data: Circular2DArray<f32>,
    mag_data: Circular2DArray<f32>,
    time_data: CircularVector<Instant>,
    acc_biases: [f32; 3],
    b: Array2<f32>,
    a_1: Array2<f32>,
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
    const SAMPLES: usize = 50;
    const ACCEL_SCALE: f32 = 2.0 / 32768.0;
    const DEG_TO_RAD: f32 = PI / 180.0;
    const GYRO_SCALE: f32 = 250.0 / 32768.0;
    const MAG_SCALE: f32 = 4800.0 / 8192.0;

    pub fn new(bus: u8) -> Result<Self, Error> {
        let i2c = rppal::i2c::I2c::with_bus(bus)?;
        let mut delay = rppal::hal::Delay::new();
        let mpu = Mpu9250::marg_default(i2c, &mut delay)?;
        let s = Self {
            device: mpu,
            acc_data: Circular2DArray::new(Self::SAMPLES, 3),
            gyro_data: Circular2DArray::new(Self::SAMPLES, 3),
            mag_data: Circular2DArray::new(Self::SAMPLES, 3),
            time_data: CircularVector::new(Self::SAMPLES, Instant::now()),
            acc_biases: [0.0; 3],
            b: Array2::zeros((3, 1)),
            a_1: Array2::eye(3),
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

    fn update_acc_calibration(&mut self) {
        let acc_biases = self.acc_data.buf.mean_axis(Axis(0)).unwrap();
        self.acc_biases = [acc_biases[0], acc_biases[1], acc_biases[2]];
        eprintln!("acc biases: {:?}", self.acc_biases);
    }

    fn update_mag_calibartion(&mut self) -> bool {
        info!("MAGNETOMETER CALIBRATION START");

        let s: &ArrayBase<OwnedRepr<f32>, Dim<[usize; 2]>> = &self.mag_data.buf;
        //eprintln!("{s}");
        let xs: ArrayBase<ViewRepr<&f32>, Dim<[usize; 1]>> = s.slice(s![.., 0]);
        let ys: ArrayBase<ViewRepr<&f32>, Dim<[usize; 1]>> = s.slice(s![.., 1]);
        let zs: ArrayBase<ViewRepr<&f32>, Dim<[usize; 1]>> = s.slice(s![.., 2]);

        //eprintln!("{xs}\n{ys}]\n{zs}");

        let d: ArrayBase<OwnedRepr<f32>, Dim<[usize; 2]>> = stack![
            Axis(0),
            xs.mapv(|a| a.powi(2)),
            ys.mapv(|a| a.powi(2)),
            zs.mapv(|a| a.powi(2)),
            2f32 * &ys * zs,
            2f32 * &xs * zs,
            2f32 * &xs * ys,
            2f32 * &xs,
            2f32 * &ys,
            2f32 * &zs,
            Array::ones(xs.raw_dim())
        ];
        //eprintln!("{d}");
        //eprintln!("{:?}", d.shape());

        let ss: ArrayBase<OwnedRepr<f32>, Dim<[usize; 2]>> = d.dot(&d.t());
        //eprintln!("{:?}", ss.shape());
        let ss_11 = ss.slice(s![..6, ..6]);
        //eprintln!("{:?}", ss_11.shape());
        let ss_12 = ss.slice(s![..6, 6..]);
        //eprintln!("{:?}", ss_12.shape());
        let ss_21 = ss.slice(s![6.., ..6]);
        //eprintln!("{:?}", ss_21.shape());
        let ss_22 = ss.slice(s![6.., 6..]);
        //eprintln!("{:?}", ss_22.shape());

        let cc = array![
            [-1f32, 1.0, 1.0, 0.0, 0.0, 0.0],
            [1.0, -1.0, 1.0, 0.0, 0.0, 0.0],
            [1.0, 1.0, -1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, -4.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0, -4.0, 0.0],
            [0.0, 0.0, 0.0, 0.0, 0.0, -4.0]
        ];

        //eprintln!("{ss_22}");
        let ss_22_1 = ss_22.inv().unwrap();

        let ee = cc
            .inv()
            .unwrap()
            .dot(&(&ss_11 - &ss_12.dot(&ss_22_1.dot(&ss_21))));

        let (ee_w, ee_v) = ee.eig().unwrap();
        let max_index = ee_w
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.re.total_cmp(&b.re))
            .map(|(index, _)| index)
            .unwrap();
        let ee_v = ee_v.map(|a| a.re);
        let v_1: ArrayBase<OwnedRepr<f32>, Dim<[usize; 1]>> = ee_v.slice_move(s![.., max_index]);
        let v_1 = if v_1[0] < 0.0 { -v_1 } else { v_1 };

        let v_2 = (-(ss_22.inv().unwrap())).dot(&ss_21).dot(&v_1);

        let mm = array![
            [v_1[0], v_1[3], v_1[4]],
            [v_1[3], v_1[1], v_1[5]],
            [v_1[4], v_1[5], v_1[2]]
        ];

        let n = array![[v_2[0]], [v_2[1]], [v_2[2]]];

        let d = v_2[3];

        let mm_1 = mm.inv().unwrap();

        let b = -(mm_1.dot(&n));

        let mm_sqrt: ArrayBase<OwnedRepr<f32>, Dim<[usize; 2]>> = {
            let (ew, ev) = mm.eig().unwrap();
            let ew = ew.map(|a| a.re);
            if ew.iter().any(|a| a <= &0.0) {
                warn!("MAGNETOMETER CALIBRATION FAILED");
                return false;
            }
            let ev = ev.map(|a| a.re);
            let ew_sqrt = Array2::from_diag(&ew.mapv(f32::sqrt));
            ev.dot(&ew_sqrt.dot(&ev.inv().unwrap()))
        };

        let den = &n;
        eprintln!("n:\n{den}");
        let den = &mm_1.dot(den);
        eprintln!("M_1.dot(n):\n{den}");
        let den = n.t().dot(den);
        eprintln!("n_T.dot(M_1.dot(n)):\n{den}");
        let den = den[[0, 0]] - d;
        eprintln!("n_T.dot(M_1.dot(n)) - d:\n{den}");
        eprintln!("mm_sqrt:\n{mm_sqrt}");

        if den > 0.0 {
            self.a_1 = (1.0 / den.sqrt()) * mm_sqrt;
            self.b = b;
        } else {
            warn!("MAGNETOMETER CALIBRATION FAILED");
            return false;
        }

        //self.a_1 = (1.0 / (n.t().dot(&mm_1.dot(&n)) - d).mapv(f32::sqrt)) * mm_sqrt;

        info!("MAGNETOMETER CALIBRATION END");
        true
    }

    fn calculate_angle_and_magnitude(mag: &Array1<f32>, acc: &Array1<f32>) -> (f32, f32) {
        let mag_magnitude = mag.norm();
        //let mag_magnitude = mag.iter().map(|a| a.powi(2)).sum::<f32>().sqrt();

        // Project mag onto a plane perpendicular to Earth's gravity vector
        let vec_north = mag - ((mag.dot(acc) / acc.dot(acc)) * acc);

        // Assuming x is forward y is left
        let angle = vec_north[0].atan2(vec_north[1]);
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
        self.device
            .set_accel_bias(true, accel_biases.map(|a| a / 9.806))?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, Copy)]
pub struct Data {
    pub acc: [f32; 3],
    gyro: [f32; 3],
    pub mag: [f32; 3],
    pub angle_rel_to_north: f32,
    mag_magnitute: f32,
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
        match self.device.unscaled_all::<[i16; 3]>() {
            Ok(data) => {
                //eprintln!("{:?}", data.accel);
                let now = Instant::now();
                let mag = [
                    f32::from(data.mag[0]) * Self::MAG_SCALE,
                    f32::from(data.mag[1]) * Self::MAG_SCALE,
                    f32::from(data.mag[2]) * Self::MAG_SCALE,
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

                let mag_arr = array![mag[0], mag[1], mag[2]];
                let acc_arr = array![acc[0], acc[1], acc[2]];
                let gyro_arr = array![gyro[0], gyro[1], gyro[2]];

                self.time_data.push(now);
                self.mag_data.push(&mag_arr);
                self.acc_data.push(&acc_arr);
                self.gyro_data.push(&gyro_arr);

                //eprintln!("mag_arr: {mag_arr}");
                //eprintln!("a_1:\n{}", self.a_1);
                //eprintln!("b:\n{}", self.b);

                let mag_arr = array![[mag[0]], [mag[1]], [mag[2]]];
                let mag_arr = self.a_1.dot(&(mag_arr - &self.b));
                //eprintln!("a_1.dot(mag_arr - b):\n{mag_arr}");
                let mag_arr = array![mag_arr[[0, 0]], mag_arr[[1, 0]], mag_arr[[2, 0]]];

                //let acc_arr = array![
                //    acc[0] - self.acc_biases[0],
                //    acc[1] - self.acc_biases[1],
                //    acc[2] - self.acc_biases[2],
                //];

                let (angle, mag_magnitute) =
                    Self::calculate_angle_and_magnitude(&mag_arr, &acc_arr);

                let n = self.gyro_data.index;
                eprintln!("{n}");
                if n == 0 {
                    //if self.detect_rotation(2.0 * PI, Duration::from_secs(10), n) {
                    //self.update_acc_calibration();
                    self.update_mag_calibartion();
                    //self.gyro_data = vec![];
                    //self.mag_data = Default::default();
                    //self.time_data = vec![];
                    //} else if now.duration_since(self.time_data[0]) > Duration::from_secs(10) {
                    //self.gyro_data = vec![];
                    //self.mag_data = Default::default();
                    //self.time_data = vec![];
                    //}
                };
                Ok(Self::Data {
                    acc,
                    gyro,
                    mag,
                    angle_rel_to_north: angle,
                    mag_magnitute,
                })
            }
            Err(e) => Err(Error::Bus(e)),
        }
    }
}
