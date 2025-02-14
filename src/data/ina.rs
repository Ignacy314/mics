use std::cmp::Ordering;
//use std::fmt::Display;
use std::thread;

use ina219::address::Address;
use ina219::calibration::UnCalibrated;
use ina219::SyncIna219;
use linreg::linear_regression_of;
use log::info;
use serde::{Deserialize, Serialize};

use super::Device;

pub struct Ina {
    device: SyncIna219<rppal::i2c::I2c, UnCalibrated>,
    //prev_voltage: u16,
    voltage: CircularVoltage,
    prev_charge: Charge,
}

impl Ina {
    pub fn new() -> Result<Self, Error> {
        let i2c = rppal::i2c::I2c::new()?;
        let ina = SyncIna219::new(i2c, Address::from_byte(0x40)?)?;
        Ok(Self {
            device: ina,
            voltage: CircularVoltage::new(),
            prev_charge: Charge::default(),
        })
    }

    //pub fn get_charge(&self) -> Charge {
    //    self.prev_charge
    //}
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum Charge {
    Unknown,
    Charging(u16),
    Discharging(u16),
    CriticalError,
    CriticalDischarge,
}

impl Default for Charge {
    fn default() -> Self {
        Self::Unknown
    }
}
//
//impl Display for Charge {
//    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//        match self {
//            Charge::Unknown => write!(f, "Unknown"),
//            Charge::Charging(p) => write!(f, "Charging: {p}%"),
//            Charge::Discharging(p) => write!(f, "Discharging: {p}%"),
//            Charge::CriticalError => write!(f, "Critical Error"),
//            Charge::CriticalDischarge => write!(f, "Critical Discharge"),
//        }
//    }
//}

#[derive(Debug, Serialize, Deserialize, Default, Clone, Copy)]
pub struct Data {
    pub bus_voltage: u16,
    shunt_voltage: i32,
    current: u16,
    power: f32,
    charge: Charge,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I2C error")]
    I2C(#[from] rppal::i2c::Error),
    #[error("INA address error")]
    InaAddress(#[from] ina219::address::OutOfRange),
    #[error("INA init error")]
    InaInit(#[from] ina219::errors::InitializationError<rppal::i2c::I2c, rppal::i2c::Error>),
    #[error("INA config read error")]
    InaConfigRead(#[from] ina219::errors::ConfigurationReadError<rppal::i2c::Error>),
    #[error("INA measurement error")]
    InaMeasurement(#[from] ina219::errors::MeasurementError<rppal::i2c::Error>),
    #[error("INA bus voltage read error")]
    InaBusRead(#[from] ina219::errors::BusVoltageReadError<rppal::i2c::Error>),
    #[error("INA shunt voltage read error")]
    InaShuntRead(#[from] ina219::errors::ShuntVoltageReadError<rppal::i2c::Error>),
}

impl Device for Ina {
    type Data = Data;
    type Error = Error;

    fn get_data(&mut self) -> Result<Self::Data, Self::Error> {
        if let Some(time) = self.device.configuration()?.conversion_time() {
            thread::sleep(time);
        }

        let bus_voltage = (self.device.bus_voltage()?).voltage_mv();
        let shunt_voltage = (self.device.shunt_voltage()?).shunt_voltage_uv();
        let current = (self.device.current_raw()?).0 * 10;
        let power = shunt_voltage.unsigned_abs() as f32 / 100.0;

        let old = self.voltage.push(u32::from(bus_voltage));
        let new_ord = self.voltage.update_mean();
        let charge = if old == 0 {
            Charge::Unknown
        } else if bus_voltage >= 15000 {
            Charge::CriticalError
        } else if bus_voltage <= 10000 {
            Charge::CriticalDischarge
        } else if new_ord == Ordering::Greater {
            let percentage = (bus_voltage - 10500) / 43;
            Charge::Charging(percentage)
        } else if new_ord == Ordering::Less {
            let percentage = (bus_voltage - 10500) / 24;
            Charge::Discharging(percentage)
        } else {
            self.prev_charge
        };

        self.prev_charge = charge;

        Ok(Self::Data {
            bus_voltage,
            shunt_voltage,
            current,
            power,
            charge,
        })
    }
}

pub struct CircularVoltage {
    voltage: [u32; Self::SIZE],
    index: usize,
    //mean: u32,
}

struct CircularBatStatus {
    status: [u8; 50],
    index: usize,
}

impl CircularVoltage {
    const SIZE: usize = 10 * 100;

    pub fn new() -> Self {
        Self {
            voltage: [0; Self::SIZE],
            index: 0,
            //mean: 0,
        }
    }

    pub fn push(&mut self, v: u32) -> u32 {
        let old = self.voltage[self.index];
        self.voltage[self.index] = v;
        self.index = (self.index + 1) % Self::SIZE;
        old
    }

    //pub fn newest(&self) -> u16 {
    //    let index = if self.index == 0 {
    //        5
    //    } else {
    //        self.index - 1
    //    };
    //
    //    self.voltage[index]
    //}

    //pub fn oldest(&self) -> u16 {
    //    self.voltage[self.index]
    //}

    fn update_mean(&mut self) -> Ordering {
        //let tuples = self
        //    .voltage
        //    .iter()
        //    .cycle()
        //    .skip(self.index)
        //    .take(Self::SIZE)
        //    .enumerate()
        //    .map(|(i, v)| (i as f32, *v as f32))
        //    .collect::<Vec<_>>();
        //
        //let lr = linear_regression_of::<f32, f32, f32>(&tuples);
        //if let Ok((a, _)) = lr {
        //    return a.total_cmp(&0.0);
        //}
        //Ordering::Equal

        //let first_half = self
        //    .voltage
        //    .iter()
        //    .cycle()
        //    .skip(self.index)
        //    .take(Self::SIZE / 2)
        //    .collect::<Vec<_>>();

        let mean_1 = self
            .voltage
            .iter()
            .cycle()
            .skip(self.index)
            .take(Self::SIZE / 5)
            .sum::<u32>() as f32
            / self.voltage.len() as f32;

        let mean_2 = self
            .voltage
            .iter()
            .cycle()
            .skip(self.index + Self::SIZE * 4 / 5)
            .take(Self::SIZE - Self::SIZE * 4 / 5)
            .sum::<u32>() as f32
            / self.voltage.len() as f32;

        mean_2.total_cmp(&mean_1)

        //let new_mean: u32 = self
        //    .voltage
        //    .iter()
        //    .filter(|v| **v as f32 >= mean)
        //    //.enumerate()
        //    //.map(|(i, v)| (i as u32 + 1) * v)
        //    .sum();
        //
        //let c = new_mean.cmp(&self.mean);
        //self.mean = new_mean;
        //c
    }
}
