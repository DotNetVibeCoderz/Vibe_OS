# __APPNAME__ -- a notes web app

Created by `bz new notes-web`: a page for Buitenzorg's browser, Peramban,
that adds, edits and deletes notes with JavaScript. The notes live in
`localStorage`, which Peramban keeps per site on the device
(`/disk/APPDATA/MWEB/STORE/<hash>.TXT`, at most 512 KiB per site), so they are
still there after the browser or the machine restarts.

* `INDEX.HTM` -- the form, the list and the script (`load` / `store` read and
  write the whole list as one JSON value; `render` rebuilds the list with
  `innerHTML`; the form's `onsubmit` returns false so the page stays put).
* `STYLE.CSS` -- colours from the desktop's *kebun* theme.

File names are 8.3 upper case because they live on the FAT `/disk`.

```
bz deploy INDEX.HTM --no-run
bz deploy STYLE.CSS --no-run
bz run "run mweb /disk/INDEX.HTM"
```

or pack the folder (`bz pack .`) and install it with `bz install`.
Scripts Peramban runs: the DOM (getElementById, querySelector, innerHTML,
textContent, value, events), timers, JSON, localStorage / sessionStorage.
Guide: `docs/internet.md`.
