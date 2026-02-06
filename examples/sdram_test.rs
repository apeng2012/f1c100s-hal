//! SDRAM initialization and test example for F1C100S.
//!
//! Initializes DDR1 SDRAM and runs basic memory tests.
//! Results are printed via UART debug output (115200 baud).

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use f1c100s_hal as hal;
use hal::println;

const SDRAM_BASE: u32 = 0x8000_0000;

#[inline(always)]
unsafe fn read32(addr: u32) -> u32 {
    core::ptr::read_volatile(addr as *const u32)
}

#[inline(always)]
unsafe fn write32(addr: u32, val: u32) {
    core::ptr::write_volatile(addr as *mut u32, val);
}

/// Sequential pattern test
unsafe fn test_seq(base: u32, count: u32) -> bool {
    for i in 0..count {
        write32(base + i * 4, base + i * 4);
    }
    for i in 0..count {
        if read32(base + i * 4) != base + i * 4 {
            return false;
        }
    }
    true
}

/// Walking ones test
unsafe fn test_walk1(base: u32) -> bool {
    for bit in 0..32u32 {
        let pat = 1u32 << bit;
        write32(base, pat);
        if read32(base) != pat {
            return false;
        }
    }
    true
}

/// Alternating pattern test
unsafe fn test_alt(base: u32, count: u32) -> bool {
    for i in 0..count {
        write32(base + i * 4, 0x5555_5555);
    }
    for i in 0..count {
        if read32(base + i * 4) != 0x5555_5555 {
            return false;
        }
    }
    for i in 0..count {
        write32(base + i * 4, 0xAAAA_AAAA);
    }
    for i in 0..count {
        if read32(base + i * 4) != 0xAAAA_AAAA {
            return false;
        }
    }
    true
}

fn pass_fail(ok: bool) -> &'static str {
    if ok {
        "PASS"
    } else {
        "FAIL"
    }
}

#[embassy_executor::main(entry = "arm9_rt::entry")]
async fn main(_spawner: Spawner) -> ! {
    let _p = hal::init(Default::default());

    println!("\n=== SDRAM Test ===\n");

    println!("Init DRAM...");
    match hal::dram::init() {
        Some(info) => {
            println!("OK {}MB", info.size_mb);
        }
        None => {
            println!("FAIL!");
            loop {
                Timer::after(Duration::from_secs(1)).await;
            }
        }
    }

    // Test 1: Walking ones
    let r = unsafe { test_walk1(SDRAM_BASE) };
    println!("Walk1: {}", pass_fail(r));

    // Test 2: Sequential 4KB
    let r = unsafe { test_seq(SDRAM_BASE, 1024) };
    println!("Seq 4K: {}", pass_fail(r));

    // Test 3: Alternating 1KB
    let r = unsafe { test_alt(SDRAM_BASE, 256) };
    println!("Alt 1K: {}", pass_fail(r));

    // Test 4: Sequential at different offsets
    let offsets: [u32; 4] = [0, 8 << 20, 16 << 20, 24 << 20];
    for &off in &offsets {
        let r = unsafe { test_seq(SDRAM_BASE + off, 1024) };
        println!("Seq@+{}M: {}", off >> 20, pass_fail(r));
    }

    println!("\n=== Done ===");

    loop {
        Timer::after(Duration::from_secs(5)).await;
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    println!("PANIC!");
    loop {}
}
