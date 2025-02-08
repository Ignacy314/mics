use std::fmt::Display;
use std::thread;

use ina219::address::Address;
use ina219::calibration::UnCalibrated;
use ina219::SyncIna219;
use serde::{Deserialize, Serialize};

use super::Device;

pub struct Ina {
    device: SyncIna219<rppal::i2c::I2c, UnCalibrated>,
    prev_voltage: u16,
    prev_charge: Charge,
}

impl Ina {
    pub fn new() -> Result<Self, Error> {
        let i2c = rppal::i2c::I2c::new()?;
        let ina = SyncIna219::new(i2c, Address::from_byte(0x40)?)?;
        Ok(Self {
            device: ina,
            prev_voltage: 0,
            prev_charge: Charge::default(),
        })
    }

    pub fn get_charge(&self) -> Charge {
        self.prev_charge
    }
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
        #[allow(clippy::cast_precision_loss)]
        let power = shunt_voltage.unsigned_abs() as f32 / 100.0;

        let charge = if self.prev_voltage == 0 {
            Charge::Unknown
        } else if bus_voltage >= 15000 {
            Charge::CriticalError
        } else if bus_voltage <= 10000 {
            Charge::CriticalDischarge
        } else if self.prev_voltage < bus_voltage {
            let percentage = (bus_voltage - 10500) / 43;
            Charge::Charging(percentage)
        } else if self.prev_voltage > bus_voltage {
            let percentage = (bus_voltage - 10500) / 24;
            Charge::Discharging(percentage)
        } else {
            self.prev_charge
        };

        self.prev_voltage = bus_voltage;
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

//pub struct CircularVoltage {
//    voltage: [u16; 5],
//    index: usize,
//}
//
//impl CircularVoltage {
//    pub fn new() -> Self {
//        Self { voltage: [0; 5], index: 0 }
//    }
//
//    pub fn push(&mut self, v: u16) {
//        self.voltage[self.index] = v;
//        self.index = (self.index + 1) % 5;
//    }
//
//    pub fn newest()
//}
