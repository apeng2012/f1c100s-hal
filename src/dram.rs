//! SDRAM (DRAM) controller driver for F1C100S/F1C200S.
//!
//! Ported from the C reference implementation (sys-dram.c by Jianjun Jiang).
//!
//! - F1C100S: 32MB DDR1 (col=10, row=13)
//! - F1C200S: 64MB DDR1 (col=10, row=13, detected as 64MB)
//!
//! The DRAM controller registers at 0x01c01000 are not in the PAC,
//! so we use raw pointer access for those. CCU and PIO registers use the PAC API.

use crate::pac::{Ccu, Pio};

// ============================================================================
// DRAM controller base and register offsets (not in PAC)
// ============================================================================
const DRAM_BASE: u32 = 0x01c0_1000;

const DRAM_SCONR: u32 = 0x00;
const DRAM_STMG0R: u32 = 0x04;
const DRAM_STMG1R: u32 = 0x08;
const DRAM_SCTLR: u32 = 0x0c;
const DRAM_SREFR: u32 = 0x10;
const DRAM_DDLYR: u32 = 0x24;
const DRAM_DRPTR0: u32 = 0x30;
const DRAM_DRPTR1: u32 = 0x34;
const DRAM_DRPTR2: u32 = 0x38;
const DRAM_DRPTR3: u32 = 0x3c;

// ============================================================================
// Timing parameters (for 156MHz DDR clock)
// ============================================================================
const SDR_T_CAS: u32 = 0x2;
const SDR_T_RAS: u32 = 0x8;
const SDR_T_RCD: u32 = 0x3;
const SDR_T_RP: u32 = 0x3;
const SDR_T_WR: u32 = 0x3;
const SDR_T_RFC: u32 = 0xd;
const SDR_T_XSR: u32 = 0xf9;
const SDR_T_RC: u32 = 0xb;
const SDR_T_INIT: u32 = 0x8;
const SDR_T_INIT_REF: u32 = 0x7;
const SDR_T_WTR: u32 = 0x2;
const SDR_T_RRD: u32 = 0x2;
const SDR_T_XP: u32 = 0x0;

const SDRAM_BASE: u32 = 0x8000_0000;

// ============================================================================
// Public configuration
// ============================================================================

/// Chip variant for DRAM sizing
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chip {
    /// F1C100S — 32MB DDR1
    F1C100S,
    /// F1C200S — 64MB DDR1
    F1C200S,
}

/// DRAM configuration
#[derive(Clone, Copy, Debug)]
pub struct DramConfig {
    /// Chip variant
    pub chip: Chip,
    /// PLL DDR clock in Hz (default 156MHz)
    pub pll_ddr_hz: u32,
}

impl Default for DramConfig {
    fn default() -> Self {
        Self {
            #[cfg(feature = "f1c200s")]
            chip: Chip::F1C200S,
            #[cfg(all(feature = "f1c100s", not(feature = "f1c200s")))]
            chip: Chip::F1C100S,
            #[cfg(not(any(feature = "f1c100s", feature = "f1c200s")))]
            chip: Chip::F1C200S,
            pll_ddr_hz: 156_000_000,
        }
    }
}

/// DRAM initialization result
#[derive(Clone, Copy, Debug)]
pub struct DramInfo {
    /// DRAM base address (0x80000000)
    pub base: u32,
    /// Detected DRAM size in MB
    pub size_mb: u32,
}

// ============================================================================
// Internal types
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
enum DramType {
    Sdr = 0,
    Ddr = 1,
}

#[derive(Clone, Copy, Debug)]
struct DramPara {
    base: u32,
    size: u32,
    clk: u32,
    access_mode: u32,
    cs_num: u32,
    ddr8_remap: u32,
    sdr_ddr: DramType,
    bwidth: u32,
    col_width: u32,
    row_width: u32,
    bank_size: u32,
    cas: u32,
}

impl DramPara {
    fn from_config(cfg: &DramConfig) -> Self {
        let (size, col_width, row_width) = match cfg.chip {
            Chip::F1C100S => (32u32, 10u32, 13u32),
            Chip::F1C200S => (64u32, 10u32, 13u32),
        };
        Self {
            base: SDRAM_BASE,
            size,
            clk: cfg.pll_ddr_hz / 1_000_000,
            access_mode: 1,
            cs_num: 1,
            ddr8_remap: 0,
            sdr_ddr: DramType::Ddr,
            bwidth: 16,
            col_width,
            row_width,
            bank_size: 4,
            cas: 0x3,
        }
    }
}

// ============================================================================
// Raw register helpers
// ============================================================================
#[inline(always)]
unsafe fn read32(addr: u32) -> u32 {
    core::ptr::read_volatile(addr as *const u32)
}

#[inline(always)]
unsafe fn write32(addr: u32, val: u32) {
    core::ptr::write_volatile(addr as *mut u32, val);
}

#[inline(always)]
fn sdelay(loops: u32) {
    for _ in 0..loops {
        core::hint::spin_loop();
    }
}

fn dram_delay(ms: u32) {
    sdelay(ms * 2 * 1000);
}

// ============================================================================
// DRAM controller operations
// ============================================================================
unsafe fn dram_initial() -> bool {
    let mut time: u32 = 0xffffff;
    let val = read32(DRAM_BASE + DRAM_SCTLR) | 0x1;
    write32(DRAM_BASE + DRAM_SCTLR, val);
    while (read32(DRAM_BASE + DRAM_SCTLR) & 0x1) != 0 {
        time -= 1;
        if time == 0 {
            return false;
        }
    }
    true
}

unsafe fn dram_delay_scan() -> bool {
    let mut time: u32 = 0xffffff;
    let val = read32(DRAM_BASE + DRAM_DDLYR) | 0x1;
    write32(DRAM_BASE + DRAM_DDLYR, val);
    while (read32(DRAM_BASE + DRAM_DDLYR) & 0x1) != 0 {
        time -= 1;
        if time == 0 {
            return false;
        }
    }
    true
}

unsafe fn dram_set_autofresh_cycle(clk: u32) {
    let row = (read32(DRAM_BASE + DRAM_SCONR) & 0x1e0) >> 5;
    let mut val: u32 = 0;

    if row == 0xc {
        if clk >= 1_000_000 {
            let mut temp = clk + (clk >> 3) + (clk >> 4) + (clk >> 5);
            let threshold = 10_000_000 >> 6;
            while temp >= threshold {
                temp -= threshold;
                val += 1;
            }
        } else {
            val = (clk * 499) >> 6;
        }
    } else if row == 0xb {
        if clk >= 1_000_000 {
            let mut temp = clk + (clk >> 3) + (clk >> 4) + (clk >> 5);
            let threshold = 10_000_000 >> 7;
            while temp >= threshold {
                temp -= threshold;
                val += 1;
            }
        } else {
            val = (clk * 499) >> 5;
        }
    }
    write32(DRAM_BASE + DRAM_SREFR, val);
}

unsafe fn dram_para_setup(para: &DramPara) -> bool {
    let bw_shift = if para.sdr_ddr != DramType::Sdr {
        para.bwidth >> 4
    } else {
        para.bwidth >> 5
    };
    let val = para.ddr8_remap
        | (0x1 << 1)
        | ((para.bank_size >> 2) << 3)
        | ((para.cs_num >> 1) << 4)
        | ((para.row_width - 1) << 5)
        | ((para.col_width - 1) << 9)
        | (bw_shift << 13)
        | (para.access_mode << 15)
        | ((para.sdr_ddr as u32) << 16);

    write32(DRAM_BASE + DRAM_SCONR, val);
    let sctlr = read32(DRAM_BASE + DRAM_SCTLR) | (0x1 << 19);
    write32(DRAM_BASE + DRAM_SCTLR, sctlr);
    dram_initial()
}

unsafe fn dram_check_delay(bwidth: u32) -> u32 {
    let dsize = if bwidth == 16 { 4 } else { 2 };
    let mut num: u32 = 0;
    for i in 0..dsize {
        let dflag = match i {
            0 => read32(DRAM_BASE + DRAM_DRPTR0),
            1 => read32(DRAM_BASE + DRAM_DRPTR1),
            2 => read32(DRAM_BASE + DRAM_DRPTR2),
            3 => read32(DRAM_BASE + DRAM_DRPTR3),
            _ => 0,
        };
        num += dflag.count_ones();
    }
    num
}

unsafe fn sdr_readpipe_scan() -> bool {
    for k in 0u32..32 {
        write32(SDRAM_BASE + 4 * k, k);
    }
    for k in 0u32..32 {
        if read32(SDRAM_BASE + 4 * k) != k {
            return false;
        }
    }
    true
}

unsafe fn sdr_readpipe_select() -> u32 {
    let mut value = 0u32;
    for i in 0u32..8 {
        let val = (read32(DRAM_BASE + DRAM_SCTLR) & !(0x7 << 6)) | (i << 6);
        write32(DRAM_BASE + DRAM_SCTLR, val);
        if sdr_readpipe_scan() {
            value = i;
            return value;
        }
    }
    value
}

unsafe fn dram_check_type(para: &mut DramPara) -> u32 {
    let mut times = 0u32;
    for i in 0u32..8 {
        let val = (read32(DRAM_BASE + DRAM_SCTLR) & !(0x7 << 6)) | (i << 6);
        write32(DRAM_BASE + DRAM_SCTLR, val);
        dram_delay_scan();
        if (read32(DRAM_BASE + DRAM_DDLYR) & 0x30) != 0 {
            times += 1;
        }
    }
    if times == 8 {
        para.sdr_ddr = DramType::Sdr;
        0
    } else {
        para.sdr_ddr = DramType::Ddr;
        1
    }
}

unsafe fn dram_scan_readpipe(para: &DramPara) {
    if para.sdr_ddr == DramType::Ddr {
        let mut rp_best = 0u32;
        let mut rp_val = 0u32;
        let mut readpipe = [0u32; 8];
        for i in 0u32..8 {
            let val = (read32(DRAM_BASE + DRAM_SCTLR) & !(0x7 << 6)) | (i << 6);
            write32(DRAM_BASE + DRAM_SCTLR, val);
            dram_delay_scan();
            readpipe[i as usize] = 0;
            let ddlyr = read32(DRAM_BASE + DRAM_DDLYR);
            if (((ddlyr >> 4) & 0x3) == 0x0) && (((ddlyr >> 4) & 0x1) == 0x0) {
                readpipe[i as usize] = dram_check_delay(para.bwidth);
            }
            if rp_val < readpipe[i as usize] {
                rp_val = readpipe[i as usize];
                rp_best = i;
            }
        }
        let val = (read32(DRAM_BASE + DRAM_SCTLR) & !(0x7 << 6)) | (rp_best << 6);
        write32(DRAM_BASE + DRAM_SCTLR, val);
        dram_delay_scan();
    } else {
        let val = read32(DRAM_BASE + DRAM_SCONR) & !(0x1 << 16) & !(0x3 << 13);
        write32(DRAM_BASE + DRAM_SCONR, val);
        let rp_best = sdr_readpipe_select();
        let val = (read32(DRAM_BASE + DRAM_SCTLR) & !(0x7 << 6)) | (rp_best << 6);
        write32(DRAM_BASE + DRAM_SCTLR, val);
    }
}

unsafe fn dram_get_dram_size(para: &mut DramPara) {
    let mut colflag: u32 = 10;
    let mut rowflag: u32 = 13;

    para.col_width = colflag;
    para.row_width = rowflag;
    dram_para_setup(para);
    dram_scan_readpipe(para);

    // Detect column width
    for i in 0u32..32 {
        write32(SDRAM_BASE + 0x200 + i, 0x1111_1111);
        write32(SDRAM_BASE + 0x600 + i, 0x2222_2222);
    }
    let mut count = 0u32;
    for i in 0u32..32 {
        if read32(SDRAM_BASE + 0x200 + i) == 0x2222_2222 {
            count += 1;
        }
    }
    if count == 32 {
        colflag = 9;
    } else {
        colflag = 10;
    }

    // Detect row width
    count = 0;
    para.col_width = colflag;
    para.row_width = rowflag;
    dram_para_setup(para);

    let (addr1, addr2) = if colflag == 10 {
        (0x8040_0000u32, 0x80c0_0000u32)
    } else {
        (0x8020_0000u32, 0x8060_0000u32)
    };
    for i in 0u32..32 {
        write32(addr1 + i, 0x3333_3333);
        write32(addr2 + i, 0x4444_4444);
    }
    for i in 0u32..32 {
        if read32(addr1 + i) == 0x4444_4444 {
            count += 1;
        }
    }
    if count == 32 {
        rowflag = 12;
    } else {
        rowflag = 13;
    }

    para.col_width = colflag;
    para.row_width = rowflag;
    if para.row_width != 13 {
        para.size = 16;
    } else if para.col_width == 10 {
        para.size = 64;
    } else {
        para.size = 32;
    }

    dram_set_autofresh_cycle(para.clk);
    para.access_mode = 0;
    dram_para_setup(para);
}

unsafe fn dram_init_inner(para: &mut DramPara) -> bool {
    let pio = &*Pio::ptr();
    let ccu = &*Ccu::ptr();

    // Configure PB3 as SDR_DQS function (func 7) — critical for DDR data strobe
    pio.pb_cfg0().modify(|_, w| w.pb3_select().bits(7));

    // Configure SDR pad driving strength
    pio.sdr_pad_drv().modify(|r, w| w.bits(r.bits() | (0x7 << 12)));
    dram_delay(5);

    // Configure SDR pad pull based on CAS
    if ((para.cas) >> 3) & 0x1 != 0 {
        pio.sdr_pad_pull()
            .modify(|r, w| w.bits(r.bits() | (0x1 << 23) | (0x20 << 17)));
    }

    // Configure SDR pad driving for clock frequency
    if para.clk >= 144 && para.clk <= 180 {
        pio.sdr_pad_drv().write(|w| w.bits(0xaaa));
    }
    if para.clk >= 180 {
        pio.sdr_pad_drv().write(|w| w.bits(0xfff));
    }

    // Configure PLL_DDR
    let val = if para.clk <= 96 {
        (0x1 << 0) | (0x0 << 4) | (((para.clk * 2) / 12 - 1) << 8) | (0x1u32 << 31)
    } else {
        (0x0 << 0) | (0x0 << 4) | (((para.clk * 2) / 24 - 1) << 8) | (0x1u32 << 31)
    };

    // Set PLL DDR pattern for sigma-delta
    if para.cas & (0x1 << 4) != 0 {
        ccu.pll_ddr_pat_ctrl().write(|w| w.bits(0xd130_3333));
    } else if para.cas & (0x1 << 5) != 0 {
        ccu.pll_ddr_pat_ctrl().write(|w| w.bits(0xcce0_6666));
    } else if para.cas & (0x1 << 6) != 0 {
        ccu.pll_ddr_pat_ctrl().write(|w| w.bits(0xc890_9999));
    } else if para.cas & (0x1 << 7) != 0 {
        ccu.pll_ddr_pat_ctrl().write(|w| w.bits(0xc440_cccc));
    }

    let val = if para.cas & (0xf << 4) != 0 {
        val | (0x1 << 24)
    } else {
        val
    };

    ccu.pll_ddr_ctrl().write(|w| w.bits(val));
    ccu.pll_ddr_ctrl().modify(|r, w| w.bits(r.bits() | (0x1 << 20)));
    // Wait for PLL lock
    while !ccu.pll_ddr_ctrl().read().lock().bit_is_set() {}
    dram_delay(5);

    // Enable SDRAM bus clock gating
    ccu.bus_clk_gating0().modify(|_, w| w.sdram_gating().set_bit());
    // Assert SDRAM reset
    ccu.bus_soft_rst0().modify(|_, w| w.sdram_rst().clear_bit());
    sdelay(20);
    // De-assert SDRAM reset
    ccu.bus_soft_rst0().modify(|_, w| w.sdram_rst().set_bit());

    // Set DDR/SDR mode in SDR pad pull register
    if para.sdr_ddr == DramType::Ddr {
        pio.sdr_pad_pull().modify(|r, w| w.bits(r.bits() | (0x1 << 16)));
    } else {
        pio.sdr_pad_pull().modify(|r, w| w.bits(r.bits() & !(0x1 << 16)));
    }

    // Set timing parameters
    let stmg0 = (SDR_T_CAS << 0)
        | (SDR_T_RAS << 3)
        | (SDR_T_RCD << 7)
        | (SDR_T_RP << 10)
        | (SDR_T_WR << 13)
        | (SDR_T_RFC << 15)
        | (SDR_T_XSR << 19)
        | (SDR_T_RC << 28);
    write32(DRAM_BASE + DRAM_STMG0R, stmg0);

    let stmg1 = (SDR_T_INIT << 0) | (SDR_T_INIT_REF << 16) | (SDR_T_WTR << 20) | (SDR_T_RRD << 22) | (SDR_T_XP << 25);
    write32(DRAM_BASE + DRAM_STMG1R, stmg1);

    // Initial setup and type detection
    if !dram_para_setup(para) {
        return false;
    }
    dram_check_type(para);

    // Update DDR/SDR mode after type detection
    if para.sdr_ddr == DramType::Ddr {
        pio.sdr_pad_pull().modify(|r, w| w.bits(r.bits() | (0x1 << 16)));
    } else {
        pio.sdr_pad_pull().modify(|r, w| w.bits(r.bits() & !(0x1 << 16)));
    }

    dram_set_autofresh_cycle(para.clk);
    dram_scan_readpipe(para);
    dram_get_dram_size(para);

    // Verification: write and read back
    for i in 0u32..128 {
        write32(para.base + 4 * i, para.base + 4 * i);
    }
    for i in 0u32..128 {
        if read32(para.base + 4 * i) != para.base + 4 * i {
            return false;
        }
    }
    true
}

/// Initialize the SDRAM controller with the given configuration.
///
/// Returns `Some(DramInfo)` on success with detected size,
/// or `None` if initialization failed.
///
/// Must be called after system clock initialization (`hal::init()`).
pub fn init_with_config(cfg: DramConfig) -> Option<DramInfo> {
    // Check if DDR is already initialized (magic marker at 0x5c)
    let dsz = unsafe { read32(0x5c) };
    if (dsz >> 24) == b'X' as u32 {
        return Some(DramInfo {
            base: SDRAM_BASE,
            size_mb: dsz & 0x00FF_FFFF,
        });
    }

    let mut para = DramPara::from_config(&cfg);

    unsafe {
        if dram_init_inner(&mut para) {
            write32(0x5c, (b'X' as u32) << 24 | para.size);
            Some(DramInfo {
                base: SDRAM_BASE,
                size_mb: para.size,
            })
        } else {
            None
        }
    }
}

/// Initialize the SDRAM controller with default configuration.
///
/// Uses the chip variant selected by Cargo feature:
/// - `f1c200s` (default): 64MB DDR1
/// - `f1c100s`: 32MB DDR1
///
/// Returns `Some(DramInfo)` on success, `None` on failure.
pub fn init() -> Option<DramInfo> {
    init_with_config(DramConfig::default())
}
