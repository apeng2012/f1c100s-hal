//! F1C100S Blinky Example
//!
//! 在 PE5 引脚上闪烁 LED

#![no_std]
#![no_main]

use arm9_rt::entry;
use hal::gpio::{DriveStrength, Level, Output};
use {f1c100s_hal as hal, panic_halt as _};

// 简单延时循环
#[inline(never)]
fn delay(count: u32) {
    for _ in 0..count {
        unsafe { core::arch::asm!("nop") };
    }
}

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut led = Output::new(p.PE5, Level::Low, DriveStrength::default());

    loop {
        led.set_high();
        delay(100_000);
        led.set_low();
        delay(100_000);
    }
}
