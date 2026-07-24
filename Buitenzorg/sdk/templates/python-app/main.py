# Python app template for Buitenzorg (v0.14 "Babel").
# Runs on the polyglot runtime (subset, indentation-based):
# shell `script py main.py` (or `py main.py`).
# Uniform host binding: print(...) prints to the terminal.

def greet(name):
    return "Halo dari " + name + "!"

print(greet("Python"))

total = 0
for i in range(1, 6):
    total = total + i

print("1..5 = " + str(total))
