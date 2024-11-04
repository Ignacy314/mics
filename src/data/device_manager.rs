use serde::{Deserialize, Serialize};

use super::{aht, bmp, gps};
use crate::data::{Aht, Bmp, Gps};
use std::time::Duration;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Status {
    Ok = 0,
    Disconnected = 1,
    NoData = 2,
    OtherError = 3,
}

impl Default for Status {
    fn default() -> Self {
        Self::Disconnected
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Statuses {
    pub gps: Status,
    pub aht: Status,
    pub wind: Status,
    pub imu: Status,
    pub bmp: Status,
}

#[derive(Default)]
pub struct DeviceManager {
    pub gps: Option<Gps>,
    pub aht: Option<Aht>,
    //pub wind: Option<Wind>,
    //pub imu: Option<Imu>,
    pub bmp: Option<Bmp>,
    pub settings: Settings,
    pub statuses: Statuses,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn try_set_gps(&mut self) -> Result<(), gps::Error> {
        let UartDeviceSettings { port, baud_rate, timeout } = self.settings.gps;
        self.gps = Some(Gps::new(port, baud_rate, timeout)?);
        self.statuses.gps = Status::Ok;
        Ok(())
    }

    pub fn try_set_aht(&mut self) -> Result<(), aht::Error> {
        self.aht = Some(Aht::new(self.settings.aht_bus)?);
        self.statuses.aht = Status::Ok;
        Ok(())
    }

    //pub fn try_set_wind(&mut self) -> Result<(), wind::Error> {
    //    let UartDeviceSettings { port, baud_rate, timeout } = self.settings.wind;
    //    self.wind = Some(Wind::new(port, baud_rate, timeout)?);
    //    self.statuses.wind = Status::Ok;
    //    Ok(())
    //}

    //pub fn try_set_imu(&mut self) -> Result<(), imu::Error> {
    //    let mut imu = Imu::new(self.settings.imu_bus)?;
    //    imu.calibrate()?;
    //    self.imu = Some(imu);
    //    self.statuses.imu = Status::Ok;
    //    Ok(())
    //}

    pub fn try_set_bmp(&mut self) -> Result<(), bmp::Error> {
        self.bmp = Some(Bmp::new()?);
        self.statuses.bmp = Status::Ok;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct UartDeviceSettings {
    pub port: &'static str,
    pub baud_rate: u32,
    pub timeout: Duration,
}

#[derive(Debug)]
pub struct Settings {
    pub gps: UartDeviceSettings,
    pub aht_bus: u8,
    pub wind: UartDeviceSettings,
    pub imu_bus: u8,
}

impl Default for Settings {
    fn default() -> Self {
        let gps = UartDeviceSettings {
            port: "/dev/ttyAMA0",
            baud_rate: 9_600,
            timeout: Duration::from_millis(250),
        };
        let aht_bus = 1u8;
        let wind = UartDeviceSettings {
            port: "/dev/ttyAMA2",
            baud_rate: 9_600,
            timeout: Duration::from_millis(250),
        };
        let imu_bus = 1u8;
        Self { gps, aht_bus, wind, imu_bus }
    }
}
