extern crate alsa;
extern crate chrono;
extern crate libc;
extern crate network_manager;
extern crate xcb;

use chrono::prelude::*;
use xcb::ffi::xproto::xcb_change_property;
use libc::c_void;
use std::fs::File;
use std::io;
use std::io::Read;
use std::string::String;
use std::io::BufReader;
use std::io::BufRead;
use std::io::{Error, ErrorKind};

#[derive(Debug)]
enum BatteryStatus {
    Charged,
    Charging,
    Discharging,
    Unknown,
}

struct Battery {
    power: f32,
    energy: f32,
    capacity: f32,
    status: BatteryStatus,
}

impl Battery {
    fn new() -> Battery {
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

    fn status(&self) -> String {
        match self.status {
            BatteryStatus::Charged => "charged".to_owned(),
            BatteryStatus::Charging => format!("{:.2}% (charging)", self.percent()),
            BatteryStatus::Discharging => {
                format!("{:.2}% ({:.2} hrs)", self.percent(), self.remaining())
            }
            BatteryStatus::Unknown => "no battery found".to_owned(),
        }
    }

    fn combine(&mut self, other: Battery) {
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

fn file_as_number(mut file: File) -> f32 {
    let mut buf = String::new();
    file.read_to_string(&mut buf).is_ok();
    // Remove \n
    let trimmed = buf.trim_right();
    trimmed.parse::<f32>().unwrap()
}

fn get_battery(battery: &&str) -> io::Result<Battery> {
    let mut f = File::open(format!("/sys/class/power_supply/{}/status", battery))?;
    let status = {
        let mut buf = String::new();
        f.read_to_string(&mut buf).is_ok();
        // Remove \n
        buf.trim_right().to_owned()
    };

    let energy_now = file_as_number(File::open(
        format!("/sys/class/power_supply/{}/energy_now", battery),
    )?);

    let energy_full = file_as_number(File::open(
        format!("/sys/class/power_supply/{}/energy_full", battery),
    )?);

    let power = file_as_number(File::open(
        format!("/sys/class/power_supply/{}/power_now", battery),
    )?);

    let status = if status == "Charging" {
        BatteryStatus::Charging
    } else if (status == "Unknown" || status == "Full") && power == 0.0 {
        BatteryStatus::Charged
    } else {
        BatteryStatus::Discharging
    };

    Ok(Battery {
        power: power,
        energy: energy_now,
        capacity: energy_full,
        status: status,
    })
}

#[derive(Debug)]
struct NetworkInterface {
    device: network_manager::Device,
    rx: f32,
    tx: f32,
}

impl NetworkInterface {
    fn devices() -> Vec<Self> {
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

    fn wifi() -> Option<Self> {
        NetworkInterface::devices()
            .into_iter()
            .filter(|dev| {
                dev.device.device_type() == &network_manager::DeviceType::WiFi
            })
            .next()
    }

    fn ethernet() -> Option<Self> {
        NetworkInterface::devices()
            .into_iter()
            .filter(|dev| {
                dev.device.device_type() == &network_manager::DeviceType::Ethernet
            })
            .next()
    }

    fn activated(&self) -> bool {
        match self.device.get_state() {
            Ok(network_manager::DeviceState::Activated) => true,
            _ => false,
        }
    }

    fn status(&mut self, manager: &network_manager::NetworkManager) -> String {
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

fn get_date() -> String {
    let dt: DateTime<Local> = Local::now();
    format!(
        "{}.{:02}.{:02} {:02}:{:02}",
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute()
    )
}

fn get_volume(audio_card_name: &str, selem_id: &alsa::mixer::SelemId) -> String {
    let mixer = alsa::mixer::Mixer::new(audio_card_name, true).unwrap();
    let selem = mixer.find_selem(&selem_id).unwrap();

    let (pmin, pmax) = selem.get_playback_volume_range();
    let pvol = selem
        .get_playback_volume(alsa::mixer::SelemChannelId::FrontLeft)
        .unwrap();
    let volume_percent = 100.0 * pvol as f64 / (pmax - pmin) as f64;
    let psw = selem
        .get_playback_switch(alsa::mixer::SelemChannelId::FrontLeft)
        .unwrap();
    if psw == 1 {
        format!("Vol: {}", volume_percent as i8)
    } else {
        format!("Vol: [off]")
    }
}

fn main() {
    let (conn, screen_num) = xcb::Connection::connect(None).unwrap();
    let setup = conn.get_setup();
    let root = setup.roots().nth(screen_num as usize).unwrap().root();
    let one_sec = std::time::Duration::new(1, 0);
    let batteries = vec!["BAT0", "BAT1", "BAT2"];
    let audio_card_name = "default";
    let selem_id = alsa::mixer::SelemId::new("Master", 0);

    let manager = network_manager::NetworkManager::new();

    let mut wifi = NetworkInterface::wifi().unwrap();
    let mut ethernet = NetworkInterface::ethernet().unwrap();

    loop {
        let network_output = if ethernet.activated() {
            ethernet.status(&manager)
        } else if wifi.activated() {
            wifi.status(&manager)
        } else {
            "no connection found".to_owned()
        };

        let battery: Battery = batteries
            .iter()
            .filter_map(|bat| get_battery(bat).ok())
            .fold(Battery::new(), |mut acc, bat| {
                acc.combine(bat);
                acc
            });

        let message = format!(
            " {} | {} | {} | {} ",
            network_output,
            get_volume(audio_card_name, &selem_id),
            get_date(),
            battery.status(),
        );

        let data = message.as_ptr() as *const c_void;

        unsafe {
            xcb_change_property(
                conn.get_raw_conn(),
                xcb::ffi::xproto::XCB_PROP_MODE_REPLACE as u8,
                root,
                xcb::ffi::xproto::XCB_ATOM_WM_NAME,
                xcb::ffi::xproto::XCB_ATOM_STRING,
                8 as u8,
                message.len() as u32,
                data,
            );
        }
        conn.flush();

        std::thread::sleep(one_sec);
    }
}
