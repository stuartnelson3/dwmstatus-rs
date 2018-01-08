#![feature(libc)]
extern crate libc;
extern crate xcb;
extern crate chrono;

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

enum BatteryStatus {
    Charged,
    // percentage charged (0-100)
    Charging(f32),
    // percentage charged (0-100), time remaining (hours)
    Discharging(f32, f32),
    Unknown,
}

fn file_as_number(mut file: File) -> f32 {
    let mut buf = String::new();
    file.read_to_string(&mut buf).is_ok();
    // Remove \n
    let trimmed = buf.trim_right();
    trimmed.parse::<f32>().unwrap()
}

fn get_battery(batteries: &[&str]) -> io::Result<BatteryStatus> {
    for bat in batteries.iter() {
        let status = match File::open(format!("/sys/class/power_supply/{}/status", bat)) {
            Ok(mut f) => {
                let mut buf = String::new();
                f.read_to_string(&mut buf).is_ok();
                // Remove \n
                buf.trim_right().to_owned()
            }
            Err(_) => continue,
        };

        let energy_now = match File::open(format!("/sys/class/power_supply/{}/energy_now", bat)) {
            Ok(mut f) => file_as_number(f),
            Err(_) => continue,
        };

        let energy_full =
            match File::open(format!("/sys/class/power_supply/{}/energy_full", bat)) {
                Ok(mut f) => file_as_number(f),
                Err(_) => continue,
            };

        let percent = 100.0 * energy_now / energy_full;

        let power = match File::open(format!("/sys/class/power_supply/{}/power_now", bat)) {
            Ok(mut f) => file_as_number(f),
            Err(_) => continue,
        };

        if status == "Charging" {
            return Ok(BatteryStatus::Charging(percent));
        } else if status == "Unknown" && power == 0.0 {
            return Ok(BatteryStatus::Charged);
        } else {
            return Ok(BatteryStatus::Discharging(percent, energy_now / power));
        }

    }
    Ok(BatteryStatus::Unknown)
}

fn get_interface_bytes(interface: &str) -> io::Result<(f32, f32)> {
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
            Ok(line) => {
                if line.starts_with(interface) {
                    let mut split = line.split_whitespace();
                    split.next();
                    let rx = split.nth(0);
                    let tx = split.nth(1 * section_len);

                    return Ok((
                        rx.unwrap_or("0").parse::<f32>().unwrap(),
                        tx.unwrap_or("0").parse::<f32>().unwrap(),
                    ));
                }
            }
            Err(_) => continue,
        }
    }

    return Err(Error::new(ErrorKind::Other, "oh no!"));
}

struct NetworkInterface<'a> {
    name: &'a str,
    rx: f32,
    tx: f32,
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

        let interface_kilobytes = match get_interface_bytes(interface.name) {
            Ok((rx, tx)) => {
                let rx = rx / 1024.0;
                let tx = tx / 1024.0;
                let bytes =
                    format!(
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

        let battery_status = match get_battery(&batteries) {
            // For some weird reason, my charged battery says it has more energy in it that its
            // capacity.
            Ok(BatteryStatus::Charged) => "charged".to_owned(),
            Ok(BatteryStatus::Charging(percent)) => format!("{:.2}% (charging)", percent),
            Ok(BatteryStatus::Discharging(percent, seconds)) => {
                format!("{:.2}% ({:.2} hrs)", percent, seconds)
            }
            Ok(BatteryStatus::Unknown) => "no battery found".to_owned(),
            Err(_) => "error".to_owned(),
        };

        let message =
            format!(
            " tc-73db9 | {} | {} | {} ",
            interface_kilobytes,
            get_date(),
            battery_status,
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
