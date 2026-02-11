/* F1C100S / F1C200S SPL Mode Memory Layout */
/* SPL (f1c100s-spl_uart0.bin) 已初始化时钟、SDRAM、MMU */
/* 主程序直接运行在 SDRAM 中，32KB SRAM 全部可用 */
/*
 * 启动流程:
 *   FEL模式: sunxi-fel 写入SPL到SRAM并执行 → SPL初始化DRAM → 返回FEL
 *            → sunxi-fel 写入主程序到0x80000000 → 执行
 *   SPI模式: BROM加载SPL → SPL初始化DRAM → 从SPI Flash 0x8000读取主程序到SDRAM → 跳转
 */
MEMORY
{
    /* SDRAM 前半部分 - 代码 + 只读数据 (FLASH) */
    /* 注意: SPL 的 MMU 页表 (TTB) 在 0x80004000-0x80008000 (16KB) */
    /* 程序必须从 0x80008000 之后开始，避免覆盖页表导致 MMU 异常 */
    /* F1C200S: 32MB, F1C100S: 16MB */
    FLASH : ORIGIN = 0x80008000, LENGTH = 32M - 32K

    /* SDRAM 后半部分 - 数据 + BSS + 堆 + 栈 (RAM) */
    RAM   : ORIGIN = 0x82000000, LENGTH = 32M
}
