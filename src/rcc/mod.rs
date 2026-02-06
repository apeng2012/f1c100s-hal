use crate::time::Hertz;

mod f1c100s;
pub use f1c100s::*;

const HSE_FREQ: u32 = 24_000_000;

static mut CLOCKS: Clocks = Clocks {
    sysclk: Hertz(HSE_FREQ),
    hclk: Hertz(HSE_FREQ),
    pclk: Hertz(HSE_FREQ),
};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Clocks {
    /// CPU / system clock
    pub sysclk: Hertz,
    /// AHB clock
    pub hclk: Hertz,
    /// APB clock
    pub pclk: Hertz,
}

#[inline]
pub fn clocks() -> &'static Clocks {
    unsafe { &CLOCKS }
}

/// Compute and cache clock frequencies from config.
fn update_clocks(config: &Config) {
    let sysclk = match config.cpu_src {
        CpuClkSrc::Losc => 32_768,
        CpuClkSrc::Osc24M => HSE_FREQ,
        CpuClkSrc::PllCpu => config.pll_cpu.map(|p| p.freq_hz()).unwrap_or(HSE_FREQ),
    };

    let pll_periph_hz = config.pll_periph.map(|p| p.freq_hz()).unwrap_or(HSE_FREQ);

    let ahb_pre = match config.ahb_pre_div {
        AhbPreDiv::Div1 => 1u32,
        AhbPreDiv::Div2 => 2,
        AhbPreDiv::Div3 => 3,
        AhbPreDiv::Div4 => 4,
    };
    let ahb_ratio = match config.ahb_div {
        AhbDiv::Div1 => 1u32,
        AhbDiv::Div2 => 2,
        AhbDiv::Div4 => 4,
        AhbDiv::Div8 => 8,
    };

    let ahb_input = match config.ahb_src {
        AhbClkSrc::Losc => 32_768,
        AhbClkSrc::Osc24M => HSE_FREQ,
        AhbClkSrc::CpuClk => sysclk,
        AhbClkSrc::PllPeriph => pll_periph_hz / ahb_pre,
    };
    let hclk = ahb_input / ahb_ratio;

    let apb_ratio = match config.apb_div {
        ApbDiv::Div2 => 2u32,
        ApbDiv::Div4 => 4,
        ApbDiv::Div8 => 8,
    };
    let pclk = hclk / apb_ratio;

    unsafe {
        CLOCKS = Clocks {
            sysclk: Hertz(sysclk),
            hclk: Hertz(hclk),
            pclk: Hertz(pclk),
        };
    }
}

pub unsafe fn init(config: Config) {
    f1c100s::init(&config);
}
