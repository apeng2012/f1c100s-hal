#!/usr/bin/env python3
"""
生成 F1C100S 可启动的 SPI Flash / SD Card 镜像
添加 eGON.BT0 头

参考:
- https://linux-sunxi.org/Boot0
- sunxi-tools/uart0-helloworld-sdboot.c
- sunxi-tools/fel-sdboot.S
"""

import struct
import sys

# eGON.BT0 header 大小
HEADER_SIZE = 32

# BROM 会在 header 后面写入 boot device info，所以代码需要从更后面开始
# 参考 sunxi-tools 的 workaround，代码应该从 0x30 开始
CODE_START_OFFSET = 0x30

def make_boot0(bin_file, output_file):
    with open(bin_file, 'rb') as f:
        code = f.read()
    
    # 计算需要的填充
    # header (32 bytes) + padding (到 CODE_START_OFFSET) + code
    padding_size = CODE_START_OFFSET - HEADER_SIZE
    
    # 对齐到 512 字节 (SD card) 或 8KB (NAND)
    total_size = CODE_START_OFFSET + len(code)
    aligned_size = (total_size + 511) & ~511
    tail_padding = aligned_size - total_size
    
    # 构建 eGON.BT0 header (32 字节)
    # 参考: https://linux-sunxi.org/Boot0
    header = bytearray(HEADER_SIZE)
    
    # 0x00: 跳转指令 - 跳转到 CODE_START_OFFSET
    # ARM: b +offset = 0xEA000000 | ((offset/4 - 2) & 0x00FFFFFF)
    # 从 0x00 跳转到 0x30: offset = 0x30, (0x30/4 - 2) = 10 = 0x0A
    jump_offset = (CODE_START_OFFSET // 4) - 2
    header[0:4] = struct.pack('<I', 0xEA000000 | (jump_offset & 0x00FFFFFF))
    
    # 0x04: Magic: "eGON.BT0"
    header[4:12] = b'eGON.BT0'
    
    # 0x0C: Checksum - 先填入 0x5F0A6C39，后面计算
    header[12:16] = struct.pack('<I', 0x5F0A6C39)
    
    # 0x10: Length (整个镜像大小，包括 header)
    header[16:20] = struct.pack('<I', aligned_size)
    
    # 0x14-0x1F: 其他字段 (pub_head_size, versions 等)
    # pub_head_size = 32
    header[20:24] = struct.pack('<I', HEADER_SIZE)
    # 其余填 0
    
    # 构建完整镜像
    # header + padding + code + tail_padding
    padding = b'\xff' * padding_size  # 使用 0xff 填充，类似 sunxi-tools
    tail = b'\x00' * tail_padding
    
    image = bytes(header) + padding + code + tail
    
    # 计算校验和
    # 校验和 = 所有 32 位字的和
    checksum = 0
    for i in range(0, len(image), 4):
        word = struct.unpack('<I', image[i:i+4])[0]
        checksum += word
    checksum &= 0xFFFFFFFF
    
    # 写回校验和
    image = bytearray(image)
    image[12:16] = struct.pack('<I', checksum)
    
    with open(output_file, 'wb') as f:
        f.write(image)
    
    print(f"Created {output_file}")
    print(f"  Code size: {len(code)} bytes")
    print(f"  Total size: {len(image)} bytes (aligned to 512)")
    print(f"  Code starts at: 0x{CODE_START_OFFSET:02X}")
    print(f"  Checksum: 0x{checksum:08X}")

if __name__ == '__main__':
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <input.bin> <output.bin>")
        print()
        print("Creates a bootable image for F1C100S SPI flash or SD card")
        print("by adding an eGON.BT0 header to the input binary.")
        sys.exit(1)
    
    make_boot0(sys.argv[1], sys.argv[2])
