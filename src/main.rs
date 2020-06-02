extern crate alsa;
extern crate chrono;
extern crate libc;
extern crate network_manager;
extern crate xcb;
#[macro_use]
extern crate crossbeam_channel;

use chrono::prelude::*;
use crossbeam_channel as channel;
use libc::c_void;
use std::fs::File;
use std::io;
use std::io::{BufReader, Error, ErrorKind, Read};
use std::string::String;
use std::time::Duration;
use xcb::ffi::xproto::xcb_change_property;

mod models;

fn file_as_number(path: String) -> io::Result<f32> {
    let contents = read_file(path)?;
    contents
        .parse::<f32>()
        .map_err(|err| Error::new(ErrorKind::Other, err))
}

fn read_file(path: String) -> io::Result<String> {
    let f = File::open(path)?;
    let mut buf_reader = BufReader::new(f);
    let mut buf = String::new();
    buf_reader.read_to_string(&mut buf)?;
    // Remove \n
    Ok(buf.trim_end().to_owned())
}

fn get_battery(battery: &&str) -> io::Result<models::Battery> {
    let status = read_file(format!("/sys/class/power_supply/{}/status", battery))?;

    let energy_now = file_as_number(format!("/sys/class/power_supply/{}/energy_now", battery))?;

    let energy_full = file_as_number(format!("/sys/class/power_supply/{}/energy_full", battery))?;

    let power = file_as_number(format!("/sys/class/power_supply/{}/power_now", battery))?;

    let status = match status.as_ref() {
        "Charging" => models::BatteryStatus::Charging,
        "Unknown" | "Full" if power == 0.0 => models::BatteryStatus::Charged,
        _ => models::BatteryStatus::Discharging,
    };

    Ok(models::Battery {
        power: power,
        energy: energy_now,
        capacity: energy_full,
        status: status,
    })
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

fn get_volume_text(audio_card_name: &str, selem_id: &alsa::mixer::SelemId) -> String {
    let vol = match get_volume(audio_card_name, selem_id) {
        Some(AudioStatus::On(volume_percent)) => format!("{}", volume_percent as i8),
        Some(AudioStatus::Off) => "[off]".to_owned(),
        Some(AudioStatus::Error) | None => "[err]".to_owned(),
    };

    format!("Vol: {}", vol)
}

pub enum AudioStatus {
    On(f64),
    Off,
    Error,
}

fn get_volume(audio_card_name: &str, selem_id: &alsa::mixer::SelemId) -> Option<AudioStatus> {
    let mixer = alsa::mixer::Mixer::new(audio_card_name, true).ok()?;
    let selem = mixer.find_selem(&selem_id)?;

    let (pmin, pmax) = selem.get_playback_volume_range();
    let pvol = selem
        .get_playback_volume(alsa::mixer::SelemChannelId::FrontLeft)
        .ok()?;
    let volume_percent = 100.0 * pvol as f64 / (pmax - pmin) as f64;
    let psw = selem
        .get_playback_switch(alsa::mixer::SelemChannelId::FrontLeft)
        .ok()?;

    if psw == 1 {
        Some(AudioStatus::On(volume_percent))
    } else if psw == 0 {
        Some(AudioStatus::Off)
    } else {
        None
    }
}

fn get_network(
    manager: &network_manager::NetworkManager,
    ethernet: &mut Vec<models::NetworkInterface>,
    wifi: &mut Vec<models::NetworkInterface>,
) -> String {
    let cxn = if let Some(en) = ethernet
        .iter_mut()
        .find(|en: &&mut models::NetworkInterface| en.activated())
    {
        en.status(manager)
    } else if let Some(wl) = wifi
        .iter_mut()
        .find(|wl: &&mut models::NetworkInterface| wl.activated())
    {
        wl.status(manager)
    } else {
        "no connection found".to_owned()
    };

    format!(
        "{}{}",
        cxn,
        if let Some(_tun) = models::NetworkInterface::vpn(&manager)
            .iter()
            .find(|en: &&models::NetworkInterface| en.activated())
        {
            " | vpn"
        } else {
            ""
        }
    )
}

fn main() {
    let (conn, screen_num) = xcb::Connection::connect(None).unwrap();
    let setup = conn.get_setup();
    let root = setup.roots().nth(screen_num as usize).unwrap().root();
    let batteries = vec!["BAT0", "BAT1", "BAT2"];
    let mut battery = batteries
        .iter()
        .filter_map(|bat| get_battery(bat).ok())
        .fold(models::Battery::new(), |mut acc, bat| {
            acc.combine(bat);
            acc
        });
    let audio_card_name = "default";
    let selem_id = alsa::mixer::SelemId::new("Master", 0);
    let manager = network_manager::NetworkManager::new();
    let mut wifi = models::NetworkInterface::wifi(&manager);
    let mut ethernet = models::NetworkInterface::ethernet(&manager);
    let mut date = get_date();
    let mut old_message = String::new();
    // Zero out the initial network counters.
    let _ = get_network(&manager, &mut ethernet, &mut wifi);
    let mut network_output = get_network(&manager, &mut ethernet, &mut wifi);

    let debug = match std::env::var("DEBUG") {
        Ok(_val) => true,
        Err(_) => false,
    };

    let seconds = |seconds| Duration::from_secs(seconds);
    let networkc = channel::tick(seconds(2));
    let statusc = channel::tick(seconds(1));
    let batteryc = channel::tick(seconds(5));
    let datec = channel::tick(seconds(10));

    loop {
        // let eth = models::NetworkInterface::ethernet();
        // if eth.len() > ethernet.len() {
        //     // If a new device has been added, we want to include that in our search for an active
        //     // connection.
        //     ethernet = eth;
        // }
        select! {
            recv(networkc) -> _ => network_output = get_network(&manager, &mut ethernet, &mut wifi),
            recv(datec) -> _ => date = get_date(),
            recv(batteryc) -> _ => {
                battery = batteries
                    .iter()
                    .filter_map(|bat| get_battery(bat).ok())
                    .fold(models::Battery::new(), |mut acc, bat| {
                        acc.combine(bat);
                        acc
                    });
            },
            recv(statusc) -> _ => {
                let message = format!(
                    " {} | {} | {} | {} ",
                    network_output,
                    get_volume_text(audio_card_name, &selem_id),
                    date,
                    battery.status(),
                );
                if message == old_message {
                    continue;
                }
                old_message = message.clone();

                let data = message.as_ptr() as *const c_void;

                if debug {
                    println!("{}", message);
                } else {
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
                }
            }
        }
    }
}
