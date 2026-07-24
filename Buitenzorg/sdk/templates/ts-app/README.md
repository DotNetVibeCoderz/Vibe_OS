# TypeScript App Template (Buitenzorg "Babel")

A polyglot console app in **TypeScript**. The polyglot runtime transpiles TS to
JS (it strips `interface`/`type` declarations and `: Type` annotations) and then
runs it — a real, minimal transpile step, not `tsc`.

## Run

In the Buitenzorg terminal:

```
script ts main.ts
# or:
ts main.ts
# built-in demo:
ts
```

Supported subset matches the `js-app` template (types are erased before
execution). See `js-app/README.md`.
