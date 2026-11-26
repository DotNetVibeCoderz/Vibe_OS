# Desktop features: notifications, search, personalization, display

*Bahasa Indonesia: [desktop-features.id.md](desktop-features.id.md)*

What the Buitenzorg desktop offers beyond windows and the Start menu, and how
programs reach it.

## Notifications and the action center

A program posts a notification with the `NOTIFY` syscall (73: title, body).
The desktop shows it as a toast in the top-right corner for ~4.5 s and keeps
the last 50 in the **action center**: click the bell in the tray (a dot marks
unread ones), then remove one or clear them all. The kernel posts its own too
(a USB disk plugged in, a screenshot saved, an app that failed to start).

| From | How |
|---|---|
| C# (managed) | `Buitenzorg.App.Notifications.Show("Unduhan", "selesai")` |
| UI scripts | `notify("Unduhan", "selesai")` |
| Shell | `notify Judul isi...`, `notify list`, `notify read`, `notify rm 3`, `notify clear` |

## Global search

Type in the Start menu's search box (or `cari WORD` in the shell): apps,
settings pages and files/folders on every mounted volume (root and one folder
level) show up as you type. Enter or a click opens the hit: an app starts, a
setting opens Pengaturan, a file or folder opens in Berkas
(`mfiles --open PATH`).

## Windows

* Opening and closing a window is animated (it grows / shrinks from its
  centre); the `ANIMATIONS` setting turns it off.
* Double-click a title bar to maximize or restore the window.
* Shutdown and restart from the Start menu ask first (Enter confirms, Esc
  cancels).

## Personalization (Pengaturan > Tampilan)

| Setting | Key | Takes effect |
|---|---|---|
| Theme | `THEME` (0) | at once; built-in themes + theme packages |
| Text size | `UI_SCALE` (13): 100 / 110 / 125 % | at once |
| Icon pack | `ICON_PACK` (14): built-in, then `/disk/ICONS/*.BIN` | at once |
| Screen resolution | `RESOLUTION` (12): 800x600 .. 1920x1080 | next start; QEMU / Bochs / VirtualBox std VGA only (elsewhere the firmware's mode stays). The kernel heap grows for the bigger screen (at most half of RAM); std VGA widths are multiples of 8, so 1366x768 is shown as 1360x768 |

**Kept on disk:** theme, wallpaper, screensaver + delay, animations, cursor,
text scale, icon pack and resolution are written to `/disk/DESKTOP.CFG`
whenever they change (from Pengaturan, the shell's `theme` / `bg` / `saver`,
or the Start menu) and restored on the next start.

### Theme packages

A theme package is a text file `/disk/THEMES/NAME.THM` (8.3 name):

```
name=senja
base=kebun            # a built-in theme; keys left out come from it
dark=1
desktop_top=#2B1B2E
desktop_bottom=#6B3A3A
accent=#F2A65A
title_style=flat      # flat | beveled | tab
shadow_style=soft     # none | soft | hard
glass=170             # 0-255, translucency of the shell panels
```

Keys: `name`, `base`, `dark`, `desktop_top`, `desktop_bottom`, `gradient`,
`win_body`, `win_title_active`, `win_title_inactive`, `win_border`, `border`
(px), `title_style`, `shadow_style`, `text`, `title_text`, `taskbar_bg`,
`accent`, `cursor`, `shadow`, `glass`. Every image carries the sample
`SENJA.THM` (`assets/themes/senja.thm`). Ship a theme in a `.bzpkg` with the
file as `package/THEMES/NAME.THM`; `theme reload` (or the next start) picks it
up, `theme senja` or Pengaturan switches to it.

### Icon packs

An icon pack is a BZIC file `/disk/ICONS/NAME.BIN` (the format of
`/disk/ICONS.BIN`, same icon sizes: 44 and 18 px). It is laid over the
built-in icons: an app the pack has an icon for uses it, the others keep
theirs. `icons list`, `icons daun`, `icons bawaan` in the shell; the sample
pack `DAUN.BIN` (every icon in two leaf greens) is built by bzimage.

## Network: tray icon, Jaringan, Pengaturan > Jaringan

The tray has a network icon left of the bell: a screen when a cable is up, signal
bars when Wi-Fi is up, a dimmed screen with a red mark when only the loopback
exists. The kernel keeps the link kind in an atomic (`net::link_kind()`, set by
`net::attach` from `NetDevice::wireless()`), so drawing it takes no lock; the
desktop loop redraws when it changes (DHCP finishes in the background).

Clicking it opens **Jaringan** (`mjaring`, also in Start > Pengaturan): the link,
IPv4/prefix, gateway, DNS, MAC, IPv6, "Sambungkan ulang" (= `netup`), and -- when
the machine has a Wi-Fi adapter whose driver can join -- scan, a network list with
signal bars, SSID + password and Sambungkan / Lupakan. The same panel
(`MJaring.NetworkPanel`) is Pengaturan's **Jaringan** page (`run msetting --page 6`).

Under it: shell `netinfo` (key=value: `link`, `mac`, `ip`, `prefix`, `gateway`,
`dns`, `tx`, `rx`, `ipv6`, `status`, `wifi`, `wifi_scan`, `wifi_saved`),
`wifi scan --raw` (`@bss<TAB>rssi<TAB>channel<TAB>wpa|open<TAB>ssid`), and
`wifi connect 'SSID' 'PASSWORD'` -- `wifi` now takes the tokenized words, so names
and passwords may contain spaces. Managed: `Buitenzorg.Net.NetworkConfig`
(BZAPP.DLL `NetConfig.cs`: `Status`, `Scan`, `Connect`, `Forget`, `Renew`; off the
device a simulated machine). Joining works only where `wifi connect` works: the
Raspberry Pi 4 (CYW43455, verified on a board) and the Intel 8260/8265 (verified
on the Latitude 7290). Screenshots `docs/img/network-app.png`,
`docs/img/settings-network.png`.

## Display: zero-copy surfaces, vsync, full screen (`WIN_CTL`, 74)

| Operation | What it does |
|---|---|
| `SURFACE` | maps two client-sized 32-bit buffers into the program; the compositor reads the shown one in place (no BLIT copy). They stay pinned: MUNMAP refuses them |
| `SURFACE_PRESENT` | show buffer 0 or 1 from now on (draw into the other one) |
| `VSYNC_WAIT` / `VSYNC_INFO` | block until the next vertical blank / refresh rate + counter. The source is the display's (Intel pipe A frame counter, used once it is seen moving -- transcoder A or EDP) or a 60 Hz timer |
| `FULLSCREEN` | the window's client area becomes the whole screen without frame or taskbar; off restores it. Refused for a window with a surface |
| `RESIZABLE` | the program redraws at any size: maximize / restore resize its pixels and send a `RESIZE` pointer event (kind 6, x = width, y = height). Every Buitenzorg.UI window turns it on, so managed apps fill a maximized window |

The C runtime's SDL layer uses them (`SDL_SetWindowFullscreen`,
`SDL_RenderSetVSync`; DOOM presents through the surface), managed apps have
`Display.WaitVsync()` / `Window.SetFullscreen(bool)`, UI scripts `vsync()` /
`fullscreen(on)`. `userland/riscv/surface.rs` is the test (`SURFACE OK`).
