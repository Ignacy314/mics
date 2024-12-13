use std::thread;

use ina219::address::Address;
use ina219::calibration::UnCalibrated;
use ina219::SyncIna219;
use serde::{Deserialize, Serialize};

use super::Device;

pub struct Ina {
    device: SyncIna219<rppal::i2c::I2c, UnCalibrated>,
}

impl Ina {
    pub fn new() -> Result<Self, Error> {
        let i2c = rppal::i2c::I2c::new()?;
        let ina = SyncIna219::new(i2c, Address::from_byte(0x40)?)?;
        Ok(Self { device: ina })
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, Copy)]
pub struct Data {
    bus_voltage: u16,
    shunt_voltage: i32,
    current: u16,
    power: u16,
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

        //let measure: Option<ina219::measurements::Measurements<(), ()>> = self.device.next_measurement()?;
        //if let Some(measure) = self.device.next_measurement()? {
        //    let power = measure.power;
        //}
        let bus_voltage = (self.device.bus_voltage()?).voltage_mv();
        let shunt_voltage = (self.device.shunt_voltage()?).shunt_voltage_uv();
        let current = (self.device.current_raw()?).0 * 10;
        let power = (self.device.power_raw()?).0 * 2;

        let d = Self::Data {
            bus_voltage,
            shunt_voltage,
            current,
            power,
        };

        eprintln!("{d:?}");

        Ok(Self::Data {
            bus_voltage,
            shunt_voltage,
            current,
            power,
        })
    }
}
