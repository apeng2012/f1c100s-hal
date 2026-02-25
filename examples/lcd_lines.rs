//! LCD display example: draws lines and rectangles on an 800x480 RGB565 screen.
//!
//! Requires: SPL boot mode, PLL_VIDEO configured, 800x480 RGB HV panel on Port D.

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use f1c100s_hal as hal;
use hal::display::{rgb565, Display, LcdConfig};
use hal::println;

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
        config.rcc.de_dram_gating = true;
    }
    let _p = hal::init(config);

    println!("\n=== F1C200S LCD Lines Demo ===\n");

    // Framebuffer in SDRAM (must not overlap with program/stack)
    // 800*480*2 = 768000 bytes ≈ 750KB
    let fb_addr = 0x8180_0000 as *mut u16;

    let lcd_config = LcdConfig::lcd_800x480();
    let lcd = unsafe { Display::new(&lcd_config, fb_addr) };

    println!("[lcd] display initialized: {}x{}", lcd.width(), lcd.height());

    // Colors
    let red = rgb565(255, 0, 0);
    let green = rgb565(0, 255, 0);
    let blue = rgb565(0, 0, 255);
    let white = rgb565(255, 255, 255);
    let yellow = rgb565(255, 255, 0);
    let cyan = rgb565(0, 255, 255);

    // Clear to black
    lcd.fill(0);

    // Draw a white border
    lcd.draw_rect(0, 0, 800, 480, white);

    // Draw a red cross
    lcd.draw_line(0, 0, 799, 479, red);
    lcd.draw_line(799, 0, 0, 479, red);

    // Draw colored horizontal lines
    for i in 0..10u16 {
        lcd.draw_hline(50, 750, 50 + i * 40, green);
    }

    // Draw colored vertical lines
    for i in 0..10u16 {
        lcd.draw_vline(100 + i * 60, 50, 430, blue);
    }

    // Draw some filled rectangles
    lcd.fill_rect(320, 180, 160, 120, yellow);
    lcd.fill_rect(350, 200, 100, 80, cyan);

    println!("[lcd] drawing complete");

    loop {
        Timer::after(Duration::from_secs(5)).await;
        println!("[lcd] heartbeat");
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("PANIC: {:?}", info);
    loop {}
}
