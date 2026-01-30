ENTRY(_start)

SECTIONS
{
    /* Move the kernel start address to 1MB + 1MB offset to avoid collisions */
    . = 0x100000;

    .text :
    {
        *(.text .text.*)
    }

    .rodata :
    {
        *(.rodata .rodata.*)
    }

    .data :
    {
        *(.data .data.*)
    }

    .bss :
    {
        *(.bss .bss.*)
    }
}