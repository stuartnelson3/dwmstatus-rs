#![feature(libc)]
extern crate libc;
extern crate xcb;
extern crate chrono;

use chrono::prelude::*;
use xcb::ffi::xproto::xcb_change_property;
use libc::c_void;

fn main() {
    let (conn, screen_num) = xcb::Connection::connect(None).unwrap();
    let setup = conn.get_setup();
    let root = setup.roots().nth(screen_num as usize).unwrap().root();
    let one_sec = std::time::Duration::new(1, 0);

    loop {
        let dt: DateTime<Local> = Local::now();
        let message = format!(
            " tc-73db9 | {}-{}-{} {}:{}",
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
