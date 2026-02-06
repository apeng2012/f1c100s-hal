#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use f1c100s_hal as hal;
use hal::gpio::{AnyPin, Level, Output};
use hal::{println, Peri};

#[embassy_executor::task(pool_size = 3)]
async fn blink(pin: Peri<'static, AnyPin>, interval_ms: u64) {
    let mut led = Output::new(pin, Level::Low, Default::default());
    let mut count = 0u32;

    loop {
        led.set_high();
        println!("[blink] {} ON", count);
        Timer::after(Duration::from_millis(interval_ms)).await;
        led.set_low();
        println!("[blink] {} OFF", count);
        Timer::after(Duration::from_millis(interval_ms)).await;
        count += 1;
    }
}

#[embassy_executor::main(entry = "arm9_rt::entry")]
async fn main(spawner: Spawner) -> ! {
    let mut config = hal::Config::default();
    {
        use hal::rcc::*;
        config.rcc.pll_cpu = Some(PllCpu::freq_720mhz());
        config.rcc.pll_periph = Some(PllPeriph::freq_600mhz());
        config.rcc.pll_video = Some(PllVideo::freq_198mhz());
        config.rcc.cpu_src = CpuClkSrc::PllCpu;
        config.rcc.ahb_src = AhbClkSrc::PllPeriph;
        config.rcc.ahb_pre_div = AhbPreDiv::Div3; // 600/3 = 200MHz
        config.rcc.ahb_div = AhbDiv::Div1; // 200MHz
        config.rcc.apb_div = ApbDiv::Div2; // 100MHz
    }
    let p = hal::init(config);

    println!("\n=== F1C100S Embassy Blinky ===\n");

    // Spawn blink task
    spawner.spawn(blink(p.PE5.into(), 500)).unwrap();
    println!("Blink task spawned");

    loop {
        Timer::after_millis(5000).await;
        println!("[main] heartbeat");
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("PANIC: {:?}", info);
    loop {}
}
