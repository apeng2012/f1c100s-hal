//! SPI Flash DMA read example — reads 256 bytes from address 0x000000

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use f1c100s_hal as hal;
use hal::println;
use hal::spi::{ChipSelect, Config, Spi};

const CMD_READ: u8 = 0x03;

fn hex(n: u8) -> u8 {
    if n < 10 {
        b'0' + n
    } else {
        b'A' + n - 10
    }
}

fn print_hex_line(off: usize, data: &[u8]) {
    let mut line = [0u8; 80];
    let mut p = 0;
    for &d in &[
        hex((off >> 12) as u8 & 0xF),
        hex((off >> 8) as u8 & 0xF),
        hex((off >> 4) as u8 & 0xF),
        hex(off as u8 & 0xF),
    ] {
        line[p] = d;
        p += 1;
    }
    line[p] = b':';
    p += 1;
    for &byte in data {
        line[p] = b' ';
        p += 1;
        line[p] = hex(byte >> 4);
        p += 1;
        line[p] = hex(byte & 0xF);
        p += 1;
    }
    println!("{}", unsafe { core::str::from_utf8_unchecked(&line[..p]) });
}

#[embassy_executor::main(entry = "arm9_rt::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init(Default::default());
    println!("\n=== SPI Flash DMA Test ===\n");

    // --- SPI DMA read from flash ---
    println!("[spi] configuring SPI0...");
    let mut cfg = Config::default();
    cfg.frequency = hal::time::Hertz(10_000_000);
    cfg.cs = ChipSelect::Ss0;

    let mut spi = Spi::new(p.SPI0, p.PC0, p.PC3, p.PC2, p.PC1, cfg);
    println!("[spi] SPI0 ready");

    let tx = [CMD_READ, 0, 0, 0];
    let mut buf = [0u8; 256];

    spi.cs_low();
    println!("[spi] starting DMA transfer (dst={:#010X})...", buf.as_mut_ptr() as u32);
    match spi.dma_transfer_blocking(0, &tx, &mut buf) {
        Ok(()) => println!("[spi] DMA transfer OK"),
        Err(e) => println!("[spi] DMA transfer error: {:?}", e),
    }
    spi.cs_high();

    println!("\nFlash data @ 0x000000:");
    for i in 0..4 {
        print_hex_line(i * 16, &buf[i * 16..(i + 1) * 16]);
    }

    println!("\n=== Done ===");
    loop {
        Timer::after(Duration::from_secs(5)).await;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("PANIC: {:?}", info);
    loop {}
}
