// TypeScript app template for Buitenzorg (v0.14 "Babel").
// The runtime transpiles TS -> JS (strips types) then runs it:
// shell `script ts main.ts` (or `ts main.ts`).

interface Greeting {
  who: string;
}

function greet(name: string): string {
  return "Halo dari " + name + "!";
}

console.log(greet("TypeScript"));

let total: number = 0;
for (let i: number = 1; i <= 5; i = i + 1) {
  total = total + i;
}
console.log("1..5 = " + total);
