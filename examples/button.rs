//! Button interrupt example for F1C100S using PE3

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use f1c100s_hal as hal;
use hal::exti::ExtiInput;
use hal::gpio::Pull;
use hal::{bind_interrupts, exti, interrupt, println};

bind_interrupts!(struct Irqs {
    PIOE => exti::InterruptHandler<interrupt::typelevel::PIOE>;
});

#[embassy_executor::main(entry = "arm9_rt::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    let mut button = ExtiInput::new(p.PE3, Pull::Up);

    println!("Press the button on PE3...");

    loop {
        button.wait_for_falling_edge().await;
        println!("Pressed!");

        button.wait_for_rising_edge().await;
        println!("Released!");
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("PANIC: {:?}", info);
    loop {}
}
