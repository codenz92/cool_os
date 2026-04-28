use core::hint::spin_loop;
use x86_64::instructions::{interrupts, port::Port};

#[derive(Clone, Copy)]
pub struct RtcDateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct RawRtcSnapshot {
    second: u8,
    minute: u8,
    hour: u8,
    day: u8,
    month: u8,
    year: u8,
    century: u8,
    status_b: u8,
}

pub fn read_datetime() -> Option<RtcDateTime> {
    interrupts::without_interrupts(|| {
        let raw = read_consistent_snapshot()?;
        let is_binary = raw.status_b & 0x04 != 0;
        let is_24h = raw.status_b & 0x02 != 0;

        let mut second = raw.second;
        let mut minute = raw.minute;
        let mut hour = raw.hour;
        let mut day = raw.day;
        let mut month = raw.month;
        let mut year = raw.year;
        let mut century = raw.century;

        let pm = hour & 0x80 != 0;
        hour &= 0x7f;

        if !is_binary {
            second = bcd_to_bin(second);
            minute = bcd_to_bin(minute);
            hour = bcd_to_bin(hour);
            day = bcd_to_bin(day);
            month = bcd_to_bin(month);
            year = bcd_to_bin(year);
            if century != 0 && century != 0xff {
                century = bcd_to_bin(century);
            }
        }

        if !is_24h {
            hour %= 12;
            if pm {
                hour = hour.saturating_add(12);
            }
        }

        let full_year = if century != 0 && century != 0xff {
            century as u16 * 100 + year as u16
        } else if year >= 70 {
            1900 + year as u16
        } else {
            2000 + year as u16
        };

        if !is_valid(second, minute, hour, day, month) {
            return None;
        }

        Some(RtcDateTime {
            year: full_year,
            month,
            day,
            hour,
            minute,
        })
    })
}

fn read_consistent_snapshot() -> Option<RawRtcSnapshot> {
    for _ in 0..4 {
        wait_until_ready()?;
        let first = read_raw_snapshot();
        wait_until_ready()?;
        let second = read_raw_snapshot();
        if first == second {
            return Some(second);
        }
    }

    wait_until_ready()?;
    Some(read_raw_snapshot())
}

fn wait_until_ready() -> Option<()> {
    for _ in 0..100_000 {
        if read_reg(0x0a) & 0x80 == 0 {
            return Some(());
        }
        spin_loop();
    }
    None
}

fn read_raw_snapshot() -> RawRtcSnapshot {
    RawRtcSnapshot {
        second: read_reg(0x00),
        minute: read_reg(0x02),
        hour: read_reg(0x04),
        day: read_reg(0x07),
        month: read_reg(0x08),
        year: read_reg(0x09),
        century: read_reg(0x32),
        status_b: read_reg(0x0b),
    }
}

fn read_reg(index: u8) -> u8 {
    let mut addr = Port::<u8>::new(0x70);
    let mut data = Port::<u8>::new(0x71);
    unsafe {
        addr.write(index | 0x80);
        data.read()
    }
}

fn bcd_to_bin(value: u8) -> u8 {
    ((value >> 4) * 10) + (value & 0x0f)
}

fn is_valid(second: u8, minute: u8, hour: u8, day: u8, month: u8) -> bool {
    second < 60 && minute < 60 && hour < 24 && day >= 1 && day <= 31 && month >= 1 && month <= 12
}
