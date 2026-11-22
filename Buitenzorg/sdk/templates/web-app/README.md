# __APPNAME__ -- a web app (HTML + CSS)

Created by `bz new web-app`: pages shown by Buitenzorg's browser, Peramban.
File names are 8.3 upper case (`INDEX.HTM`, `STYLE.CSS`) because they live on
the FAT `/disk`.

```
bz deploy INDEX.HTM --no-run
bz deploy STYLE.CSS --no-run
bz deploy ABOUT.HTM --no-run
bz run "run mweb /disk/INDEX.HTM"
```

or pack the folder (`bz pack .`) and install the package with `bz install`.
The browser understands HTML text and lists, links, pictures and a CSS subset
(colors, backgrounds, borders, margins, padding, font size) -- no scripts.
Guide: `docs/internet.md`.
