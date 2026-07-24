# JavaScript App Template (Buitenzorg "Babel")

A polyglot console app in **JavaScript**. It runs on Buitenzorg's in-OS
polyglot runtime (a shared interpreter for JS/TS/Python with a uniform host
binding API), not on Node.js.

## Run

In the Buitenzorg terminal:

```
script js main.js
# or the shorthand:
js main.js
# with no file, run the built-in demo:
js
```

## Supported subset

Variables (`let`/`var`/`const`), functions + recursion, `if/else`, `while`,
C-style `for`, arithmetic/comparison/logical operators, string `+`
concatenation, and the host builtins `console.log`, `str`, `len`, `abs`.

The same program logic ports directly to the `ts-app` and `python-app`
templates — that's the point of "Babel".
