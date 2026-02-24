//! DMA driver for F1C100S/F1C200S
//!
//! The F1C100S has 4 Normal DMA (NDMA) channels suitable for SPI, UART, etc.
//! Each NDMA channel supports up to 128KB per transfer.
//!
//! DRQ types for NDMA:
//! - SPI0_RX=0x04, SPI0_TX=0x04
//! - SPI1_RX=0x05, SPI1_TX=0x05
//! - SRAM=0x10, SDRAM=0x11

#![macro_use]

use crate::pac;

pub mod word;

/// NDMA channel count
pub const NDMA_COUNT: usize = 4;

/// DRQ type for NDMA source/destination
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum NdmaDrqType {
    IrRx = 0x00,
    OwaTx = 0x01,
    Spi0 = 0x04,
    Spi1 = 0x05,
    Uart0Rx = 0x08,
    Uart1Rx = 0x09,
    Uart2Rx = 0x0A,
    AudioCodec = 0x0C,
    TpAdc = 0x0D,
    Daudio = 0x0E,
    Sram = 0x10,
    Sdram = 0x11,
    Usb = 0x14,
}

impl NdmaDrqType {
    /// Auto-detect the correct memory DRQ type based on address.
    /// Addresses >= 0x8000_0000 are SDRAM, otherwise SRAM.
    #[inline]
    pub fn for_addr(addr: u32) -> Self {
        if addr >= 0x8000_0000 {
            NdmaDrqType::Sdram
        } else {
            NdmaDrqType::Sram
        }
    }
}

/// DMA data width
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum DataWidth {
    Bit8 = 0,
    Bit16 = 1,
    Bit32 = 2,
}

/// DMA address type
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum AddrType {
    /// Linear (auto-increment)
    Linear = 0,
    /// IO (fixed address)
    Io = 1,
}

/// DMA burst length
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum BurstLen {
    Single = 0,
    Burst4 = 1,
}

/// NDMA transfer configuration
#[derive(Debug, Copy, Clone)]
pub struct NdmaConfig {
    pub src_drq: NdmaDrqType,
    pub src_addr_type: AddrType,
    pub src_burst: BurstLen,
    pub src_width: DataWidth,
    pub dst_drq: NdmaDrqType,
    pub dst_addr_type: AddrType,
    pub dst_burst: BurstLen,
    pub dst_width: DataWidth,
    /// Wait state (0..=7), clock cycles = 2^wait_state
    pub wait_state: u8,
    /// Continuous mode (auto-reload)
    pub continuous: bool,
}

impl Default for NdmaConfig {
    fn default() -> Self {
        Self {
            src_drq: NdmaDrqType::Sdram,
            src_addr_type: AddrType::Linear,
            src_burst: BurstLen::Single,
            src_width: DataWidth::Bit8,
            dst_drq: NdmaDrqType::Sdram,
            dst_addr_type: AddrType::Linear,
            dst_burst: BurstLen::Single,
            dst_width: DataWidth::Bit8,
            wait_state: 0,
            continuous: false,
        }
    }
}

use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{compiler_fence, fence, AtomicBool, Ordering};
use core::task::{Context, Poll};

use embassy_sync::waitqueue::AtomicWaker;

/// Per-channel state for async wakeup
struct ChannelState {
    waker: AtomicWaker,
    complete: AtomicBool,
}

impl ChannelState {
    const NEW: Self = Self {
        waker: AtomicWaker::new(),
        complete: AtomicBool::new(false),
    };
}

static CHANNEL_STATE: [ChannelState; NDMA_COUNT] = [
    ChannelState::NEW,
    ChannelState::NEW,
    ChannelState::NEW,
    ChannelState::NEW,
];

/// Initialize the DMA controller. Called once from hal::init().
pub(crate) unsafe fn init() {
    let ccu = &*pac::Ccu::ptr();

    // Enable DMA clock gating
    ccu.bus_clk_gating0().modify(|_, w| w.dma_gating().set_bit());
    // De-assert DMA reset
    ccu.bus_soft_rst0().modify(|_, w| w.dma_rst().clear_bit());
    for _ in 0..100 {
        core::hint::spin_loop();
    }
    ccu.bus_soft_rst0().modify(|_, w| w.dma_rst().set_bit());

    let dma = &*pac::Dma::ptr();

    // Disable DMA auto clock gating — required for reliable DMA operation
    // on F1C100S. When auto-gating is enabled (default), the DMA controller's
    // internal clock can be gated at unexpected times, causing bus lockups.
    dma.dma_pty_cfg().modify(|_, w| w.dma_auto_gating().set_bit());

    // Disable all DMA interrupts
    dma.dma_int_ctrl().write(|w| w.bits(0));
    // Clear all pending
    dma.dma_int_sta().write(|w| w.bits(0x00FF_00FF));

    // Register DMA IRQ handler
    crate::intc::set_irq_handler(crate::interrupt::Interrupt::DMA.number(), dma_irq_handler);
    crate::intc::enable_irq(crate::interrupt::Interrupt::DMA.number());
}

/// DMA IRQ handler — dispatches half/full transfer events for all NDMA channels
fn dma_irq_handler() {
    let dma = unsafe { &*pac::Dma::ptr() };
    let status = dma.dma_int_sta().read().bits();

    for ch in 0..NDMA_COUNT {
        let full_bit = 1u32 << (ch * 2 + 1);
        let half_bit = 1u32 << (ch * 2);

        if status & (full_bit | half_bit) != 0 {
            dma.dma_int_sta().write(|w| unsafe { w.bits(full_bit | half_bit) });

            CHANNEL_STATE[ch].complete.store(true, Ordering::Release);
            CHANNEL_STATE[ch].waker.wake();
        }
    }
}

// ============================================================================
// Low-level NDMA channel register access
// ============================================================================

/// Get the base address of NDMA channel N registers.
/// NDMA0=0x100, NDMA1=0x120, NDMA2=0x140, NDMA3=0x160
/// Each channel: CFG(+0), SRC_ADR(+4), DES_ADR(+8), BYTE_CNT(+0xC)
const DMA_BASE: usize = 0x01C0_2000;

#[inline]
fn ndma_cfg_addr(ch: usize) -> *mut u32 {
    (DMA_BASE + 0x100 + ch * 0x20) as *mut u32
}
#[inline]
fn ndma_src_addr(ch: usize) -> *mut u32 {
    (DMA_BASE + 0x104 + ch * 0x20) as *mut u32
}
#[inline]
fn ndma_dst_addr(ch: usize) -> *mut u32 {
    (DMA_BASE + 0x108 + ch * 0x20) as *mut u32
}
#[inline]
fn ndma_byte_cnt_addr(ch: usize) -> *mut u32 {
    (DMA_BASE + 0x10C + ch * 0x20) as *mut u32
}

/// Check if an address is in SDRAM (cached) region.
#[inline]
fn is_cached_addr(addr: u32) -> bool {
    addr >= 0x8000_0000
}

/// Configure and start an NDMA transfer.
///
/// # Safety
/// Caller must ensure addresses and lengths are valid.
unsafe fn ndma_start(ch: usize, src: u32, dst: u32, byte_count: u32, config: &NdmaConfig) {
    assert!(ch < NDMA_COUNT);
    assert!(byte_count > 0 && byte_count <= 0x1_FFFF);

    // Cache maintenance: flush src data from D-cache to physical memory
    // so DMA controller can read the correct data.
    if config.src_addr_type == AddrType::Linear && is_cached_addr(src) {
        arm9::asm::clean_dcache_range(src, byte_count);
    }
    // Invalidate dst cache lines so CPU won't read stale data after DMA writes.
    if config.dst_addr_type == AddrType::Linear && is_cached_addr(dst) {
        arm9::asm::invalidate_dcache_range(dst, byte_count);
    }

    let state = &CHANNEL_STATE[ch];
    state.complete.store(false, Ordering::Release);

    core::ptr::write_volatile(ndma_src_addr(ch), src);
    core::ptr::write_volatile(ndma_dst_addr(ch), dst);
    core::ptr::write_volatile(ndma_byte_cnt_addr(ch), byte_count);

    let cfg: u32 = (config.src_drq as u32)
        | ((config.src_addr_type as u32) << 5)
        | ((config.src_burst as u32) << 7)
        | ((config.src_width as u32) << 8)
        | (1u32 << 15)
        | ((config.dst_drq as u32) << 16)
        | ((config.dst_addr_type as u32) << 21)
        | ((config.dst_burst as u32) << 23)
        | ((config.dst_width as u32) << 24)
        | (((config.wait_state & 0x7) as u32) << 26)
        | ((config.continuous as u32) << 29);

    core::ptr::write_volatile(ndma_cfg_addr(ch), cfg);

    let dma = &*pac::Dma::ptr();
    dma.dma_int_ctrl().modify(|r, w| {
        let full_en_bit = 1u32 << (ch * 2 + 1);
        w.bits(r.bits() | full_en_bit)
    });

    compiler_fence(Ordering::SeqCst);

    // Write cfg with LOADING bit in one shot (reading CFG while busy causes bus errors)
    core::ptr::write_volatile(ndma_cfg_addr(ch), cfg | (1u32 << 31));
}

/// Start an NDMA transfer for polling via DMA_INT_STA.
///
/// # Safety
/// Caller must ensure addresses and lengths are valid.
///
/// IMPORTANT: This function is `#[inline(always)]` to avoid function call
/// overhead between SPI register setup and DMA start. On F1C100S, any
/// extra bus activity (stack operations in SDRAM, UART polling) between
/// SPI setup and DMA CFG write can cause AHB bus lockups.
#[inline(always)]
pub unsafe fn ndma_start_poll(ch: usize, src: u32, dst: u32, byte_count: u32, config: &NdmaConfig) {
    // Cache maintenance: flush src data from D-cache to physical memory
    if config.src_addr_type == AddrType::Linear && is_cached_addr(src) {
        arm9::asm::clean_dcache_range(src, byte_count);
    }
    // NOTE: dst cache invalidation is done by the caller AFTER DMA completes,
    // to avoid invalidating cache lines that overlap with stack/local variables.

    // Exactly match the working raw test sequence:
    // 1. Stop channel
    core::ptr::write_volatile(ndma_cfg_addr(ch), 0);
    // 2. Clear ALL pending interrupt flags
    core::ptr::write_volatile((0x01C0_2004) as *mut u32, 0x00FF_00FF);
    // 3. Write src/dst/cnt
    core::ptr::write_volatile(ndma_src_addr(ch), src);
    core::ptr::write_volatile(ndma_dst_addr(ch), dst);
    core::ptr::write_volatile(ndma_byte_cnt_addr(ch), byte_count);
    // 4. Disable DMA interrupts (polling mode — don't let IRQ handler steal the pending bit)
    core::ptr::write_volatile((0x01C0_2000) as *mut u32, 0);

    let cfg: u32 = (config.src_drq as u32)
        | ((config.src_addr_type as u32) << 5)
        | ((config.src_burst as u32) << 7)
        | ((config.src_width as u32) << 8)
        | (1u32 << 15)
        | ((config.dst_drq as u32) << 16)
        | ((config.dst_addr_type as u32) << 21)
        | ((config.dst_burst as u32) << 23)
        | ((config.dst_width as u32) << 24)
        | (((config.wait_state & 0x7) as u32) << 26)
        | ((config.continuous as u32) << 29);

    compiler_fence(Ordering::SeqCst);
    // 5. Write CFG with LOADING bit to start DMA
    core::ptr::write_volatile(ndma_cfg_addr(ch), cfg | (1u32 << 31));
}

/// Poll-wait for NDMA channel to finish by checking DMA_INT_STA.
///
/// Note: Reading NDMA CFG register while DMA is busy causes bus errors
/// on F1C100S, so we poll the interrupt status register instead.
///
/// # Safety
/// Channel must have been started with ndma_start_poll.
pub unsafe fn ndma_poll_wait(ch: usize) {
    let dma = &*pac::Dma::ptr();
    let full_bit = 1u32 << (ch * 2 + 1);

    // Poll DMA_INT_STA for full-transfer pending bit
    let mut loops = 0u32;
    while dma.dma_int_sta().read().bits() & full_bit == 0 {
        loops += 1;
        if loops > 10_000_000 {
            crate::println!("[dma] TIMEOUT! int_sta={:#010X}", dma.dma_int_sta().read().bits());
            break;
        }
        core::hint::spin_loop();
    }

    // Clear the pending bit
    dma.dma_int_sta().write(|w| w.bits(full_bit));

    fence(Ordering::SeqCst);
}

/// Check if NDMA channel is busy (via DMA_INT_STA, not CFG register)
#[inline]
fn ndma_is_busy(ch: usize) -> bool {
    // Note: reading NDMA CFG register while busy causes bus errors on F1C100S.
    // Check if full-transfer pending bit is NOT set (meaning still busy).
    let dma = unsafe { &*pac::Dma::ptr() };
    let full_bit = 1u32 << (ch * 2 + 1);
    dma.dma_int_sta().read().bits() & full_bit == 0
}

/// Stop an NDMA channel
unsafe fn ndma_stop(ch: usize) {
    // Write 0 to CFG register to clear LOADING and stop the channel
    core::ptr::write_volatile(ndma_cfg_addr(ch), 0);

    // Disable interrupts for this channel
    let dma = &*pac::Dma::ptr();
    dma.dma_int_ctrl().modify(|r, w| {
        let mask = !(0x3u32 << (ch * 2));
        w.bits(r.bits() & mask)
    });

    // Clear pending
    dma.dma_int_sta().write(|w| w.bits(0x3u32 << (ch * 2)));
}

// ============================================================================
// Async DMA Transfer
// ============================================================================

/// An async NDMA transfer. Completes when the DMA finishes.
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Transfer {
    ch: usize,
}

impl Transfer {
    /// Start a new NDMA transfer.
    ///
    /// # Safety
    /// `src` and `dst` must be valid for `byte_count` bytes.
    pub unsafe fn new(ch: usize, src: u32, dst: u32, byte_count: u32, config: &NdmaConfig) -> Self {
        ndma_start(ch, src, dst, byte_count, config);
        Self { ch }
    }

    /// Check if the transfer is still running.
    pub fn is_running(&self) -> bool {
        !CHANNEL_STATE[self.ch].complete.load(Ordering::Acquire) && ndma_is_busy(self.ch)
    }

    /// Blocking wait until transfer completes.
    pub fn blocking_wait(self) {
        while self.is_running() {}
        fence(Ordering::SeqCst);
        core::mem::forget(self);
    }
}

impl Drop for Transfer {
    fn drop(&mut self) {
        unsafe { ndma_stop(self.ch) };
        while ndma_is_busy(self.ch) {}
        fence(Ordering::SeqCst);
    }
}

impl Unpin for Transfer {}

impl Future for Transfer {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let state = &CHANNEL_STATE[self.ch];
        state.waker.register(cx.waker());

        if state.complete.load(Ordering::Acquire) || !ndma_is_busy(self.ch) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
