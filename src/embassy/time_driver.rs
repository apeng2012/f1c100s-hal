//! Time driver implementation for F1C100S using AVS Counter.
//!
//! AVS (Audio Video Sync) Counter is a 32-bit up counter with:
//! - Clock source: 24MHz / (Divisor + 1)
//! - Configurable divisor for each counter
//! - Continuous counting (no reload needed)
//!
//! We configure AVS Counter with divisor = 23, giving 24MHz/24 = 1MHz (1us per tick)
//!
//! # Features
//! - `time-driver-avs0` - Use AVS Counter 0 (default)
//! - `time-driver-avs1` - Use AVS Counter 1

use core::cell::Cell;
use critical_section::CriticalSection;
use embassy_time_driver::Driver;
use f1c100s_pac::{Ccu, Timer};

pub struct TimerDriver {
    // 用于处理 32 位溢出
    last_count: Cell<u32>,
    high_bits: Cell<u32>,
}

unsafe impl Sync for TimerDriver {}

static DRIVER: TimerDriver = TimerDriver {
    last_count: Cell::new(0),
    high_bits: Cell::new(0),
};

impl TimerDriver {
    fn init(&self, _cs: CriticalSection) {
        let ccu = unsafe { Ccu::steal() };
        let timer = unsafe { Timer::steal() };
        
        // 1. 使能 AVS 时钟
        ccu.avs_clk().modify(|_, w| w.sclk_gating().set_bit());
        
        // 2. 设置分频器: 24MHz / 24 = 1MHz (1us per tick)
        // Divisor = 24 - 1 = 23 = 0x17
        timer.avs_cnt_div().write(|w| unsafe {
            w.avs_cnt0_d().bits(0x17)
             .avs_cnt1_d().bits(0x17)
        });
        
        // 3. 清零计数器
        #[cfg(feature = "time-driver-avs0")]
        timer.avs_cnt0().write(|w| unsafe { w.bits(0) });
        #[cfg(feature = "time-driver-avs1")]
        timer.avs_cnt1().write(|w| unsafe { w.bits(0) });
        
        // 4. 使能选定的 AVS Counter
        #[cfg(feature = "time-driver-avs0")]
        timer.avs_cnt_ctl().modify(|_, w| w.avs_cnt0_en().set_bit());
        #[cfg(feature = "time-driver-avs1")]
        timer.avs_cnt_ctl().modify(|_, w| w.avs_cnt1_en().set_bit());
        
        // 初始化溢出跟踪
        self.last_count.set(0);
        self.high_bits.set(0);
    }
    
    /// Get current time in ticks (1MHz = 1us per tick)
    pub fn now(&self) -> u64 {
        critical_section::with(|_cs| {
            let timer = unsafe { Timer::steal() };
            
            #[cfg(feature = "time-driver-avs0")]
            let count = timer.avs_cnt0().read().bits();
            #[cfg(feature = "time-driver-avs1")]
            let count = timer.avs_cnt1().read().bits();
            
            let last = self.last_count.get();
            
            // 检测溢出（当前值小于上次值）
            if count < last {
                self.high_bits.set(self.high_bits.get().wrapping_add(1));
            }
            self.last_count.set(count);
            
            ((self.high_bits.get() as u64) << 32) | (count as u64)
        })
    }
}

impl Driver for TimerDriver {
    fn now(&self) -> u64 {
        TimerDriver::now(self)
    }

    fn schedule_wake(&self, at: u64, waker: &core::task::Waker) {
        // Polling mode: always wake the executor so it keeps polling
        let _ = at;
        waker.wake_by_ref();
    }
}

#[cfg(feature = "_time-driver")]
#[no_mangle]
fn _embassy_time_now() -> u64 {
    DRIVER.now()
}

#[cfg(feature = "_time-driver")]
#[no_mangle]
fn _embassy_time_schedule_wake(at: u64, waker: &core::task::Waker) {
    DRIVER.schedule_wake(at, waker);
}

pub(crate) fn init(cs: CriticalSection) {
    #[cfg(feature = "_time-driver")]
    DRIVER.init(cs);
}
