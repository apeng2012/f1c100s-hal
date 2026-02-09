//! External Interrupt (EINT) driver for F1C100S GPIO
//!
//! F1C100S supports external interrupts on ports PD, PE, and PF.
//! Each port has its own EINT registers:
//! - EINT_CFGx: Configure interrupt trigger type (4 bits per pin)
//! - EINT_CTL: Enable/disable per-pin interrupt
//! - EINT_STA: Pending status (write 1 to clear)
//! - EINT_DEB: Debounce configuration
//!
//! Interrupt trigger types:
//! - 0: Positive edge
//! - 1: Negative edge
//! - 2: High level
//! - 3: Low level
//! - 4: Double edge (both rising and falling)
//!
//! GPIO EINT IRQ numbers in INTC:
//! - IRQ 38: PIOD
//! - IRQ 39: PIOE
//! - IRQ 40: PIOF

use core::future::Future;
use core::marker::PhantomData;
use core::pin::Pin as FuturePin;
use core::task::{Context, Poll};

use embassy_sync::waitqueue::AtomicWaker;
use f1c100s_pac::Pio;

use crate::gpio::{AnyPin, Level, Pin as GpioPin, Pull, SealedPin};
use crate::interrupt::typelevel::Handler;
use crate::{intc, Peri};

/// Wakers for each EINT-capable port
/// Index: pin number within the port
const NEW_AW: AtomicWaker = AtomicWaker::new();
static PD_WAKERS: [AtomicWaker; 22] = [NEW_AW; 22];
static PE_WAKERS: [AtomicWaker; 13] = [NEW_AW; 13];
static PF_WAKERS: [AtomicWaker; 6] = [NEW_AW; 6];

/// EINT trigger type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EintTrigger {
    PositiveEdge = 0,
    NegativeEdge = 1,
    HighLevel = 2,
    LowLevel = 3,
    DoubleEdge = 4,
}

/// Port index for EINT-capable ports
/// Only PD(3), PE(4), PF(5) support EINT
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EintPort {
    PD = 3,
    PE = 4,
    PF = 5,
}

impl EintPort {
    fn from_port_num(port: u8) -> Option<Self> {
        match port {
            3 => Some(EintPort::PD),
            4 => Some(EintPort::PE),
            5 => Some(EintPort::PF),
            _ => None,
        }
    }

    fn wakers(self) -> &'static [AtomicWaker] {
        match self {
            EintPort::PD => &PD_WAKERS,
            EintPort::PE => &PE_WAKERS,
            EintPort::PF => &PF_WAKERS,
        }
    }
}

/// Configure EINT trigger type for a pin
fn set_eint_trigger(port: EintPort, pin: u8, trigger: EintTrigger) {
    let pio = unsafe { Pio::steal() };
    let cfg_reg = pin / 8; // CFG0-CFG3, 8 pins per register
    let cfg_offset = ((pin % 8) * 4) as usize;
    let trigger_val = trigger as u32;

    // Each port has 4 EINT_CFG registers, use modify with raw bits
    match (port, cfg_reg) {
        (EintPort::PD, 0) => {
            pio.pd_eint_cfg0()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0xF << cfg_offset)) | (trigger_val << cfg_offset)) });
        }
        (EintPort::PD, 1) => {
            pio.pd_eint_cfg1()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0xF << cfg_offset)) | (trigger_val << cfg_offset)) });
        }
        (EintPort::PD, 2) => {
            pio.pd_eint_cfg2()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0xF << cfg_offset)) | (trigger_val << cfg_offset)) });
        }
        (EintPort::PD, 3) => {
            pio.pd_eint_cfg3()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0xF << cfg_offset)) | (trigger_val << cfg_offset)) });
        }
        (EintPort::PE, 0) => {
            pio.pe_eint_cfg0()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0xF << cfg_offset)) | (trigger_val << cfg_offset)) });
        }
        (EintPort::PE, 1) => {
            pio.pe_eint_cfg1()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0xF << cfg_offset)) | (trigger_val << cfg_offset)) });
        }
        (EintPort::PE, 2) => {
            pio.pe_eint_cfg2()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0xF << cfg_offset)) | (trigger_val << cfg_offset)) });
        }
        (EintPort::PE, 3) => {
            pio.pe_eint_cfg3()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0xF << cfg_offset)) | (trigger_val << cfg_offset)) });
        }
        (EintPort::PF, 0) => {
            pio.pf_eint_cfg0()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0xF << cfg_offset)) | (trigger_val << cfg_offset)) });
        }
        (EintPort::PF, 1) => {
            pio.pf_eint_cfg1()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0xF << cfg_offset)) | (trigger_val << cfg_offset)) });
        }
        (EintPort::PF, 2) => {
            pio.pf_eint_cfg2()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0xF << cfg_offset)) | (trigger_val << cfg_offset)) });
        }
        (EintPort::PF, 3) => {
            pio.pf_eint_cfg3()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0xF << cfg_offset)) | (trigger_val << cfg_offset)) });
        }
        _ => {}
    }
}

/// Enable EINT for a pin (EINT_CTL register)
fn enable_eint_pin(port: EintPort, pin: u8) {
    let pio = unsafe { Pio::steal() };
    let bit = 1u32 << pin;
    match port {
        EintPort::PD => {
            pio.pd_eint_ctl().modify(|r, w| unsafe { w.bits(r.bits() | bit) });
        }
        EintPort::PE => {
            pio.pe_eint_ctl().modify(|r, w| unsafe { w.bits(r.bits() | bit) });
        }
        EintPort::PF => {
            pio.pf_eint_ctl().modify(|r, w| unsafe { w.bits(r.bits() | bit) });
        }
    }
}

/// Disable EINT for a pin
fn disable_eint_pin(port: EintPort, pin: u8) {
    let pio = unsafe { Pio::steal() };
    let bit = 1u32 << pin;
    match port {
        EintPort::PD => {
            pio.pd_eint_ctl().modify(|r, w| unsafe { w.bits(r.bits() & !bit) });
        }
        EintPort::PE => {
            pio.pe_eint_ctl().modify(|r, w| unsafe { w.bits(r.bits() & !bit) });
        }
        EintPort::PF => {
            pio.pf_eint_ctl().modify(|r, w| unsafe { w.bits(r.bits() & !bit) });
        }
    }
}

/// Read EINT status (EINT_STA register)
fn read_eint_status(port: EintPort) -> u32 {
    let pio = unsafe { Pio::steal() };
    match port {
        EintPort::PD => pio.pd_eint_sta().read().bits(),
        EintPort::PE => pio.pe_eint_sta().read().bits(),
        EintPort::PF => pio.pf_eint_sta().read().bits(),
    }
}

/// Clear EINT status (write 1 to clear)
fn clear_eint_status(port: EintPort, bits: u32) {
    let pio = unsafe { Pio::steal() };
    match port {
        EintPort::PD => {
            pio.pd_eint_sta().write(|w| unsafe { w.bits(bits) });
        }
        EintPort::PE => {
            pio.pe_eint_sta().write(|w| unsafe { w.bits(bits) });
        }
        EintPort::PF => {
            pio.pf_eint_sta().write(|w| unsafe { w.bits(bits) });
        }
    }
}

/// Read EINT_CTL register
fn read_eint_ctl(port: EintPort) -> u32 {
    let pio = unsafe { Pio::steal() };
    match port {
        EintPort::PD => pio.pd_eint_ctl().read().bits(),
        EintPort::PE => pio.pe_eint_ctl().read().bits(),
        EintPort::PF => pio.pf_eint_ctl().read().bits(),
    }
}

/// Write EINT_CTL register (clear specific bits)
fn write_eint_ctl_clear_bits(port: EintPort, clear_mask: u32) {
    let pio = unsafe { Pio::steal() };
    match port {
        EintPort::PD => {
            pio.pd_eint_ctl()
                .modify(|r, w| unsafe { w.bits(r.bits() & !clear_mask) });
        }
        EintPort::PE => {
            pio.pe_eint_ctl()
                .modify(|r, w| unsafe { w.bits(r.bits() & !clear_mask) });
        }
        EintPort::PF => {
            pio.pf_eint_ctl()
                .modify(|r, w| unsafe { w.bits(r.bits() & !clear_mask) });
        }
    }
}

/// Set pin mode to EINT function (mode 6 in CFG register)
fn set_pin_eint_mode(port: u8, pin: u8) {
    let pio = unsafe { Pio::steal() };
    let cfg_reg = pin / 8;
    let cfg_offset = ((pin % 8) * 4) as usize;
    // EINT function = 6 for PD/PE/PF
    let mode_val = 6u32;

    match (port, cfg_reg) {
        (3, 0) => {
            pio.pd_cfg0()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0x7 << cfg_offset)) | (mode_val << cfg_offset)) });
        }
        (3, 1) => {
            pio.pd_cfg1()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0x7 << cfg_offset)) | (mode_val << cfg_offset)) });
        }
        (3, 2) => {
            pio.pd_cfg2()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0x7 << cfg_offset)) | (mode_val << cfg_offset)) });
        }
        (3, 3) => {
            pio.pd_cfg3()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0x7 << cfg_offset)) | (mode_val << cfg_offset)) });
        }
        (4, 0) => {
            pio.pe_cfg0()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0x7 << cfg_offset)) | (mode_val << cfg_offset)) });
        }
        (4, 1) => {
            pio.pe_cfg1()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0x7 << cfg_offset)) | (mode_val << cfg_offset)) });
        }
        (4, 2) => {
            pio.pe_cfg2()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0x7 << cfg_offset)) | (mode_val << cfg_offset)) });
        }
        (4, 3) => {
            pio.pe_cfg3()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0x7 << cfg_offset)) | (mode_val << cfg_offset)) });
        }
        (5, 0) => {
            pio.pf_cfg0()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0x7 << cfg_offset)) | (mode_val << cfg_offset)) });
        }
        (5, 1) => {
            pio.pf_cfg1()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0x7 << cfg_offset)) | (mode_val << cfg_offset)) });
        }
        (5, 2) => {
            pio.pf_cfg2()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0x7 << cfg_offset)) | (mode_val << cfg_offset)) });
        }
        (5, 3) => {
            pio.pf_cfg3()
                .modify(|r, w| unsafe { w.bits((r.bits() & !(0x7 << cfg_offset)) | (mode_val << cfg_offset)) });
        }
        _ => {}
    }
}

/// Read pin data from the port DATA register
fn read_pin_data(port: u8, pin: u8) -> bool {
    let pio = unsafe { Pio::steal() };
    let val = match port {
        3 => pio.pd_data().read().bits(),
        4 => pio.pe_data().read().bits(),
        5 => pio.pf_data().read().bits(),
        _ => return false,
    };
    (val >> pin) & 1 != 0
}

/// IRQ handler for PIOD (IRQ 38)
fn piod_irq_handler() {
    let status = read_eint_status(EintPort::PD);
    clear_eint_status(EintPort::PD, status);
    // Disable triggered pins (will be re-enabled on next wait)
    write_eint_ctl_clear_bits(EintPort::PD, status);
    // Wake tasks
    for pin in BitIter(status) {
        if (pin as usize) < PD_WAKERS.len() {
            PD_WAKERS[pin as usize].wake();
        }
    }
}

/// IRQ handler for PIOE (IRQ 39)
fn pioe_irq_handler() {
    let status = read_eint_status(EintPort::PE);
    clear_eint_status(EintPort::PE, status);
    write_eint_ctl_clear_bits(EintPort::PE, status);
    for pin in BitIter(status) {
        if (pin as usize) < PE_WAKERS.len() {
            PE_WAKERS[pin as usize].wake();
        }
    }
}

/// IRQ handler for PIOF (IRQ 40)
fn piof_irq_handler() {
    let status = read_eint_status(EintPort::PF);
    clear_eint_status(EintPort::PF, status);
    write_eint_ctl_clear_bits(EintPort::PF, status);
    for pin in BitIter(status) {
        if (pin as usize) < PF_WAKERS.len() {
            PF_WAKERS[pin as usize].wake();
        }
    }
}

struct BitIter(u32);

impl Iterator for BitIter {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.trailing_zeros() {
            32 => None,
            b => {
                self.0 &= !(1 << b);
                Some(b)
            }
        }
    }
}

/// Initialize EXTI subsystem: register IRQ handlers and enable GPIO IRQs in INTC.
///
/// # Safety
/// Must be called once during HAL init, after INTC init.
pub(crate) unsafe fn init() {
    intc::set_irq_handler(intc::IRQ_PIOD, piod_irq_handler);
    intc::set_irq_handler(intc::IRQ_PIOE, pioe_irq_handler);
    intc::set_irq_handler(intc::IRQ_PIOF, piof_irq_handler);

    intc::enable_irq(intc::IRQ_PIOD);
    intc::enable_irq(intc::IRQ_PIOE);
    intc::enable_irq(intc::IRQ_PIOF);
}

/// Interrupt handler for GPIO external interrupts.
///
/// Use with `bind_interrupts!` for compile-time binding:
/// ```ignore
/// bind_interrupts!(struct Irqs {
///     PIOE => exti::InterruptHandler<interrupt::typelevel::PIOE>;
/// });
/// ```
///
/// Note: EXTI handlers are automatically registered during HAL init,
/// so `bind_interrupts!` is optional for EXTI. It's provided for
/// consistency with the embassy pattern and for future peripheral drivers.
pub struct InterruptHandler<I> {
    _phantom: core::marker::PhantomData<I>,
}

impl Handler<crate::interrupt::typelevel::PIOD> for InterruptHandler<crate::interrupt::typelevel::PIOD> {
    unsafe fn on_interrupt() {
        piod_irq_handler();
    }
}

impl Handler<crate::interrupt::typelevel::PIOE> for InterruptHandler<crate::interrupt::typelevel::PIOE> {
    unsafe fn on_interrupt() {
        pioe_irq_handler();
    }
}

impl Handler<crate::interrupt::typelevel::PIOF> for InterruptHandler<crate::interrupt::typelevel::PIOF> {
    unsafe fn on_interrupt() {
        piof_irq_handler();
    }
}

/// EXTI input driver for F1C100S GPIO external interrupts.
///
/// Only pins on ports PD, PE, PF support external interrupts.
pub struct ExtiInput<'d> {
    port: EintPort,
    port_num: u8,
    pin_num: u8,
    _phantom: PhantomData<&'d ()>,
}

impl<'d> Unpin for ExtiInput<'d> {}

impl<'d> ExtiInput<'d> {
    /// Create a new ExtiInput.
    ///
    /// The pin must be on port PD, PE, or PF (only these support EINT).
    /// Panics if the pin is on a port that doesn't support external interrupts.
    pub fn new<P: GpioPin + Into<AnyPin>>(pin: Peri<'d, P>, pull: Pull) -> Self {
        let pin: Peri<'d, AnyPin> = pin.into();
        let port_num = pin._port();
        let pin_num = pin._pin();

        let port = EintPort::from_port_num(port_num).expect("ExtiInput: only PD, PE, PF support external interrupts");

        // Set pin to EINT function mode (mode 6)
        set_pin_eint_mode(port_num, pin_num);

        // Set pull
        pin.set_pull(pull);

        Self {
            port,
            port_num,
            pin_num,
            _phantom: PhantomData,
        }
    }

    pub fn is_high(&self) -> bool {
        read_pin_data(self.port_num, self.pin_num)
    }

    pub fn is_low(&self) -> bool {
        !read_pin_data(self.port_num, self.pin_num)
    }

    pub fn get_level(&self) -> Level {
        read_pin_data(self.port_num, self.pin_num).into()
    }

    pub async fn wait_for_high(&mut self) {
        if self.is_high() {
            return;
        }
        ExtiInputFuture::new(self.port, self.pin_num, EintTrigger::HighLevel).await
    }

    pub async fn wait_for_low(&mut self) {
        if self.is_low() {
            return;
        }
        ExtiInputFuture::new(self.port, self.pin_num, EintTrigger::LowLevel).await
    }

    pub async fn wait_for_rising_edge(&mut self) {
        ExtiInputFuture::new(self.port, self.pin_num, EintTrigger::PositiveEdge).await
    }

    pub async fn wait_for_falling_edge(&mut self) {
        ExtiInputFuture::new(self.port, self.pin_num, EintTrigger::NegativeEdge).await
    }

    pub async fn wait_for_any_edge(&mut self) {
        ExtiInputFuture::new(self.port, self.pin_num, EintTrigger::DoubleEdge).await
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
struct ExtiInputFuture<'a> {
    port: EintPort,
    pin: u8,
    phantom: PhantomData<&'a mut AnyPin>,
}

impl<'a> ExtiInputFuture<'a> {
    fn new(port: EintPort, pin: u8, trigger: EintTrigger) -> Self {
        critical_section::with(|_| {
            // Configure trigger type
            set_eint_trigger(port, pin, trigger);

            // Clear any pending status for this pin
            clear_eint_status(port, 1 << pin);

            // Enable EINT for this pin
            enable_eint_pin(port, pin);
        });

        Self {
            port,
            pin,
            phantom: PhantomData,
        }
    }
}

impl<'a> Drop for ExtiInputFuture<'a> {
    fn drop(&mut self) {
        critical_section::with(|_| {
            disable_eint_pin(self.port, self.pin);
        });
    }
}

impl<'a> Future for ExtiInputFuture<'a> {
    type Output = ();

    fn poll(self: FuturePin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let wakers = self.port.wakers();
        wakers[self.pin as usize].register(cx.waker());

        // Check if EINT_CTL bit was cleared by the IRQ handler
        let ctl = read_eint_ctl(self.port);
        if ctl & (1 << self.pin) == 0 {
            // IRQ handler disabled this pin's EINT -> interrupt fired
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
