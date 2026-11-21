// __APPNAME__ -- a JavaScript UI script using the OS's language model, made with bz new.
// Device: run /disk/__NAME83__.JS  (a UI script runs in its own window, not with `script js`)
// AI: aiload([file]), aistart(prompt, tokens[, temperature/1000]) + ainext() ("" = done),
//     aigenerate(prompt, tokens[, temperature/1000]), aibackend(), aiinfo(6) = parameters, aispeed().
window("__APPNAME__", 520, 360);
aiload();
let story = "";
let writing = false;
let info = aibackend() + ", " + aiinfo(6) + " parameters";

while (running()) {
  clear(0x14201A);
  text(20, 14, "__APPNAME__", 0xFFFFFF, 2);
  let prompt = input(20, 56, 360, "prompt", "Once upon a time");
  if ((button(392, 52, 108, 36, "Write") || key() == 10) && !writing) {
    if (len(trim(prompt)) == 0) { prompt = "Once upon a time"; }
    story = prompt;
    aistart(prompt, 60);
    writing = true;
  }
  if (writing) {
    let piece = ainext();
    if (piece == "") { writing = false; info = aispeed() + " tokens/s on " + aibackend(); }
    else { story = story + piece; }
  }
  // 58 characters per line
  let y = 108;
  let pos = 0;
  while (pos < len(story)) {
    text(20, y, substr(story, pos, 58), 0xF0FFE0, 1);
    pos = pos + 58;
    y = y + 20;
  }
  text(20, 330, info, 0xF3C969, 1);
  present();
}