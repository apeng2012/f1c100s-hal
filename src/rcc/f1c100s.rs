use crate::pac;

/// CPU clock source selection
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpuClkSrc {
    /// 32.768kHz low-speed oscillator
    Losc,
    /// 24MHz high-speed oscillator (default at reset)
    Osc24M,
    /// PLL_CPU output
    PllCpu,
}

/// PLL_CPU configuration
///
/// Output = 24MHz * N * K / (M * P)
/// Output range: 200MHz ~ 2.6GHz, default 408MHz
#[derive(Clone, Copy, Debug)]
pub struct PllCpu {
    /// Factor N (1..=32)
    pub n: u8,
    /// Factor K (1..=4)
    pub k: u8,
    /// Factor M (1..=4)
    pub m: u8,
    /// Output divider P: 1, 2, or 4
    pub p: PllCpuP,
}

/// PLL_CPU output external divider P
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PllCpuP {
    Div1 = 0,
    Div2 = 1,
    Div4 = 2,
}

impl PllCpu {
    /// 720MHz: 24 * 30 * 1 / (1 * 1)
    pub const fn freq_720mhz() -> Self {
        Self {
            n: 30,
            k: 1,
            m: 1,
            p: PllCpuP::Div1,
        }
    }
    /// 408MHz: 24 * 17 * 1 / (1 * 1)
    pub const fn freq_408mhz() -> Self {
        Self {
            n: 17,
            k: 1,
            m: 1,
            p: PllCpuP::Div1,
        }
    }
    /// 600MHz: 24 * 25 * 1 / (1 * 1)
    pub const fn freq_600mhz() -> Self {
        Self {
            n: 25,
            k: 1,
            m: 1,
            p: PllCpuP::Div1,
        }
    }
    /// Calculate output frequency in Hz
    pub const fn freq_hz(&self) -> u32 {
        let p_val = match self.p {
            PllCpuP::Div1 => 1,
            PllCpuP::Div2 => 2,
            PllCpuP::Div4 => 4,
        };
        24_000_000 * (self.n as u32) * (self.k as u32) / ((self.m as u32) * p_val)
    }
}

/// PLL_PERIPH configuration
///
/// Output = 24MHz * N * K (should be fixed at 600MHz per manual)
/// Output range: 200MHz ~ 1.8GHz
#[derive(Clone, Copy, Debug)]
pub struct PllPeriph {
    /// Factor N (1..=32)
    pub n: u8,
    /// Factor K (1..=4)
    pub k: u8,
}

impl PllPeriph {
    /// 600MHz: 24 * 25 * 1 (recommended, do not change)
    pub const fn freq_600mhz() -> Self {
        Self { n: 25, k: 1 }
    }

    pub const fn freq_hz(&self) -> u32 {
        24_000_000 * (self.n as u32) * (self.k as u32)
    }
}

/// PLL_VIDEO mode
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PllVideoMode {
    /// Integer mode: output = 24MHz * N / M
    Integer {
        /// Factor N (1..=128)
        n: u8,
        /// Pre-div M (1..=16)
        m: u8,
    },
    /// Fractional mode: output is fixed 270MHz or 297MHz
    Fractional {
        /// true = 297MHz, false = 270MHz
        out_297mhz: bool,
    },
}

/// PLL_VIDEO configuration
///
/// Integer mode: output = 24MHz * N / M (30~600MHz)
/// Fractional mode: 270MHz or 297MHz
#[derive(Clone, Copy, Debug)]
pub struct PllVideo {
    pub mode: PllVideoMode,
}

impl PllVideo {
    /// ~198MHz integer mode: 24 * 66 / 8
    pub const fn freq_198mhz() -> Self {
        Self {
            mode: PllVideoMode::Integer { n: 66, m: 8 },
        }
    }
    /// 297MHz fractional mode
    pub const fn freq_297mhz() -> Self {
        Self {
            mode: PllVideoMode::Fractional { out_297mhz: true },
        }
    }
    /// 270MHz fractional mode
    pub const fn freq_270mhz() -> Self {
        Self {
            mode: PllVideoMode::Fractional { out_297mhz: false },
        }
    }
}

/// AHB clock source
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AhbClkSrc {
    /// 32.768kHz LOSC
    Losc = 0,
    /// 24MHz oscillator
    Osc24M = 1,
    /// CPU clock
    CpuClk = 2,
    /// PLL_PERIPH / AHB_PRE_DIV
    PllPeriph = 3,
}

/// AHB pre-divider (applied when source is PLL_PERIPH)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AhbPreDiv {
    Div1 = 0,
    Div2 = 1,
    Div3 = 2,
    Div4 = 3,
}

/// AHB clock divider
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AhbDiv {
    Div1 = 0,
    Div2 = 1,
    Div4 = 2,
    Div8 = 3,
}

/// APB clock divider (from AHB)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApbDiv {
    /// /2 (0x or 01)
    Div2 = 1,
    /// /4
    Div4 = 2,
    /// /8
    Div8 = 3,
}

/// HCLKC divider (from CPU clock)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HclkcDiv {
    Div1 = 0,
    Div2 = 1,
    Div3 = 2,
    Div4 = 3,
}

/// F1C100S Clock Configuration (embassy-stm32 style)
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// PLL_CPU config. None = don't touch PLL_CPU
    pub pll_cpu: Option<PllCpu>,
    /// PLL_PERIPH config. None = don't touch PLL_PERIPH
    pub pll_periph: Option<PllPeriph>,
    /// PLL_VIDEO config. None = don't touch PLL_VIDEO
    pub pll_video: Option<PllVideo>,
    /// CPU clock source
    pub cpu_src: CpuClkSrc,
    /// AHB clock source
    pub ahb_src: AhbClkSrc,
    /// AHB pre-divider
    pub ahb_pre_div: AhbPreDiv,
    /// AHB clock divider
    pub ahb_div: AhbDiv,
    /// APB clock divider
    pub apb_div: ApbDiv,
    /// HCLKC divider
    pub hclkc_div: HclkcDiv,
    /// Enable DE front-end/back-end DRAM clock gating
    pub de_dram_gating: bool,
}

impl Default for Config {
    /// Default: CPU=720MHz via PLL_CPU, PERIPH=600MHz, VIDEO=198MHz,
    /// AHB=200MHz, APB=100MHz — same as the C reference sys_clock_init()
    fn default() -> Self {
        Self {
            pll_cpu: Some(PllCpu::freq_720mhz()),
            pll_periph: Some(PllPeriph::freq_600mhz()),
            pll_video: Some(PllVideo::freq_198mhz()),
            cpu_src: CpuClkSrc::PllCpu,
            ahb_src: AhbClkSrc::PllPeriph,
            ahb_pre_div: AhbPreDiv::Div3, // 600/3 = 200MHz
            ahb_div: AhbDiv::Div1,        // 200/1 = 200MHz
            apb_div: ApbDiv::Div2,        // 200/2 = 100MHz
            hclkc_div: HclkcDiv::Div1,
            de_dram_gating: true,
        }
    }
}

/// Simple delay loop
#[inline(always)]
fn sdelay(loops: u32) {
    for _ in 0..loops {
        core::hint::spin_loop();
    }
}

/// Wait for PLL_CPU lock bit (bit 28)
fn wait_pll_cpu_stable(ccu: &pac::ccu::RegisterBlock) {
    let mut timeout = 0xFFFFu32;
    while timeout > 0 {
        if ccu.pll_cpu_ctrl().read().lock().bit_is_set() {
            break;
        }
        timeout -= 1;
    }
}

/// Wait for PLL_PERIPH lock bit (bit 28)
fn wait_pll_periph_stable(ccu: &pac::ccu::RegisterBlock) {
    let mut timeout = 0xFFFFu32;
    while timeout > 0 {
        if ccu.pll_periph_ctrl().read().lock().bit_is_set() {
            break;
        }
        timeout -= 1;
    }
}

/// Wait for PLL_VIDEO lock bit (bit 28)
fn wait_pll_video_stable(ccu: &pac::ccu::RegisterBlock) {
    let mut timeout = 0xFFFFu32;
    while timeout > 0 {
        if ccu.pll_video_ctrl().read().lock().bit_is_set() {
            break;
        }
        timeout -= 1;
    }
}

/// Initialize the F1C100S clock tree.
pub(crate) unsafe fn init(config: &Config) {
    let ccu = &*pac::Ccu::ptr();

    // 1. Set PLL stable time
    ccu.pll_stable_time0().write(|w| w.pll_lock_time().bits(0x1ff));
    ccu.pll_stable_time1().write(|w| w.pll_cpu_lock_time().bits(0x1ff));

    // 2. Switch CPU to OSC24M first (safe clock source before PLL changes)
    ccu.cpu_clk_src().modify(|_, w| w.cpu_clk_src_sel().bits(0x01));
    sdelay(100);

    // 3. Configure PLL_VIDEO
    if let Some(pll_video) = &config.pll_video {
        match pll_video.mode {
            PllVideoMode::Integer { n, m } => {
                ccu.pll_video_ctrl().write(|w| {
                    w.pll_en().set_bit();
                    w.pll_mode_sel().set_bit(); // integer mode
                    w.pll_factor_n().bits(n - 1);
                    w.pll_prediv_m().bits(m - 1)
                });
            }
            PllVideoMode::Fractional { out_297mhz } => {
                ccu.pll_video_ctrl().write(|w| {
                    w.pll_en().set_bit();
                    w.pll_mode_sel().clear_bit(); // fractional mode
                    w.frac_clk_out().bit(out_297mhz);
                    w.pll_prediv_m().bits(0) // M must be 0 in fractional mode
                });
            }
        }
        sdelay(100);
        wait_pll_video_stable(ccu);
    }

    // 4. Configure PLL_PERIPH
    if let Some(pll_periph) = &config.pll_periph {
        ccu.pll_periph_ctrl().write(|w| {
            w.pll_en().set_bit();
            w.pll_factor_n().bits(pll_periph.n - 1);
            w.pll_factor_k().bits(pll_periph.k - 1);
            w.pll_factor_m().bits(0) // M=1 (normal output)
        });
        sdelay(100);
        wait_pll_periph_stable(ccu);
    }

    // 5. Configure AHB/APB/HCLKC bus clocks
    //    Per manual: set division first, then switch source
    ccu.ahb_apb_hclkc_cfg().write(|w| {
        w.hclkc_div().bits(config.hclkc_div as u8);
        w.ahb_clk_src_sel().bits(config.ahb_src as u8);
        w.apb_clk_ratio().bits(config.apb_div as u8);
        w.ahb_pre_div().bits(config.ahb_pre_div as u8);
        w.ahb_clk_div_ratio().bits(config.ahb_div as u8)
    });
    sdelay(100);

    // 6. Enable DE front-end/back-end DRAM clock gating
    if config.de_dram_gating {
        ccu.dram_gating().modify(|_, w| {
            w.fe_dclk_gating().set_bit();
            w.be_dclk_gating().set_bit()
        });
        sdelay(100);
    }

    // 7. Configure PLL_CPU
    if let Some(pll_cpu) = &config.pll_cpu {
        ccu.pll_cpu_ctrl().modify(|_, w| {
            w.pll_en().set_bit();
            w.pll_out_ext_div_p().bits(pll_cpu.p as u8);
            w.pll_factor_n().bits(pll_cpu.n - 1);
            w.pll_factor_k().bits(pll_cpu.k - 1);
            w.pll_factor_m().bits(pll_cpu.m - 1)
        });
        wait_pll_cpu_stable(ccu);
    }

    // 8. Switch CPU clock source to final selection
    let cpu_src_bits = match config.cpu_src {
        CpuClkSrc::Losc => 0x00,
        CpuClkSrc::Osc24M => 0x01,
        CpuClkSrc::PllCpu => 0x02,
    };
    ccu.cpu_clk_src().modify(|_, w| w.cpu_clk_src_sel().bits(cpu_src_bits));
    sdelay(100);

    // Update global clock tracking
    super::update_clocks(config);
}
