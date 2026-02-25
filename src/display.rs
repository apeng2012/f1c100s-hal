//! Display driver for F1C100S/F1C200S LCD controller (TCON0 + DEBE).
//!
//! Supports RGB HV parallel interface with 18-bit FRM dithering.
//! Framebuffer is RGB565 in SDRAM.
//! All register access uses the `f1c100s-pac` typed register API.

use crate::pac;

/// LCD timing configuration.
pub struct LcdConfig {
    pub width: u16,
    pub height: u16,
    pub pixel_clock_hz: u32,
    pub h_front_porch: u16,
    pub h_back_porch: u16,
    pub h_sync_len: u16,
    pub v_front_porch: u16,
    pub v_back_porch: u16,
    pub v_sync_len: u16,
    /// Physical panel bits per color channel (18 or 16 for FRM dithering)
    pub line_per_pixel: u8,
    /// Invert DCLK polarity
    pub dclk_invert: bool,
}

impl LcdConfig {
    /// 800x480 panel with typical timing (AT070TN92 / EK9716 style).
    /// Assumes PLL_VIDEO = 198MHz, pixel clock ≈ 33MHz (divider = 6).
    pub const fn lcd_800x480() -> Self {
        Self {
            width: 800,
            height: 480,
            pixel_clock_hz: 33_000_000,
            h_front_porch: 40,
            h_back_porch: 88,
            h_sync_len: 48,
            v_front_porch: 13,
            v_back_porch: 32,
            v_sync_len: 3,
            line_per_pixel: 18,
            dclk_invert: true,
        }
    }
}

/// Display controller. Manages TCON0 + DEBE + framebuffer.
pub struct Display {
    width: u16,
    height: u16,
    fb: *mut u16,
}

impl Display {
    /// Initialize the LCD display subsystem.
    ///
    /// `fb_addr` must point to a framebuffer in SDRAM, aligned to at least 4 bytes,
    /// with size >= width * height * 2 bytes (RGB565).
    ///
    /// # Safety
    /// - Must be called after clock init (PLL_VIDEO must be configured).
    /// - `fb_addr` must be a valid, writable SDRAM address.
    /// - Must only be called once.
    pub unsafe fn new(config: &LcdConfig, fb_addr: *mut u16) -> Self {
        let ccu = &*pac::Ccu::ptr();

        // 1. Enable clocks: DEFE, DEBE, TCON bus gating
        ccu.fe_clk().write(|w| {
            w.sclk_gating().set_bit();
            w.clk_src_sel().bits(0); // PLL_VIDEO
            w.clk_div_ratio_m().bits(0) // div 1
        });
        ccu.be_clk().write(|w| {
            w.sclk_gating().set_bit();
            w.clk_src_sel().bits(0); // PLL_VIDEO
            w.clk_div_ratio_m().bits(0) // div 1
        });
        ccu.tcon_clk().write(|w| {
            w.sclk_gating().set_bit();
            w.clk_src_sel().bits(0) // PLL_VIDEO(1X)
        });

        // Bus clock gating
        ccu.bus_clk_gating1().modify(|_, w| {
            w.defe_gating().set_bit();
            w.debe_gating().set_bit();
            w.lcd_gating().set_bit()
        });

        // De-assert resets
        ccu.bus_soft_rst1().modify(|_, w| {
            w.defe_rst().set_bit();
            w.debe_rst().set_bit();
            w.lcd_rst().set_bit()
        });

        // DRAM gating for BE/FE
        ccu.dram_gating().modify(|_, w| {
            w.fe_dclk_gating().set_bit();
            w.be_dclk_gating().set_bit()
        });

        // 2. Configure LCD GPIO pins: PD0-PD21 as function 2 (LCD)
        Self::init_lcd_pins();

        // 3. Clear DEBE register area 0x800..0x1000 via SRAM blocks
        Self::clear_debe_sram();

        // 4. Disable TCON first
        Self::tcon_disable();

        // 5. Configure DEBE
        Self::debe_set_mode(config, fb_addr);

        // 6. Configure TCON0
        Self::tcon_set_mode(config);

        // 7. Enable TCON
        Self::tcon_enable();

        // 8. Enable layer 0
        Self::layer_enable(0, true);

        // Clear framebuffer to black
        let fb_size = config.width as usize * config.height as usize;
        core::ptr::write_bytes(fb_addr, 0, fb_size);

        Self {
            width: config.width,
            height: config.height,
            fb: fb_addr,
        }
    }

    /// Width in pixels.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Get raw framebuffer pointer.
    pub fn framebuffer(&self) -> *mut u16 {
        self.fb
    }

    /// Set a pixel at (x, y) to the given RGB565 color.
    #[inline]
    pub fn set_pixel(&self, x: u16, y: u16, color: u16) {
        if x < self.width && y < self.height {
            unsafe {
                let offset = y as usize * self.width as usize + x as usize;
                self.fb.add(offset).write_volatile(color);
            }
        }
    }

    /// Fill the entire screen with a color.
    pub fn fill(&self, color: u16) {
        let total = self.width as usize * self.height as usize;
        for i in 0..total {
            unsafe {
                self.fb.add(i).write_volatile(color);
            }
        }
    }

    /// Draw a horizontal line.
    pub fn draw_hline(&self, x0: u16, x1: u16, y: u16, color: u16) {
        let start = x0.min(x1);
        let end = x0.max(x1);
        for x in start..=end {
            self.set_pixel(x, y, color);
        }
    }

    /// Draw a vertical line.
    pub fn draw_vline(&self, x: u16, y0: u16, y1: u16, color: u16) {
        let start = y0.min(y1);
        let end = y0.max(y1);
        for y in start..=end {
            self.set_pixel(x, y, color);
        }
    }

    /// Draw a line using Bresenham's algorithm.
    pub fn draw_line(&self, x0: i16, y0: i16, x1: i16, y1: i16, color: u16) {
        let mut x0 = x0;
        let mut y0 = y0;
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx: i16 = if x0 < x1 { 1 } else { -1 };
        let sy: i16 = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            self.set_pixel(x0 as u16, y0 as u16, color);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    /// Draw a rectangle outline.
    pub fn draw_rect(&self, x: u16, y: u16, w: u16, h: u16, color: u16) {
        self.draw_hline(x, x + w - 1, y, color);
        self.draw_hline(x, x + w - 1, y + h - 1, color);
        self.draw_vline(x, y, y + h - 1, color);
        self.draw_vline(x + w - 1, y, y + h - 1, color);
    }

    /// Fill a rectangle.
    pub fn fill_rect(&self, x: u16, y: u16, w: u16, h: u16, color: u16) {
        for row in y..y + h {
            for col in x..x + w {
                self.set_pixel(col, row, color);
            }
        }
    }

    // --- Private hardware init helpers ---

    /// Configure PD0-PD21 as LCD function (function 2), drive level 3, no pull.
    unsafe fn init_lcd_pins() {
        let pio = &*pac::Pio::ptr();

        // PD0-PD7: function 2 (LCD)
        pio.pd_cfg0().write(|w| {
            w.pd0_select().bits(2);
            w.pd1_select().bits(2);
            w.pd2_select().bits(2);
            w.pd3_select().bits(2);
            w.pd4_select().bits(2);
            w.pd5_select().bits(2);
            w.pd6_select().bits(2);
            w.pd7_select().bits(2)
        });

        // PD8-PD15: function 2 (LCD)
        pio.pd_cfg1().write(|w| {
            w.pd8_select().bits(2);
            w.pd9_select().bits(2);
            w.pd10_select().bits(2);
            w.pd11_select().bits(2);
            w.pd12_select().bits(2);
            w.pd13_select().bits(2);
            w.pd14_select().bits(2);
            w.pd15_select().bits(2)
        });

        // PD16-PD21: function 2 (LCD)
        pio.pd_cfg2().write(|w| {
            w.pd16_select().bits(2);
            w.pd17_select().bits(2);
            w.pd18_select().bits(2);
            w.pd19_select().bits(2);
            w.pd20_select().bits(2);
            w.pd21_select().bits(2)
        });

        // Drive level 3 (strongest) for PD0-PD15: 2 bits per pin, 0b11 each
        let drv_all_3: u32 = 0xFFFF_FFFF; // all pins level 3
        pio.pd_drv0().write(|w| w.pd_drv().bits(drv_all_3));

        // Drive level 3 for PD16-PD21: 6 pins * 2 bits = 12 bits
        let drv_pd16_21: u16 = 0x0FFF; // 6 pins, all level 3
        pio.pd_drv1().write(|w| w.pd_drv().bits(drv_pd16_21));

        // Disable pull for PD0-PD15
        pio.pd_pull0().write(|w| w.pd_pull().bits(0));
        // Disable pull for PD16-PD21
        pio.pd_pull1().write(|w| w.pd_pull().bits(0));
    }

    /// Clear DEBE internal SRAM area (0x800..0x1000 relative to DEBE base).
    /// The PAC maps 0x800 as debe_mode_ctrl_reg, 0x804 as debe_back_color_reg,
    /// then various layer registers. We zero the mode_ctrl and back_color first,
    /// then use raw pointer for the unmapped 0x808..0x810 gap, then zero
    /// all layer config registers and SRAM blocks.
    unsafe fn clear_debe_sram() {
        // Zero the entire 0x800..0x1000 region via raw pointer.
        // This is the DEBE register + SRAM init area that must be cleared
        // before configuration. The PAC doesn't map all of it contiguously.
        let debe_base = pac::Debe::ptr() as *mut u8;
        let start = debe_base.add(0x800);
        core::ptr::write_bytes(start, 0, 0x800); // 0x800 bytes = 2048 bytes
    }

    /// Disable TCON: clear control, interrupts, clock, tristate all outputs.
    unsafe fn tcon_disable() {
        let tcon = &*pac::Tcon::ptr();

        // TCON_CTRL = 0 (module disable)
        tcon.tcon_ctrl_reg().write(|w| w);

        // TCON_INT0 = 0 (disable all interrupts)
        tcon.tcon_int_reg0().write(|w| w);

        // TCON0_DCLK: clear enable bits [31:28]
        tcon.tcon_clk_ctrl_reg().modify(|_, w| w.lclk_en().bits(0));

        // Tristate all TCON0 outputs (1 = tristate)
        tcon.tcon0_io_ctrl_reg1().write(|w| {
            w.d_output_tri_en().bits(0x00FF_FFFF);
            w.io0_output_tri_en().set_bit();
            w.io1_output_tri_en().set_bit();
            w.io2_output_tri_en().set_bit();
            w.io3_output_tri_en().set_bit()
        });

        // Tristate all TCON1 outputs
        tcon.tcon1_io_ctrl_reg1().write(|w| {
            w.d_output_tri_en().bits(0x00FF_FFFF);
            w.io0_output_tri_en().set_bit();
            w.io1_output_tri_en().set_bit();
            w.io2_output_tri_en().set_bit();
            w.io3_output_tri_en().set_bit()
        });
    }

    /// Configure DEBE: layer 0 with RGB565 framebuffer.
    unsafe fn debe_set_mode(config: &LcdConfig, fb_addr: *mut u16) {
        let debe = &*pac::Debe::ptr();
        let w = config.width as u16;
        let h = config.height as u16;

        // Enable DEBE module
        debe.debe_mode_ctrl_reg().modify(|_, wr| wr.en().set_bit());

        // Display size register at offset 0x808 (not in PAC, use raw pointer)
        let debe_base = pac::Debe::ptr() as *mut u32;
        debe_base
            .byte_add(0x808)
            .write_volatile(((h as u32 - 1) << 16) | (w as u32 - 1));

        // Layer 0 size
        debe.debe_lay0_size_reg().write(|wr| {
            wr.lay_width().bits(w - 1);
            wr.lay_height().bits(h - 1)
        });

        // Layer 0 position (0, 0)
        debe.debe_lay0_codnt_reg().write(|wr| {
            wr.x_coord().bits(0);
            wr.y_coord().bits(0)
        });

        // Layer 0 stride: width * 16 bits (RGB565 = 16 bits per pixel)
        debe.debe_lay0_linewidth_reg()
            .write(|wr| wr.line_width().bits(w as u32 * 16));

        // Layer 0 framebuffer address (address in bits = byte_addr << 3)
        // The full bit-address is 35 bits: low 32 bits in 0x850, high bits in 0x860
        let addr = fb_addr as u32;
        debe.debe_lay0_fb_addr_reg().write(|wr| wr.buf_addr().bits(addr << 3));
        // High bits of framebuffer address (offset 0x860, not in PAC)
        debe_base.byte_add(0x860).write_volatile(addr >> 29);

        // Layer 0 attr1: RGB565 format = 0x05
        debe.debe_lay0_att_ctrl_reg1().write(|wr| wr.fb_fmt().bits(0x05));

        // Layer 0 attr0: alpha=255, alpha_enable=1
        debe.debe_lay0_att_ctrl_reg0().write(|wr| {
            wr.alpha_val().bits(255);
            wr.alpha_en().set_bit()
        });

        // Register load control: trigger register update
        debe.debe_reg_buff_ctrl_reg()
            .modify(|_, wr| wr.reg_load_ctrl().set_bit());

        // Enable output (start bit)
        debe.debe_mode_ctrl_reg().modify(|_, wr| wr.start().set_bit());
    }

    /// Configure TCON0 for HV parallel RGB mode.
    unsafe fn tcon_set_mode(config: &LcdConfig) {
        let tcon = &*pac::Tcon::ptr();
        let w = config.width as u32;
        let h = config.height as u32;

        // TCON_CTRL: select TCON0 IO map (io_map_sel = 0)
        tcon.tcon_ctrl_reg().modify(|_, wr| wr.io_map_sel().clear_bit());

        // TCON0_CTRL: enable, HV mode (if=0), STA delay
        let sta_dly = config.v_front_porch as u8 + config.v_back_porch as u8 + config.v_sync_len as u8;
        tcon.tcon0_ctrl_reg().write(|wr| {
            wr.tcon0_en().set_bit();
            wr.if_().bits(0); // HV (Sync+DE) mode
            wr.tcon0_sta_dly().bits(sta_dly & 0x1F);
            wr.tcon0_src_sel().bits(0) // DE CH1
        });

        // TCON0_DCLK: enable all dclk (0xF), set divider
        let pll_video_hz = Self::get_pll_video_freq();
        let div = (pll_video_hz / config.pixel_clock_hz) as u8;
        tcon.tcon_clk_ctrl_reg().write(|wr| {
            wr.lclk_en().bits(0xF);
            wr.dclk_div().bits(div)
        });

        // TCON0 active timing: width-1, height-1
        tcon.tcon0_basic_timing_reg0().write(|wr| {
            wr.tcon0_x().bits(w as u16 - 1);
            wr.tcon0_y().bits(h as u16 - 1)
        });

        // TCON0 horizontal timing
        let h_bp = config.h_sync_len as u32 + config.h_back_porch as u32;
        let h_total = w + config.h_front_porch as u32 + h_bp;
        tcon.tcon0_basic_timing_reg1().write(|wr| {
            wr.ht().bits((h_total - 1) as u16);
            wr.hbp().bits((h_bp - 1) as u16)
        });

        // TCON0 vertical timing
        let v_bp = config.v_sync_len as u32 + config.v_back_porch as u32;
        let v_total = h + config.v_front_porch as u32 + v_bp;
        tcon.tcon0_basic_timing_reg2().write(|wr| {
            wr.vt().bits((v_total * 2) as u16);
            wr.vbp().bits((v_bp - 1) as u16)
        });

        // TCON0 sync timing
        tcon.tcon0_basic_timing_reg3().write(|wr| {
            wr.hspw().bits(config.h_sync_len - 1);
            wr.vspw().bits((config.v_sync_len - 1) as u8)
        });

        // TCON0 HV interface: parallel RGB mode (all 0)
        tcon.tcon0_hv_timing_reg().write(|wr| wr);

        // TCON0 CPU interface: not used (all 0)
        tcon.tcon0_cpu_if_ctrl_reg().write(|wr| wr);

        // FRM (Frame Rate Modulator) for 18-bit panel dithering
        if config.line_per_pixel == 18 || config.line_per_pixel == 16 {
            // FRM seeds (all 0x11111111)
            tcon.tcon_frm_seed0_r_reg()
                .write(|wr| wr.seed_r_value0().bits(0x11111111));
            tcon.tcon_frm_seed0_g_reg()
                .write(|wr| wr.seed_g_value0().bits(0x11111111));
            tcon.tcon_frm_seed0_b_reg()
                .write(|wr| wr.seed_b_value0().bits(0x11111111));
            tcon.tcon_frm_seed1_r_reg().write(|wr| wr.seed_r_value1().bits(0x1111));
            tcon.tcon_frm_seed1_g_reg().write(|wr| wr.seed_g_value1().bits(0x1111));
            tcon.tcon_frm_seed1_b_reg().write(|wr| wr.seed_b_value1().bits(0x1111));

            // FRM table
            tcon.tcon_frm_tbl_reg0()
                .write(|wr| wr.frm_tbl_value0().bits(0x0101_0000));
            tcon.tcon_frm_tbl_reg1()
                .write(|wr| wr.frm_tbl_value1().bits(0x1515_1111));
            tcon.tcon_frm_tbl_reg2()
                .write(|wr| wr.frm_tbl_value2().bits(0x5757_5555));
            tcon.tcon_frm_tbl_reg3()
                .write(|wr| wr.frm_tbl_value3().bits(0x7F7F_7777));

            // FRM control: enable, mode per channel (0 = 6bit, 1 = 5bit)
            if config.line_per_pixel == 18 {
                // 18-bit panel: 6-bit per channel (mode = 0)
                tcon.tcon_frm_ctrl_reg().write(|wr| {
                    wr.tcon0_frm_en().set_bit();
                    wr.tcon0_frm_mode_r().clear_bit();
                    wr.tcon0_frm_mode_g().clear_bit();
                    wr.tcon0_frm_mode_b().clear_bit()
                });
            } else {
                // 16-bit panel: 5-bit per channel (mode = 1)
                tcon.tcon_frm_ctrl_reg().write(|wr| {
                    wr.tcon0_frm_en().set_bit();
                    wr.tcon0_frm_mode_r().set_bit();
                    wr.tcon0_frm_mode_g().clear_bit(); // green is 6-bit in 565
                    wr.tcon0_frm_mode_b().set_bit()
                });
            }
        }

        // IO polarity: DCLK phase select
        // When dclk_invert is set, use DCLK1 (1/3 phase shift) via dclk_sel=1
        // This matches the hardware behavior of the original raw-register driver.
        tcon.tcon0_io_ctrl_reg0().write(|wr| {
            if config.dclk_invert {
                wr.dclk_sel().bits(1);
            }
            wr
        });

        // IO tristate: enable all outputs (write 0 = all outputs enabled)
        // Note: reset value is 0x0FFF_FFFF (all tristate), must explicitly clear
        tcon.tcon0_io_ctrl_reg1().write(|wr| {
            wr.d_output_tri_en().bits(0);
            wr.io0_output_tri_en().clear_bit();
            wr.io1_output_tri_en().clear_bit();
            wr.io2_output_tri_en().clear_bit();
            wr.io3_output_tri_en().clear_bit()
        });
    }

    /// Enable TCON module.
    unsafe fn tcon_enable() {
        let tcon = &*pac::Tcon::ptr();
        tcon.tcon_ctrl_reg().modify(|_, w| w.module_en().set_bit());
    }

    /// Enable or disable a DEBE layer.
    unsafe fn layer_enable(layer: u8, enable: bool) {
        let debe = &*pac::Debe::ptr();
        debe.debe_mode_ctrl_reg().modify(|_, w| match layer {
            0 => {
                if enable {
                    w.lay0_en().set_bit()
                } else {
                    w.lay0_en().clear_bit()
                }
            }
            1 => {
                if enable {
                    w.lay1_en().set_bit()
                } else {
                    w.lay1_en().clear_bit()
                }
            }
            2 => {
                if enable {
                    w.lay2_en().set_bit()
                } else {
                    w.lay2_en().clear_bit()
                }
            }
            3 => {
                if enable {
                    w.lay3_en().set_bit()
                } else {
                    w.lay3_en().clear_bit()
                }
            }
            _ => w,
        });
    }

    /// Read PLL_VIDEO frequency from CCU registers.
    unsafe fn get_pll_video_freq() -> u32 {
        let ccu = &*pac::Ccu::ptr();
        let pll = ccu.pll_video_ctrl().read();
        if pll.pll_mode_sel().bit_is_set() {
            // Integer mode: freq = 24MHz * (N+1) / (M+1)
            let n = pll.pll_factor_n().bits() as u32 + 1;
            let m = pll.pll_prediv_m().bits() as u32 + 1;
            24_000_000 * n / m
        } else {
            // Fractional mode
            if pll.frac_clk_out().bit_is_set() {
                297_000_000
            } else {
                270_000_000
            }
        }
    }
}

/// Convert RGB888 to RGB565.
#[inline]
pub const fn rgb565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0xF8) << 8) | ((g as u16 & 0xFC) << 3) | ((b as u16) >> 3)
}
