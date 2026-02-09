//! Interrupt Controller (INTC) driver for F1C100S
//!
//! F1C100S INTC handles 64 interrupt sources with 4-level priority.
//!
//! IRQ source mapping (key ones):
//! - 0: NMI
//! - 1-3: UART0-2
//! - 7-9: TWI0-2
//! - 10-11: SPI0-1
//! - 13-15: Timer0-2
//! - 18: DMA
//! - 26: USB-OTG
//! - 38: PIOD (GPIO Port D external interrupt)
//! - 39: PIOE (GPIO Port E external interrupt)
//! - 40: PIOF (GPIO Port F external interrupt)

use f1c100s_pac::Intc;

/// Total number of IRQ sources
pub const IRQ_COUNT: usize = 64;

/// IRQ numbers for GPIO ports
pub const IRQ_PIOD: u8 = 38;
pub const IRQ_PIOE: u8 = 39;
pub const IRQ_PIOF: u8 = 40;

/// IRQ handler function type
pub type IrqHandler = fn();

/// IRQ dispatch table
static mut IRQ_TABLE: [Option<IrqHandler>; IRQ_COUNT] = [None; IRQ_COUNT];

/// Initialize the INTC controller.
///
/// Disables all interrupts, clears all pending, resets masks and fast-forcing.
pub unsafe fn init() {
    let intc = Intc::steal();

    // Disable all interrupts
    intc.intc_en_reg0().write(|w| w.bits(0));
    intc.intc_en_reg1().write(|w| w.bits(0));

    // Unmask all (mask=0 means not masked)
    intc.intc_mask_reg0().write(|w| w.bits(0));
    intc.intc_mask_reg1().write(|w| w.bits(0));

    // Clear fast forcing
    intc.intc_ff_reg0().write(|w| w.bits(0));
    intc.intc_ff_reg1().write(|w| w.bits(0));

    // Clear response
    intc.intc_resp_reg0().write(|w| w.bits(0));
    intc.intc_resp_reg1().write(|w| w.bits(0));

    // Clear all pending (write 1 to clear)
    intc.intc_pend_reg0().write(|w| w.bits(0xFFFF_FFFF));
    intc.intc_pend_reg1().write(|w| w.bits(0xFFFF_FFFF));

    // Reset base address
    intc.intc_base_addr().write(|w| w.bits(0));

    // Reset NMI control (match Keil reference)
    intc.nmi_int_ctrl().write(|w| w.bits(0));

    // Clear dispatch table
    for slot in IRQ_TABLE.iter_mut() {
        *slot = None;
    }
}

/// Register an IRQ handler for the given IRQ number.
pub fn set_irq_handler(irq: u8, handler: IrqHandler) {
    critical_section::with(|_| unsafe {
        if (irq as usize) < IRQ_COUNT {
            IRQ_TABLE[irq as usize] = Some(handler);
        }
    });
}

/// Enable an IRQ source in the INTC.
pub fn enable_irq(irq: u8) {
    critical_section::with(|_| {
        let intc = unsafe { Intc::steal() };
        let reg_idx = irq / 32;
        let bit = 1u32 << (irq % 32);
        if reg_idx == 0 {
            intc.intc_en_reg0().modify(|r, w| unsafe { w.bits(r.bits() | bit) });
        } else {
            intc.intc_en_reg1().modify(|r, w| unsafe { w.bits(r.bits() | bit) });
        }
    });
}

/// Disable an IRQ source in the INTC.
pub fn disable_irq(irq: u8) {
    critical_section::with(|_| {
        let intc = unsafe { Intc::steal() };
        let reg_idx = irq / 32;
        let bit = 1u32 << (irq % 32);
        if reg_idx == 0 {
            intc.intc_en_reg0().modify(|r, w| unsafe { w.bits(r.bits() & !bit) });
        } else {
            intc.intc_en_reg1().modify(|r, w| unsafe { w.bits(r.bits() & !bit) });
        }
    });
}

/// Clear pending for an IRQ (write 1 to clear).
pub fn clear_pending(irq: u8) {
    let intc = unsafe { Intc::steal() };
    let reg_idx = irq / 32;
    let bit = 1u32 << (irq % 32);
    if reg_idx == 0 {
        intc.intc_pend_reg0().write(|w| unsafe { w.bits(bit) });
    } else {
        intc.intc_pend_reg1().write(|w| unsafe { w.bits(bit) });
    }
}

/// Check if an IRQ is enabled in the INTC.
pub fn is_irq_enabled(irq: u8) -> bool {
    let intc = unsafe { Intc::steal() };
    let reg_idx = irq / 32;
    let bit = 1u32 << (irq % 32);
    if reg_idx == 0 {
        intc.intc_en_reg0().read().bits() & bit != 0
    } else {
        intc.intc_en_reg1().read().bits() & bit != 0
    }
}

/// Force an IRQ via fast-forcing (software trigger).
pub fn force_irq(irq: u8) {
    critical_section::with(|_| {
        let intc = unsafe { Intc::steal() };
        let reg_idx = irq / 32;
        let bit = 1u32 << (irq % 32);
        if reg_idx == 0 {
            intc.intc_ff_reg0().write(|w| unsafe { w.bits(bit) });
        } else {
            intc.intc_ff_reg1().write(|w| unsafe { w.bits(bit) });
        }
    });
}

/// Get the currently active IRQ number from INTC_VECTOR_REG.
#[inline]
fn get_active_irq() -> u8 {
    let intc = unsafe { Intc::steal() };
    ((intc.intc_vector().read().bits() >> 2) & 0x3F) as u8
}

/// Dispatch the IRQ to the registered handler.
fn dispatch(irq: u8) {
    let handler = unsafe { IRQ_TABLE[irq as usize] };
    if let Some(h) = handler {
        h();
    }
}

/// IRQ handler entry point - called from the ARM9 vector table.
///
/// This function:
/// 1. Saves context
/// 2. Reads the active IRQ number from INTC
/// 3. Clears the fast-forcing flag
/// 4. Dispatches to the registered handler
/// 5. Clears the pending bit
/// 6. Restores context and returns from IRQ

/// Internal dispatch function called from the arm9-rt IRQ asm wrapper.
/// arm9-rt 的 IRQ wrapper 会调用 __irq_handler（通过 bl），
/// 已经处理了 sub lr, #4 和 context save/restore。
#[no_mangle]
unsafe extern "C" fn __irq_handler() {
    let irq = get_active_irq();

    // Clear fast-forcing flag
    let intc = Intc::steal();
    let reg_idx = irq / 32;
    let bit = 1u32 << (irq % 32);
    if reg_idx == 0 {
        intc.intc_ff_reg0().modify(|r, w| w.bits(r.bits() & !bit));
    } else {
        intc.intc_ff_reg1().modify(|r, w| w.bits(r.bits() & !bit));
    }

    // Dispatch
    dispatch(irq);

    // Clear pending
    clear_pending(irq);
}
