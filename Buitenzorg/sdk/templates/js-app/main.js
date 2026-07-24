// JavaScript app template for Buitenzorg (v0.14 "Babel").
// Runs on the polyglot runtime: shell `script js main.js` (or `js main.js`).
// Uniform host binding: console.log(...) prints to the terminal.

function greet(name) {
  return "Halo dari " + name + "!";
}

console.log(greet("JavaScript"));

let total = 0;
for (let i = 1; i <= 5; i = i + 1) {
  total = total + i;
}
console.log("1..5 = " + total);
