/* F1C100S / F1C200S Memory Layout */
MEMORY
{
    /* 内置 32KB SRAM - 用于从 FEL 或 SPI Flash 启动时的初始代码 */
    /* arm9-rt 需要 FLASH 和 RAM 区域 */
    /* Embassy 需要更多空间，调整为 24K FLASH + 8K RAM */
    FLASH : ORIGIN = 0x00000000, LENGTH = 24K
    RAM   : ORIGIN = 0x00006000, LENGTH = 8K
    
    /* DDR 内存 - F1C100S: 32MB, F1C200S: 64MB */
    /* 如果需要使用 DDR，需要先初始化 DRAM 控制器 */
    /* DRAM : ORIGIN = 0x80000000, LENGTH = 32M */
}

/* F1C100S 从 SPI/SD 直接启动需要 eGON.BT0 header (0x30 字节) */
_boot_header_size = 0x30;
