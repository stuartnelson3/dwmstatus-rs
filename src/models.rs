extern crate network_manager;

use std::fs::File;
use std::io;
use std::string::String;
use std::io::BufReader;
use std::io::BufRead;
use std::io::{Error, ErrorKind};

#[derive(Debug)]
pub enum BatteryStatus {
    Charged,
    Charging,
    Discharging,
    Unknown,
}

pub struct Battery {
    pub power: f32,
    pub energy: f32,
    pub capacity: f32,
    pub status: BatteryStatus,
}

impl Battery {
    pub fn new() -> Battery {
        Battery {
            power: 0.0,
            energy: 0.0,
            capacity: 0.0,
            status: BatteryStatus::Unknown,
        }
    }

    fn percent(&self) -> f32 {
        100.0 * self.energy / self.capacity
    }

    fn remaining(&self) -> f32 {
        // Watts / Watt*hrs
        self.energy / self.power
    }

    pub fn status(&self) -> String {
        match self.status {
            BatteryStatus::Charged => "charged".to_owned(),
            BatteryStatus::Charging => format!("{:.2}% (+)", self.percent()),
            BatteryStatus::Discharging => {
                format!("{:.2}% ({:.2} hrs)", self.percent(), self.remaining())
            }
            BatteryStatus::Unknown => "no battery found".to_owned(),
        }
    }

    pub fn combine(&mut self, other: Battery) {
        self.power += other.power;
        self.capacity += other.capacity;
        self.energy += other.energy;

        match other.status {
            BatteryStatus::Charged => {
                // Implement std::cmp::PartialEq so I can just use an if statement.
                match self.status {
                    BatteryStatus::Charging => self.status = BatteryStatus::Charging,
                    BatteryStatus::Discharging => {}
                    _ => self.status = BatteryStatus::Charged,
                }
            }
            _ => self.status = other.status,
        };
    }
}

#[derive(Debug)]
pub struct NetworkInterface {
    device: network_manager::Device,
    rx: f32,
    tx: f32,
}

impl NetworkInterface {
    pub fn devices() -> Vec<Self> {
        use network_manager::NetworkManager;
        let manager = NetworkManager::new();
        let devices = manager.get_devices().unwrap();

        // Find active wifi device
        // Assuming we are on wifi, and that the first card is the right card.
        devices
            .into_iter()
            .map(|dev| {
                NetworkInterface {
                    device: dev,
                    rx: 0.0,
                    tx: 0.0,
                }
            })
            .collect()
    }

    pub fn wifi() -> Option<Self> {
        NetworkInterface::devices()
            .into_iter()
            .filter(|dev| {
                dev.device.device_type() == &network_manager::DeviceType::WiFi
            })
            .next()
    }

    pub fn ethernet() -> Option<Self> {
        NetworkInterface::devices()
            .into_iter()
            .filter(|dev| {
                dev.device.device_type() == &network_manager::DeviceType::Ethernet
            })
            .next()
    }

    pub fn activated(&self) -> bool {
        match self.device.get_state() {
            Ok(network_manager::DeviceState::Activated) => true,
            _ => false,
        }
    }

    pub fn status(&mut self, manager: &network_manager::NetworkManager) -> String {
        let active_conn = self.find_conn(manager).unwrap();
        match self.get_bytes() {
            Ok((rx, tx)) => {
                let rx = rx / 1024.0;
                let tx = tx / 1024.0;
                let status = format!(
                    "{:?}: {} rx: {:.0} kbps tx: {:.0} kbps",
                    self.device_type(),
                    active_conn.settings().id,
                    rx - self.rx,
                    tx - self.tx,
                );
                self.rx = rx;
                self.tx = tx;
                status
            }
            Err(_e) => format!("data err: {:?}", self.device_type()),
        }
    }

    fn find_conn(
        &self,
        manager: &network_manager::NetworkManager,
    ) -> Option<network_manager::Connection> {
        // Find active connection for active wifi device
        manager
            .get_active_connections()
            .unwrap()
            .into_iter()
            .filter(|conn| {
                conn.get_devices()
                    .unwrap_or(vec![])
                    .iter()
                    .any(|dev| dev.interface() == self.interface())
            })
            .next()
    }

    fn get_bytes(&self) -> io::Result<(f32, f32)> {
        let f = File::open("/proc/net/dev")?;

        let reader = BufReader::new(f);
        let mut lines = reader.lines();
        // drop the header
        let _ = lines.next();
        // ideally, do some fancy index checking to make sure bytes received and transmitted
        // line up.
        let _ = lines.next();
        let section_len = 7;

        for line in lines {
            match line {
                Ok(line) => if line.starts_with(self.interface()) {
                    let mut split = line.split_whitespace();
                    split.next();
                    let rx = split.nth(0);
                    let tx = split.nth(1 * section_len);

                    return Ok((
                        rx.unwrap_or("0").parse::<f32>().unwrap(),
                        tx.unwrap_or("0").parse::<f32>().unwrap(),
                    ));
                },
                Err(_) => continue,
            }
        }

        return Err(Error::new(ErrorKind::Other, "oh no!"));
    }

    fn interface(&self) -> &str {
        self.device.interface()
    }

    fn device_type(&self) -> &network_manager::DeviceType {
        self.device.device_type()
    }
}
