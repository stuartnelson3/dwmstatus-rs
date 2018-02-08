extern crate alsa;
extern crate chrono;
extern crate libc;
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

struct NetworkInterface<'a> {
    name: &'a str,
    rx: f32,
    tx: f32,
}

impl<'a> NetworkInterface<'a> {
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
                Ok(line) => if line.starts_with(self.name) {
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

fn main() {
    let (conn, screen_num) = xcb::Connection::connect(None).unwrap();
    let setup = conn.get_setup();
    let root = setup.roots().nth(screen_num as usize).unwrap().root();
    let one_sec = std::time::Duration::new(1, 0);
    let batteries = vec!["BAT0", "BAT1", "BAT2"];
    let mut interface = NetworkInterface {
        name: "wlp4s0",
        rx: 0.0,
        tx: 0.0,
    };

    loop {
        let interface_kilobytes = match interface.get_bytes() {
            Ok((rx, tx)) => {
                let rx = rx / 1024.0;
                let tx = tx / 1024.0;
                let bytes = format!(
                    "rx: {:.0} kbps tx: {:.0} kbps",
                    rx - interface.rx,
                    tx - interface.tx,
                );
                interface.rx = rx;
                interface.tx = tx;
                bytes
            }
            Err(_e) => "interface not found".to_owned(),
        };

        let battery: Battery = batteries
            .iter()
            .filter_map(|bat| get_battery(bat).ok())
            .fold(Battery::new(), |mut acc, bat| {
                acc.combine(bat);
                acc
            });

        // TODO:
        // - Move into method
        // - Try to not create a new mixer each time
        // - Better way than hardcoding to get mixer/selem_id? (was going through elem iterator and
        // wrapping with selem...)
        let mixer = alsa::mixer::Mixer::new("hw:0", true).unwrap();
        let selem_id = alsa::mixer::SelemId::new("Master", 0);
        let selem = mixer.find_selem(&selem_id).unwrap();
        let volume = format!(
            "Vol: {}",
            selem
                .get_playback_volume(alsa::mixer::SelemChannelId::FrontLeft)
                .unwrap()
        );

        let message = format!(
            " {} | {} | {} | {} ",
            interface_kilobytes,
            volume,
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
