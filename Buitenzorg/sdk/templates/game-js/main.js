// __APPNAME__ -- a 2D game as a JavaScript UI script (bz new game-js).
// Catch the falling leaves with the basket: arrow keys or A/D, Enter to
// play again. Runs in its own window: run /disk/__NAME83__.JS
//
// Game functions of the UI script API:
//   keydown("left") / keydown("a") / keydown("space")  is a key held now?
//   sprite(x, y, "rows|of|#.", scale, color[, color2]) a pixel picture
//   hit(x1, y1, w1, h1, x2, y2, w2, h2)                do two boxes overlap?
//   sound(hz, ms)                                      a tone
//   random(n)                                          0 .. n-1
// (the script language has no arrays: two leaves are two pairs of variables)

let W = 320;
let H = 240;
let LEAF = "..##..|.####.|######|.####.|..##..";

window("__APPNAME__", W, H);

let bx = 136;
let score = 0;
let missed = 0;
let ax = random(W - 24);
let ay = -10;
let bxl = random(W - 24);
let byl = -130;

function drop(y) {
    return y + 2 + score / 5;
}

while (running()) {
    if (missed < 3) {
        if (keydown("left") || keydown("a")) { bx = bx - 4; }
        if (keydown("right") || keydown("d")) { bx = bx + 4; }
        bx = max(0, min(W - 48, bx));
        ay = drop(ay);
        byl = drop(byl);
        if (hit(ax, ay, 24, 20, bx, H - 18, 48, 10)) {
            score = score + 1;
            sound(880, 60);
            ax = random(W - 24);
            ay = -10;
        }
        if (hit(bxl, byl, 24, 20, bx, H - 18, 48, 10)) {
            score = score + 1;
            sound(880, 60);
            bxl = random(W - 24);
            byl = -30;
        }
        if (ay > H) { missed = missed + 1; sound(220, 120); ax = random(W - 24); ay = -10; }
        if (byl > H) { missed = missed + 1; sound(220, 120); bxl = random(W - 24); byl = -30; }
    } else {
        if (keydown("enter") || keydown("space")) { score = 0; missed = 0; ay = -10; byl = -130; }
    }

    clear(rgb(18, 48, 31));
    sprite(ax, ay, LEAF, 4, rgb(124, 196, 164));
    sprite(bxl, byl, LEAF, 4, rgb(160, 210, 120));
    rect(bx, H - 18, 48, 10, rgb(201, 139, 74), 4);
    text(8, 8, "Skor " + str(score) + "   Terlewat " + str(missed) + " / 3", rgb(240, 255, 224));
    if (missed >= 3) {
        text(W / 2 - 40, H / 2 - 8, "Selesai", rgb(255, 220, 120), 2);
        text(W / 2 - 80, H / 2 + 20, "Enter untuk main lagi", rgb(240, 255, 224));
    }
    present();
}
print("__APPNAME__: skor " + str(score));
