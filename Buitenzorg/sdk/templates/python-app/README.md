# Python App Template (Buitenzorg "Babel")

A polyglot console app in **Python** (indentation-based subset). It runs on
Buitenzorg's in-OS polyglot runtime, not on CPython.

## Run

In the Buitenzorg terminal:

```
script py main.py
# or:
py main.py
# built-in demo:
py
```

## Supported subset

`def` functions + recursion, `if/elif/else`, `while`, `for x in range(...)`,
arithmetic/comparison and `and`/`or`/`not`, string `+` concatenation, and the
builtins `print`, `str`, `len`, `abs`. `True`/`False`/`None` literals.
