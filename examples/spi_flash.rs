//! W25Q128 SPI Flash read example
//!
//! Reads the JEDEC ID and first 256 bytes from a W25Q128 flash chip
//! connected to SPI0:
//!   Pin 59 = PC0 = CLK
//!   Pin 60 = PC1 = CS
//!   Pin 61 = PC2 = MISO
//!   Pin 62 = PC3 = MOSI

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use f1c100s_hal as hal;
use hal::println;
use hal::spi::{self, ChipSelect, Config, Spi};

/// W25Q128 commands
const CMD_READ_JEDEC_ID: u8 = 0x9F;
const CMD_READ_DATA: u8 = 0x03;
const CMD_READ_STATUS_REG1: u8 = 0x05;
const CMD_READ_UNIQUE_ID: u8 = 0x4B;

/// Read JEDEC ID (manufacturer + device type + capacity)
fn read_jedec_id(spi: &mut Spi<'_, impl spi::Instance>) -> [u8; 3] {
    let tx = [CMD_READ_JEDEC_ID];
    let mut rx = [0u8; 3];
    spi.cs_low();
    spi.transfer(&tx, &mut rx).ok();
    spi.cs_high();
    rx
}

/// Read status register 1
fn read_status(spi: &mut Spi<'_, impl spi::Instance>) -> u8 {
    let tx = [CMD_READ_STATUS_REG1];
    let mut rx = [0u8; 1];
    spi.cs_low();
    spi.transfer(&tx, &mut rx).ok();
    spi.cs_high();
    rx[0]
}

/// Read data from flash at given 24-bit address
fn read_flash(spi: &mut Spi<'_, impl spi::Instance>, addr: u32, buf: &mut [u8]) {
    let tx = [CMD_READ_DATA, (addr >> 16) as u8, (addr >> 8) as u8, addr as u8];
    spi.cs_low();
    spi.transfer(&tx, buf).ok();
    spi.cs_high();
}

/// Read unique 64-bit ID
fn read_unique_id(spi: &mut Spi<'_, impl spi::Instance>) -> [u8; 8] {
    // Command + 4 dummy bytes, then 8 bytes of unique ID
    let tx = [CMD_READ_UNIQUE_ID, 0, 0, 0, 0];
    let mut rx = [0u8; 8];
    spi.cs_low();
    spi.transfer(&tx, &mut rx).ok();
    spi.cs_high();
    rx
}

#[embassy_executor::main(entry = "arm9_rt::entry")]
async fn main(_spawner: Spawner) -> ! {
    let mut config = hal::Config::default();
    {
        use hal::rcc::*;
        config.rcc.pll_cpu = Some(PllCpu::freq_720mhz());
        config.rcc.pll_periph = Some(PllPeriph::freq_600mhz());
        config.rcc.pll_video = Some(PllVideo::freq_198mhz());
        config.rcc.cpu_src = CpuClkSrc::PllCpu;
        config.rcc.ahb_src = AhbClkSrc::PllPeriph;
        config.rcc.ahb_pre_div = AhbPreDiv::Div3;
        config.rcc.ahb_div = AhbDiv::Div1;
        config.rcc.apb_div = ApbDiv::Div2;
    }
    let p = hal::init(config);

    println!("\n=== W25Q128 SPI Flash Test ===\n");

    // SPI0 config: Mode 0, 10MHz, MSB first
    let mut spi_cfg = Config::default();
    spi_cfg.frequency = hal::time::Hertz(10_000_000);
    spi_cfg.cs = ChipSelect::Ss0;

    // SPI0 pins: PC0=CLK, PC3=MOSI, PC2=MISO, PC1=CS
    let mut spi = Spi::new(p.SPI0, p.PC0, p.PC3, p.PC2, p.PC1, spi_cfg);

    // 1. Read JEDEC ID
    let id = read_jedec_id(&mut spi);
    println!(
        "JEDEC ID: manufacturer=0x{:02X}, type=0x{:02X}, capacity=0x{:02X}",
        id[0], id[1], id[2]
    );

    // W25Q128: manufacturer=0xEF (Winbond), type=0x40, capacity=0x18 (128Mbit)
    if id[0] == 0xEF && id[1] == 0x40 && id[2] == 0x18 {
        println!("  -> W25Q128 detected!");
    } else if id[0] == 0xFF && id[1] == 0xFF {
        println!("  -> No flash detected (all 0xFF). Check wiring.");
    } else {
        println!("  -> Unknown flash device");
    }

    // 2. Read status register
    let status = read_status(&mut spi);
    println!("Status Register 1: 0x{:02X}", status);

    // 3. Read unique ID
    let uid = read_unique_id(&mut spi);
    println!(
        "Unique ID: {:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        uid[0], uid[1], uid[2], uid[3], uid[4], uid[5], uid[6], uid[7]
    );

    // 4. Read first 256 bytes from address 0x000000
    let mut buf = [0u8; 256];
    read_flash(&mut spi, 0x000000, &mut buf);

    println!("\nFirst 256 bytes from address 0x000000:");
    for (i, chunk) in buf.chunks(16).enumerate() {
        print_hex_line(i * 16, chunk);
    }

    println!("\n=== SPI Flash Test Complete ===");

    loop {
        Timer::after(Duration::from_secs(1)).await;
    }
}

fn print_hex_line(addr: usize, data: &[u8]) {
    // Build hex string manually (no alloc)
    let mut line = [0u8; 80];
    let mut pos = 0;

    // Address
    let addr_hex = [
        hex_char((addr >> 12) as u8 & 0xF),
        hex_char((addr >> 8) as u8 & 0xF),
        hex_char((addr >> 4) as u8 & 0xF),
        hex_char(addr as u8 & 0xF),
    ];
    for &c in &addr_hex {
        line[pos] = c;
        pos += 1;
    }
    line[pos] = b':';
    pos += 1;
    line[pos] = b' ';
    pos += 1;

    for &byte in data {
        line[pos] = hex_char(byte >> 4);
        pos += 1;
        line[pos] = hex_char(byte & 0xF);
        pos += 1;
        line[pos] = b' ';
        pos += 1;
    }

    // Safety: we only wrote ASCII bytes
    let s = unsafe { core::str::from_utf8_unchecked(&line[..pos]) };
    println!("{}", s);
}

fn hex_char(nibble: u8) -> u8 {
    match nibble {
        0..=9 => b'0' + nibble,
        _ => b'A' + nibble - 10,
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("PANIC: {:?}", info);
    loop {}
}
