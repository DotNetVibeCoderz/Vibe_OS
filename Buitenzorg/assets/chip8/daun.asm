; Daun -- an original CHIP-8 demo for Buitenzorg OS: a leaf drifts across
; the screen and bounces off the edges with a click.
; Build: python scripts/chip8asm.py assets/chip8/daun.asm assets/chip8/DAUN.CH8
;
; registers: v0 x, v1 y, v2 dx (1 or 0xFF), v3 dy, v4 scratch, v5/v6 the drawn x/y

    cls
    ld v0, 20
    ld v1, 10
    ld v2, 1
    ld v3, 1
    ld i, leaf
    drw v0, v1, 6
loop:
    ld v4, dt
    se v4, 0
    jp loop
    ld v4, 2
    ld dt, v4
    ld v5, v0             ; remember where the leaf is drawn
    ld v6, v1
    add v0, v2
    add v1, v3
    ; x: bounce at 0 and 56
    se v0, 0
    jp x_right
    ld v2, 1
    call click
x_right:
    se v0, 56
    jp y_top
    ld v2, 0xFF
    call click
y_top:
    se v1, 0
    jp y_bottom
    ld v3, 1
    call click
y_bottom:
    se v1, 26
    jp draw
    ld v3, 0xFF
    call click
draw:                     ; erase and redraw back to back: no frame shows it gone
    ld i, leaf
    drw v5, v6, 6
    drw v0, v1, 6
    jp loop

click:
    ld v4, 2
    ld st, v4
    ret

leaf:
    db 0b00011100, 0b00111110, 0b01111110, 0b11111100, 0b01111000, 0b10000000
