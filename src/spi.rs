//! SPI driver for F1C100S/F1C200S
//!
//! The F1C100S has two SPI controllers (SPI0, SPI1) with:
//! - Full-duplex synchronous serial interface
//! - Master/Slave configurable
//! - 64-byte TX/RX FIFOs
//! - 4 chip selects for SPI0, 1 for SPI1
//! - SPI modes 0-3 (CPOL/CPHA)
//! - Clock: 3KHz ~ 100MHz, AHB_CLK >= 2x SPI_SCLK
//!
//! Pin mapping (from datasheet):
//! - SPI0: PC0=CLK, PC1=CS, PC2=MISO, PC3=MOSI (Func2)
//! - SPI1: PA0=CS, PA1=MOSI, PA2=CLK, PA3=MISO (Func5)

use core::marker::PhantomData;

use embedded_hal::spi::{Mode, Phase, Polarity, MODE_0};

use crate::gpio::{self, PinMode, Pull};
use crate::{pac, rcc, Peri};

const SPI_FIFO_DEPTH: usize = 64;

/// SPI Error
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    RxOverflow,
    TxUnderrun,
    Timeout,
}

#[derive(Copy, Clone)]
pub enum BitOrder {
    MsbFirst,
    LsbFirst,
}

#[derive(Copy, Clone, Debug)]
#[repr(u8)]
pub enum ChipSelect {
    Ss0 = 0,
    Ss1 = 1,
    Ss2 = 2,
    Ss3 = 3,
}

#[non_exhaustive]
#[derive(Copy, Clone)]
pub struct Config {
    pub mode: Mode,
    pub bit_order: BitOrder,
    pub frequency: crate::time::Hertz,
    pub cs: ChipSelect,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: MODE_0,
            bit_order: BitOrder::MsbFirst,
            frequency: crate::time::Hertz(1_000_000),
            cs: ChipSelect::Ss0,
        }
    }
}

/// SPI driver (blocking, master mode).
pub struct Spi<'d, T: Instance> {
    _peri: PhantomData<&'d mut T>,
}

impl<'d, T: Instance> Spi<'d, T> {
    /// Create SPI master with SCK, MOSI, MISO, CS pins.
    pub fn new(
        _peri: Peri<'d, T>,
        sck: Peri<'d, impl SckPin<T>>,
        mosi: Peri<'d, impl MosiPin<T>>,
        miso: Peri<'d, impl MisoPin<T>>,
        cs: Peri<'d, impl CsPin<T>>,
        config: Config,
    ) -> Self {
        // Enable clock and reset BEFORE configuring pins/registers
        T::enable_clock();
        T::assert_reset();
        T::deassert_reset();
        into_af_pin(&*sck);
        into_af_pin(&*mosi);
        into_af_pin(&*miso);
        into_af_pin(&*cs);
        let mut this = Self { _peri: PhantomData };
        this.configure(&config);
        this
    }

    /// Create SPI master without CS pin (use cs_low/cs_high for manual control).
    pub fn new_nocs(
        _peri: Peri<'d, T>,
        sck: Peri<'d, impl SckPin<T>>,
        mosi: Peri<'d, impl MosiPin<T>>,
        miso: Peri<'d, impl MisoPin<T>>,
        config: Config,
    ) -> Self {
        T::enable_clock();
        T::assert_reset();
        T::deassert_reset();
        into_af_pin(&*sck);
        into_af_pin(&*mosi);
        into_af_pin(&*miso);
        let mut this = Self { _peri: PhantomData };
        this.configure(&config);
        this
    }

    #[inline]
    fn regs() -> &'static pac::spi0::RegisterBlock {
        unsafe { &*T::regs() }
    }

    fn configure(&mut self, config: &Config) {
        let regs = Self::regs();

        // Soft reset
        regs.spi_gcr().modify(|_, w| w.srst().set_bit());
        while regs.spi_gcr().read().srst().bit_is_set() {}

        // Enable SPI, master mode, transmit pause enable
        regs.spi_gcr().write(|w| {
            w.en().set_bit();
            w.mode().set_bit();
            w.tp_en().set_bit()
        });

        // Transfer control: CPOL, CPHA, bit order, CS, etc.
        // Manual CS mode from the start. In manual mode (SS_OWNER=1),
        // SS_LEVEL directly controls pin output:
        //   ss_level=0 → pin LOW (CS asserted)
        //   ss_level=1 → pin HIGH (CS deasserted)
        regs.spi_tcr().write(|w| unsafe {
            match config.mode.phase {
                Phase::CaptureOnFirstTransition => w.cpha().clear_bit(),
                Phase::CaptureOnSecondTransition => w.cpha().set_bit(),
            };
            match config.mode.polarity {
                Polarity::IdleLow => w.cpol().clear_bit(),
                Polarity::IdleHigh => w.cpol().set_bit(),
            };
            w.spol().set_bit(); // CS active low
            w.ss_sel().bits(config.cs as u8);
            w.ss_owner().set_bit(); // Manual CS control
            w.ss_level().set_bit(); // Deasserted (pin HIGH)
            w.dhb().set_bit(); // discard hash burst
            match config.bit_order {
                BitOrder::MsbFirst => w.fbs().clear_bit(),
                BitOrder::LsbFirst => w.fbs().set_bit(),
            };
            w
        });

        // Reset FIFOs
        regs.spi_fcr().write(|w| unsafe {
            w.tf_rst().set_bit();
            w.rf_rst().set_bit();
            w.rx_trig_level().bits(1);
            w.tx_trig_level().bits(SPI_FIFO_DEPTH as u8 / 4)
        });
        // Wait for FIFO reset to complete
        while regs.spi_fcr().read().tf_rst().bit_is_set() || regs.spi_fcr().read().rf_rst().bit_is_set() {}

        // Disable all interrupts (write 0 to all bits)
        regs.spi_ier().write(|w| w);

        // Clear all pending interrupt flags
        regs.spi_isr().write(|w| {
            w.tc().set_bit();
            w.rx_rdy().set_bit();
            w.tx_ready().set_bit();
            w.rx_ovf().set_bit();
            w.rx_udf().set_bit();
            w.tf_ovf().set_bit();
            w.tf_udf().set_bit();
            w.ssi().set_bit();
            w.rx_emp().set_bit();
            w.rx_full().set_bit();
            w.tx_emp().set_bit();
            w.tx_full().set_bit()
        });

        self.set_clock(config.frequency);
    }

    fn set_clock(&self, freq: crate::time::Hertz) {
        let regs = Self::regs();
        let ahb_clk = rcc::clocks().hclk.0;
        let target = freq.0;
        if target == 0 {
            return;
        }

        // CDR2: SPICLK = AHB_CLK / (2*(CDR2+1)), CDR2 range 0..=255
        let cdr2_val = if target >= ahb_clk / 2 {
            0u32
        } else {
            (ahb_clk / (2 * target)).saturating_sub(1)
        };

        if cdr2_val <= 255 {
            regs.spi_ccr().write(|w| unsafe {
                w.drs().set_bit();
                w.cdr2().bits(cdr2_val as u8)
            });
        } else {
            // CDR1: SPICLK = AHB_CLK / 2^(CDR1+1)
            let mut cdr1 = 0u8;
            while cdr1 < 15 {
                if ahb_clk / (1u32 << (cdr1 as u32 + 1)) <= target {
                    break;
                }
                cdr1 += 1;
            }
            regs.spi_ccr().write(|w| unsafe {
                w.drs().clear_bit();
                w.cdr1().bits(cdr1)
            });
        }
    }

    fn set_dhb(&self, discard: bool) {
        Self::regs().spi_tcr().modify(|_, w| {
            if discard {
                w.dhb().set_bit()
            } else {
                w.dhb().clear_bit()
            }
        });
    }

    /// Write one byte to TX FIFO using 8-bit access.
    /// The F1C100S SPI TX FIFO must be accessed in byte units;
    /// a 32-bit write would push 4 bytes into the FIFO.
    #[inline]
    fn write_txd_byte(byte: u8) {
        let txd_addr = T::regs() as usize + 0x200;
        unsafe {
            core::ptr::write_volatile(txd_addr as *mut u8, byte);
        }
    }

    /// Read one byte from RX FIFO using 8-bit access.
    #[inline]
    fn read_rxd_byte() -> u8 {
        let rxd_addr = T::regs() as usize + 0x300;
        unsafe { core::ptr::read_volatile(rxd_addr as *const u8) }
    }

    fn reset_fifos(&self) {
        let regs = Self::regs();
        regs.spi_fcr().modify(|_, w| {
            w.tf_rst().set_bit();
            w.rf_rst().set_bit()
        });
        while regs.spi_fcr().read().tf_rst().bit_is_set() || regs.spi_fcr().read().rf_rst().bit_is_set() {}
    }

    fn wait_transfer_complete(&self) -> Result<(), Error> {
        let regs = Self::regs();
        let mut timeout = 0xFFFF_FFFFu32;
        while timeout > 0 {
            if regs.spi_isr().read().tc().bit_is_set() {
                regs.spi_isr().write(|w| w.tc().set_bit());
                return Ok(());
            }
            timeout -= 1;
        }
        Err(Error::Timeout)
    }

    /// Full-duplex SPI transfer: send `tx_buf`, then receive `rx_buf.len()` bytes.
    ///
    /// Total burst = tx_buf.len() + rx_buf.len(). During the TX phase, received
    /// bytes are discarded. During the RX phase, dummy bytes (0x00) are sent.
    pub fn transfer(&mut self, tx_buf: &[u8], rx_buf: &mut [u8]) -> Result<(), Error> {
        let regs = Self::regs();
        let total_len = tx_buf.len() + rx_buf.len();
        if total_len == 0 {
            return Ok(());
        }

        self.reset_fifos();
        self.set_dhb(false);

        // Burst counters
        regs.spi_mbc().write(|w| unsafe { w.mbc().bits(total_len as u32) });
        regs.spi_mtc().write(|w| unsafe { w.mwtc().bits(tx_buf.len() as u32) });
        regs.spi_bcc().write(|w| unsafe { w.stc().bits(tx_buf.len() as u32) });

        // Fill TX FIFO (byte access)
        let mut tx_idx = 0usize;
        let initial = tx_buf.len().min(SPI_FIFO_DEPTH);
        while tx_idx < initial {
            Self::write_txd_byte(tx_buf[tx_idx]);
            tx_idx += 1;
        }

        // Clear TC flag before starting
        regs.spi_isr().write(|w| w.tc().set_bit());

        // Start exchange
        regs.spi_tcr().modify(|_, w| w.xch().set_bit());

        // Continue filling TX FIFO
        while tx_idx < tx_buf.len() {
            if (regs.spi_fsr().read().tf_cnt().bits() as usize) < SPI_FIFO_DEPTH {
                Self::write_txd_byte(tx_buf[tx_idx]);
                tx_idx += 1;
            }
        }

        // Read RX: skip tx_buf.len() echo bytes, then collect rx_buf
        let mut rx_skip = tx_buf.len();
        let mut rx_idx = 0usize;
        let mut rx_done = 0usize;
        while rx_done < total_len {
            let cnt = regs.spi_fsr().read().rf_cnt().bits() as usize;
            for _ in 0..cnt {
                let byte = Self::read_rxd_byte();
                rx_done += 1;
                if rx_skip > 0 {
                    rx_skip -= 1;
                } else if rx_idx < rx_buf.len() {
                    rx_buf[rx_idx] = byte;
                    rx_idx += 1;
                }
            }
        }

        self.wait_transfer_complete()
    }

    /// Write-only: send bytes, discard received data.
    pub fn blocking_write(&mut self, data: &[u8]) -> Result<(), Error> {
        let regs = Self::regs();
        if data.is_empty() {
            return Ok(());
        }

        self.reset_fifos();
        self.set_dhb(true);

        regs.spi_mbc().write(|w| unsafe { w.mbc().bits(data.len() as u32) });
        regs.spi_mtc().write(|w| unsafe { w.mwtc().bits(data.len() as u32) });
        regs.spi_bcc().write(|w| unsafe { w.stc().bits(data.len() as u32) });

        let mut idx = 0usize;
        let initial = data.len().min(SPI_FIFO_DEPTH);
        while idx < initial {
            Self::write_txd_byte(data[idx]);
            idx += 1;
        }

        // Clear TC flag before starting
        regs.spi_isr().write(|w| w.tc().set_bit());

        regs.spi_tcr().modify(|_, w| w.xch().set_bit());

        while idx < data.len() {
            if (regs.spi_fsr().read().tf_cnt().bits() as usize) < SPI_FIFO_DEPTH {
                Self::write_txd_byte(data[idx]);
                idx += 1;
            }
        }

        self.wait_transfer_complete()
    }

    /// Read-only: send dummy 0x00, collect received bytes.
    pub fn blocking_read(&mut self, data: &mut [u8]) -> Result<(), Error> {
        let regs = Self::regs();
        if data.is_empty() {
            return Ok(());
        }

        self.reset_fifos();
        self.set_dhb(false);

        regs.spi_mbc().write(|w| unsafe { w.mbc().bits(data.len() as u32) });
        regs.spi_mtc().write(|w| unsafe { w.mwtc().bits(0) });
        regs.spi_bcc().write(|w| unsafe { w.stc().bits(0) });

        // Clear TC flag before starting
        regs.spi_isr().write(|w| w.tc().set_bit());

        regs.spi_tcr().modify(|_, w| w.xch().set_bit());

        let mut idx = 0usize;
        while idx < data.len() {
            let cnt = regs.spi_fsr().read().rf_cnt().bits() as usize;
            for _ in 0..cnt {
                if idx < data.len() {
                    data[idx] = Self::read_rxd_byte();
                    idx += 1;
                }
            }
        }

        self.wait_transfer_complete()
    }

    /// Assert CS (drive low) for manual chip-select control.
    /// In manual mode (SS_OWNER=1), SS_LEVEL directly controls the pin output.
    /// ss_level=0 → pin LOW (asserted for active-low CS)
    pub fn cs_low(&self) {
        Self::regs().spi_tcr().modify(|_, w| w.ss_level().clear_bit());
    }

    /// De-assert CS (drive high).
    /// ss_level=1 → pin HIGH (deasserted)
    pub fn cs_high(&self) {
        Self::regs().spi_tcr().modify(|_, w| w.ss_level().set_bit());
    }

    /// Dump all SPI register values (println version, no defmt needed).
    pub fn dump_regs_println(&self) {
        let regs = Self::regs();
        crate::println!("SPI_GCR:  0x{:08X}", regs.spi_gcr().read().bits());
        crate::println!("SPI_TCR:  0x{:08X}", regs.spi_tcr().read().bits());
        crate::println!("SPI_CCR:  0x{:08X}", regs.spi_ccr().read().bits());
        crate::println!("SPI_FCR:  0x{:08X}", regs.spi_fcr().read().bits());
        crate::println!("SPI_FSR:  0x{:08X}", regs.spi_fsr().read().bits());
        crate::println!("SPI_IER:  0x{:08X}", regs.spi_ier().read().bits());
        crate::println!("SPI_ISR:  0x{:08X}", regs.spi_isr().read().bits());
    }

    // ========================================================================
    // DMA-based async transfers
    // ========================================================================

    /// Enable SPI RX DMA request (NDMA mode).
    fn enable_rx_dma(&self) {
        let regs = Self::regs();
        regs.spi_fcr().modify(|_, w| {
            w.rf_drq_en().set_bit();
            w.rf_dma_mode().clear_bit() // 0 = NDMA mode
        });
    }

    /// Enable SPI TX DMA request (NDMA mode).
    fn enable_tx_dma(&self) {
        let regs = Self::regs();
        regs.spi_fcr().modify(|_, w| w.tf_drq_en().set_bit());
    }

    /// Disable SPI DMA requests.
    fn disable_dma(&self) {
        let regs = Self::regs();
        regs.spi_fcr().modify(|_, w| {
            w.rf_drq_en().clear_bit();
            w.tf_drq_en().clear_bit()
        });
    }

    /// Async DMA write: send `data` via DMA, discard received bytes.
    ///
    /// Uses NDMA channel `tx_ch` (0..3). Caller must ensure the channel is free.
    pub async fn dma_write(&mut self, tx_ch: usize, data: &[u8]) -> Result<(), Error> {
        use crate::dma::{self, AddrType, BurstLen, DataWidth, NdmaConfig, NdmaDrqType};

        let regs = Self::regs();
        if data.is_empty() {
            return Ok(());
        }

        self.reset_fifos();
        self.set_dhb(true); // discard RX

        // Set burst counters
        regs.spi_mbc().write(|w| unsafe { w.mbc().bits(data.len() as u32) });
        regs.spi_mtc().write(|w| unsafe { w.mwtc().bits(data.len() as u32) });
        regs.spi_bcc().write(|w| unsafe { w.stc().bits(data.len() as u32) });

        // Clear TC flag
        regs.spi_isr().write(|w| w.tc().set_bit());

        // Enable TX DMA
        self.enable_tx_dma();

        // For SDRAM sources, use 32-bit width + burst4 (required for reliable
        // NDMA SDRAM access on F1C100S).
        let src_addr = data.as_ptr() as u32;
        let src_drq = NdmaDrqType::for_addr(src_addr);
        let (src_width, src_burst) = if src_drq == NdmaDrqType::Sdram && src_addr % 4 == 0 && data.len() % 4 == 0 {
            (DataWidth::Bit32, BurstLen::Burst4)
        } else {
            (DataWidth::Bit8, BurstLen::Single)
        };

        let config = NdmaConfig {
            src_drq,
            src_addr_type: AddrType::Linear,
            src_burst,
            src_width,
            dst_drq: T::ndma_drq(),
            dst_addr_type: AddrType::Io,
            dst_burst: BurstLen::Single,
            dst_width: DataWidth::Bit8,
            wait_state: 2,
            continuous: false,
        };

        let txd_addr = T::regs() as u32 + 0x200;

        // Start exchange BEFORE DMA so the SPI controller is ready
        regs.spi_tcr().modify(|_, w| w.xch().set_bit());

        let transfer = unsafe { dma::Transfer::new(tx_ch, data.as_ptr() as u32, txd_addr, data.len() as u32, &config) };

        transfer.await;

        // Wait for SPI transfer complete
        self.wait_transfer_complete()?;
        self.disable_dma();
        Ok(())
    }

    /// Async DMA read: receive `data.len()` bytes via DMA, sending dummy 0x00.
    ///
    /// Uses NDMA channel `rx_ch` (0..3). Caller must ensure the channel is free.
    pub async fn dma_read(&mut self, rx_ch: usize, data: &mut [u8]) -> Result<(), Error> {
        use crate::dma::{self, AddrType, BurstLen, DataWidth, NdmaConfig, NdmaDrqType};

        let regs = Self::regs();
        if data.is_empty() {
            return Ok(());
        }

        self.reset_fifos();
        self.set_dhb(false);

        // Set burst counters: MBC=total, MTC=0 (no TX), BCC=0
        regs.spi_mbc().write(|w| unsafe { w.mbc().bits(data.len() as u32) });
        regs.spi_mtc().write(|w| unsafe { w.mwtc().bits(0) });
        regs.spi_bcc().write(|w| unsafe { w.stc().bits(0) });

        // Clear TC flag
        regs.spi_isr().write(|w| w.tc().set_bit());

        // Enable RX DMA
        self.enable_rx_dma();

        // For SDRAM destinations, use 32-bit width + burst4 (required for reliable
        // NDMA SDRAM access on F1C100S). The DMA controller packs bytes internally.
        let dst_addr = data.as_mut_ptr() as u32;
        let dst_drq = NdmaDrqType::for_addr(dst_addr);
        let (dst_width, dst_burst) = if dst_drq == NdmaDrqType::Sdram && dst_addr % 4 == 0 && data.len() % 4 == 0 {
            (DataWidth::Bit32, BurstLen::Burst4)
        } else {
            (DataWidth::Bit8, BurstLen::Single)
        };

        let config = NdmaConfig {
            src_drq: T::ndma_drq(),
            src_addr_type: AddrType::Io,
            src_burst: BurstLen::Single,
            src_width: DataWidth::Bit8,
            dst_drq,
            dst_addr_type: AddrType::Linear,
            dst_burst,
            dst_width,
            wait_state: 2,
            continuous: false,
        };

        let rxd_addr = T::regs() as u32 + 0x300;

        let transfer = unsafe { dma::Transfer::new(rx_ch, rxd_addr, dst_addr, data.len() as u32, &config) };

        // Start exchange
        regs.spi_tcr().modify(|_, w| w.xch().set_bit());

        transfer.await;

        // Wait for SPI transfer complete
        self.wait_transfer_complete()?;
        self.disable_dma();
        Ok(())
    }

    /// DMA transfer: send `tx_buf` blocking, then receive `rx_buf` via polling DMA.
    ///
    /// TX phase is blocking (command bytes are small).
    /// RX phase uses NDMA with polling (no interrupt/async dependency).
    pub fn dma_transfer_blocking(&mut self, rx_ch: usize, tx_buf: &[u8], rx_buf: &mut [u8]) -> Result<(), Error> {
        let regs = Self::regs();

        if rx_buf.is_empty() {
            return self.blocking_write(tx_buf);
        }

        // Disable DMA IRQ in INTC — we're polling, don't want the IRQ handler
        // interfering with DMA_INT_STA reads or causing unexpected bus activity.
        crate::intc::disable_irq(crate::interrupt::Interrupt::DMA.number());

        // === Phase 1: blocking TX (command bytes) ===
        self.reset_fifos();
        self.set_dhb(true);

        regs.spi_mbc().write(|w| unsafe { w.mbc().bits(tx_buf.len() as u32) });
        regs.spi_mtc().write(|w| unsafe { w.mwtc().bits(tx_buf.len() as u32) });
        regs.spi_bcc().write(|w| unsafe { w.stc().bits(tx_buf.len() as u32) });

        for &b in tx_buf {
            Self::write_txd_byte(b);
        }

        regs.spi_isr().write(|w| w.tc().set_bit());
        regs.spi_tcr().modify(|_, w| w.xch().set_bit());
        self.wait_transfer_complete()?;
        while regs.spi_tcr().read().xch().bit_is_set() {}

        // === Phase 2: DMA RX ===
        self.reset_fifos();
        self.set_dhb(false);

        regs.spi_mbc().write(|w| unsafe { w.mbc().bits(rx_buf.len() as u32) });
        regs.spi_mtc().write(|w| unsafe { w.mwtc().bits(0) });
        regs.spi_bcc().write(|w| unsafe { w.stc().bits(0) });

        regs.spi_isr().write(|w| w.tc().set_bit());

        // NOTE: rf_drq_en is enabled later, right before DMA+XCH start,
        // to prevent the SPI from asserting DRQ while DMA isn't ready.

        let dst_addr = rx_buf.as_mut_ptr() as u32;
        let rxd_addr = T::regs() as u32 + 0x300;
        let len = rx_buf.len() as u32;

        // === Critical section: no UART, no function calls, just raw register writes ===
        unsafe {
            let ch = rx_ch;
            let dma_base: usize = 0x01C0_2000;
            let cfg_reg = (dma_base + 0x100 + ch * 0x20) as *mut u32;
            let src_reg = (dma_base + 0x104 + ch * 0x20) as *mut u32;
            let dst_reg = (dma_base + 0x108 + ch * 0x20) as *mut u32;
            let cnt_reg = (dma_base + 0x10C + ch * 0x20) as *mut u32;
            let int_sta = (dma_base + 0x04) as *mut u32;
            let int_ctrl = (dma_base + 0x00) as *mut u32;

            // 1. Stop channel
            core::ptr::write_volatile(cfg_reg, 0);
            // 2. Clear all pending
            core::ptr::write_volatile(int_sta, 0x00FF_00FF);
            // 3. Write src/dst/cnt
            core::ptr::write_volatile(src_reg, rxd_addr);
            core::ptr::write_volatile(dst_reg, dst_addr);
            core::ptr::write_volatile(cnt_reg, len);
            // 4. Disable DMA interrupts
            core::ptr::write_volatile(int_ctrl, 0);

            // CFG value — SPI RX is 8-bit, so both src and dst use 8-bit width.
            // Using 32-bit dst width causes byte replication/misalignment because
            // the DMA packs 8-bit reads into 32-bit writes incorrectly.
            //
            // Layout:
            //   [4:0]   src DRQ = 0x04 (SPI0)
            //   [6:5]   src addr type = 01 (IO)
            //   [7]     src burst = 0 (single)
            //   [9:8]   src width = 00 (8-bit)
            //   [15]    remain byte cnt read en = 1
            //   [20:16] dst DRQ
            //   [22:21] dst addr type = 00 (linear)
            //   [23]    dst burst = 0 (single)
            //   [25:24] dst width = 00 (8-bit)
            //   [28:26] wait state = 2
            let dst_drq: u32 = if dst_addr >= 0x8000_0000 { 0x11 } else { 0x10 };
            let cfg: u32 = 0x04           // src DRQ = SPI0
                | (1 << 5)               // src addr type = IO
                                         // src burst = single (bit7=0)
                                         // src width = 8-bit (bits[9:8]=00)
                | (1 << 15)              // remain byte cnt read en
                | (dst_drq << 16)        // dst DRQ
                                         // dst addr type = linear (bits[22:21]=00)
                                         // dst burst = single (bit23=0)
                                         // dst width = 8-bit (bits[25:24]=00)
                | (2 << 26); // wait state = 2

            // 5. Enable RX DMA request
            regs.spi_fcr().modify(|_, w| {
                w.rf_drq_en().set_bit();
                w.rf_dma_mode().clear_bit()
            });

            // 6. Start SPI exchange
            regs.spi_tcr().modify(|_, w| w.xch().set_bit());

            // 7. Write CFG with LOADING bit — DMA starts
            core::ptr::write_volatile(cfg_reg, cfg | (1u32 << 31));
        }

        // Poll DMA completion via raw register read
        unsafe {
            let int_sta_reg = 0x01C0_2004 as *const u32;
            let full_bit = 1u32 << (rx_ch * 2 + 1);
            let mut loops = 0u32;
            loop {
                let sta = core::ptr::read_volatile(int_sta_reg);
                if sta & full_bit != 0 {
                    // Clear the pending bit
                    core::ptr::write_volatile(int_sta_reg as *mut u32, full_bit);
                    break;
                }
                loops += 1;
                if loops > 10_000_000 {
                    break;
                }
                core::hint::spin_loop();
            }
        }

        // Wait for SPI exchange to finish by polling XCH bit (clears when done).
        // Do NOT use wait_transfer_complete() here — after DMA, the SPI TC flag
        // may not behave as expected, and polling SPI_ISR can hang the bus.
        {
            let tcr_addr = (T::regs() as usize + 0x08) as *const u32; // SPI_TCR offset
            let mut loops = 0u32;
            unsafe {
                while core::ptr::read_volatile(tcr_addr) & (1 << 31) != 0 {
                    loops += 1;
                    if loops > 1_000_000 {
                        break;
                    }
                    core::hint::spin_loop();
                }
            }
        }

        // Invalidate destination cache after DMA + SPI are both done (SDRAM only)
        if dst_addr >= 0x8000_0000 {
            arm9::asm::invalidate_dcache_range(dst_addr, len);
        }

        // Disable SPI DMA requests
        self.disable_dma();

        // Re-enable DMA IRQ
        crate::intc::enable_irq(crate::interrupt::Interrupt::DMA.number());

        Ok(())
    }
}

impl<'d, T: Instance> Drop for Spi<'d, T> {
    fn drop(&mut self) {
        Self::regs().spi_gcr().modify(|_, w| w.en().clear_bit());
        T::disable_clock();
    }
}

// ============================================================================
// Instance trait
// ============================================================================

trait SealedInstance {
    fn regs() -> *const pac::spi0::RegisterBlock;
    fn enable_clock();
    fn disable_clock();
    fn assert_reset();
    fn deassert_reset();
    fn ndma_drq() -> crate::dma::NdmaDrqType;
}

/// SPI peripheral instance
#[allow(private_bounds)]
pub trait Instance: SealedInstance + embassy_hal_internal::PeripheralType + 'static {}

impl SealedInstance for crate::peripherals::SPI0 {
    fn regs() -> *const pac::spi0::RegisterBlock {
        pac::Spi0::ptr()
    }
    fn enable_clock() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_clk_gating0().modify(|_, w| w.spi0_gating().set_bit());
    }
    fn disable_clock() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_clk_gating0().modify(|_, w| w.spi0_gating().clear_bit());
    }
    fn assert_reset() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_soft_rst0().modify(|_, w| w.spi0_rst().clear_bit());
    }
    fn deassert_reset() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_soft_rst0().modify(|_, w| w.spi0_rst().set_bit());
    }
    fn ndma_drq() -> crate::dma::NdmaDrqType {
        crate::dma::NdmaDrqType::Spi0
    }
}
impl Instance for crate::peripherals::SPI0 {}

impl SealedInstance for crate::peripherals::SPI1 {
    fn regs() -> *const pac::spi0::RegisterBlock {
        pac::Spi1::ptr()
    }
    fn enable_clock() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_clk_gating0().modify(|_, w| w.spi1_gating().set_bit());
    }
    fn disable_clock() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_clk_gating0().modify(|_, w| w.spi1_gating().clear_bit());
    }
    fn assert_reset() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_soft_rst0().modify(|_, w| w.spi1_rst().clear_bit());
    }
    fn deassert_reset() {
        let ccu = unsafe { &*pac::Ccu::ptr() };
        ccu.bus_soft_rst0().modify(|_, w| w.spi1_rst().set_bit());
    }
    fn ndma_drq() -> crate::dma::NdmaDrqType {
        crate::dma::NdmaDrqType::Spi1
    }
}
impl Instance for crate::peripherals::SPI1 {}

// ============================================================================
// Pin traits
// ============================================================================

fn into_af_pin<T: gpio::Pin>(pin: &T) {
    let af = pin_af_for_spi(pin.port(), pin.pin());
    pin.set_mode(af);
    pin.set_pull(Pull::None);
}

fn pin_af_for_spi(port: u8, pin: u8) -> PinMode {
    match (port, pin) {
        (2, 0..=3) => PinMode::Func2,  // SPI0 on Port C
        (0, 0..=3) => PinMode::Func5,  // SPI1 on Port A
        (4, 7..=10) => PinMode::Func4, // SPI1 on Port E (alt)
        _ => PinMode::Disabled,
    }
}

mod sealed {
    pub trait SckPin<T> {}
    pub trait MosiPin<T> {}
    pub trait MisoPin<T> {}
    pub trait CsPin<T> {}
}

#[allow(private_bounds)]
pub trait SckPin<T: Instance>: sealed::SckPin<T> + gpio::Pin {}
#[allow(private_bounds)]
pub trait MosiPin<T: Instance>: sealed::MosiPin<T> + gpio::Pin {}
#[allow(private_bounds)]
pub trait MisoPin<T: Instance>: sealed::MisoPin<T> + gpio::Pin {}
#[allow(private_bounds)]
pub trait CsPin<T: Instance>: sealed::CsPin<T> + gpio::Pin {}

// SPI0: PC0=CLK, PC1=CS, PC2=MISO, PC3=MOSI
impl sealed::SckPin<crate::peripherals::SPI0> for crate::peripherals::PC0 {}
impl SckPin<crate::peripherals::SPI0> for crate::peripherals::PC0 {}
impl sealed::CsPin<crate::peripherals::SPI0> for crate::peripherals::PC1 {}
impl CsPin<crate::peripherals::SPI0> for crate::peripherals::PC1 {}
impl sealed::MisoPin<crate::peripherals::SPI0> for crate::peripherals::PC2 {}
impl MisoPin<crate::peripherals::SPI0> for crate::peripherals::PC2 {}
impl sealed::MosiPin<crate::peripherals::SPI0> for crate::peripherals::PC3 {}
impl MosiPin<crate::peripherals::SPI0> for crate::peripherals::PC3 {}

// SPI1: PA2=CLK, PA0=CS, PA3=MISO, PA1=MOSI
impl sealed::SckPin<crate::peripherals::SPI1> for crate::peripherals::PA2 {}
impl SckPin<crate::peripherals::SPI1> for crate::peripherals::PA2 {}
impl sealed::CsPin<crate::peripherals::SPI1> for crate::peripherals::PA0 {}
impl CsPin<crate::peripherals::SPI1> for crate::peripherals::PA0 {}
impl sealed::MisoPin<crate::peripherals::SPI1> for crate::peripherals::PA3 {}
impl MisoPin<crate::peripherals::SPI1> for crate::peripherals::PA3 {}
impl sealed::MosiPin<crate::peripherals::SPI1> for crate::peripherals::PA1 {}
impl MosiPin<crate::peripherals::SPI1> for crate::peripherals::PA1 {}
