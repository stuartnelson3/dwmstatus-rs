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

enum BatteryStatus {
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
        let power = match File::open(format!("/sys/class/power_supply/{}/power_now", bat)) {
            Ok(mut f) => file_as_number(f),
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

        if power == 0.0 {
            return Ok(BatteryStatus::Charging(percent));
        } else {
            return Ok(BatteryStatus::Discharging(percent, energy_now / power));
        }

    }
    Ok(BatteryStatus::Unknown)
}

fn main() {
    let (conn, screen_num) = xcb::Connection::connect(None).unwrap();
    let setup = conn.get_setup();
    let root = setup.roots().nth(screen_num as usize).unwrap().root();
    let one_sec = std::time::Duration::new(1, 0);
    let batteries = vec!["BAT0", "BAT1", "BAT2"];

    loop {

        let battery_status = match get_battery(&batteries) {
            Ok(BatteryStatus::Charging(percent)) => format!("{:.2}%", percent),
            Ok(BatteryStatus::Discharging(percent, seconds)) => {
                format!("{:.2}% ({:.2} hrs)", percent, seconds)
            }
            Ok(BatteryStatus::Unknown) => "no battery found".to_owned(),
            Err(_) => "error".to_owned(),
        };

        let dt: DateTime<Local> = Local::now();
        let message = format!(
            " tc-73db9 | {} | {}.{}.{} {}:{}",
            battery_status,
            dt.year(),
            dt.month(),
            dt.day(),
            dt.hour(),
            dt.minute()
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
