; Tangkap Buah -- an original CHIP-8 game for Buitenzorg OS.
; Catch the falling fruit with the basket: keypad 4 = left, 6 = right
; (keyboard Q / E in the emulator). Three fruit may fall; then any key
; starts again. Score at the top left, lives at the top right.
; Build: python scripts/chip8asm.py assets/chip8/tangkap.asm assets/chip8/TANGKAP.CH8
;
; registers: vA basket x, vB basket y, vC fruit x, vD fruit y, v5 old basket x,
;            v6 score, v8 lives, v9 frame delay; v0-v4 scratch

start:
    cls
    ld vA, 28
    ld vB, 28
    ld v6, 0
    ld v8, 3
    ld v9, 3
    ld i, basket
    drw vA, vB, 3
    call draw_score
    call draw_lives

new_fruit:
    rnd vC, 0x1F          ; x = 12 + (0..31) + (0..15)
    rnd v0, 0x0F
    add vC, v0
    add vC, 12
    ld vD, 0
    ld i, fruit
    drw vC, vD, 4

loop:
    ld v0, dt
    se v0, 0
    jp loop
    ld dt, v9
    ; the basket: move first, then erase + draw back to back, so no frame
    ; shows it missing
    ld v5, vA
    ld v0, 4
    sknp v0
    call left
    ld v0, 6
    sknp v0
    call right
    ld i, basket
    drw v5, vB, 3
    drw vA, vB, 3
    ; the fruit falls one pixel
    ld i, fruit
    drw vC, vD, 4
    add vD, 1
    drw vC, vD, 4
    se vF, 0
    jp touch
check_floor:
    se vD, 28
    jp loop
    jp miss

touch:                    ; only a touch near the basket counts
    ld v0, 24
    sub v0, vD            ; v0 = 24 - y; VF = 1 when 24 >= y (no borrow)
    se vF, 0
    jp check_floor        ; still high up (it touched a digit)
    jp hit

hit:
    ld i, fruit
    drw vC, vD, 4         ; take the fruit away
    ld i, basket          ; repaint the basket the fruit overlapped
    drw vA, vB, 3
    drw vA, vB, 3
    call draw_score
    add v6, 1
    call draw_score
    ld v0, 3
    ld st, v0             ; a short beep
    ld v0, 10             ; faster after ten
    se v6, v0
    jp new_fruit
    ld v9, 2
    jp new_fruit

miss:
    ld i, fruit
    drw vC, vD, 4
    call draw_lives
    add v8, 0xFF          ; lives - 1
    call draw_lives
    ld v0, 12
    ld st, v0             ; a longer tone
    se v8, 0
    jp new_fruit
game_over:
    ld v0, k
    jp start

left:
    se vA, 0
    add vA, 0xFE
    ret

right:
    se vA, 56
    add vA, 2
    ret

draw_score:               ; two digits of v6 at (1, 1); drawing twice erases
    ld i, digits
    ld b, v6
    ld v2, [i]
    ld v3, 1
    ld v4, 1
    ld f, v1
    drw v3, v4, 5
    ld v3, 6
    ld f, v2
    drw v3, v4, 5
    ret

draw_lives:               ; one digit of v8 at (59, 1)
    ld v3, 59
    ld v4, 1
    ld f, v8
    drw v3, v4, 5
    ret

basket:
    db 0b10000001, 0b10000001, 0b11111111
fruit:
    db 0b00100000, 0b01110000, 0b11111000, 0b01110000
digits:
    db 0, 0, 0
